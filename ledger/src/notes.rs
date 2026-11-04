//! Idea notes. The file is outside the ledger. A promotion writes a fresh summary only.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::model::{check_agent, SUMMARY_MAX};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub at: String,
    pub author: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promoted_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger_id: Option<u64>,
}

pub struct Notes {
    path: PathBuf,
}

impl Notes {
    pub fn open(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> Result<PathBuf> {
        let home = std::env::var_os("HOME").context("HOME is unset")?;
        Ok(PathBuf::from(home).join(".moxi/notes/notes.jsonl"))
    }

    pub fn create(
        &self,
        author: &str,
        title: &str,
        body: &str,
        tags: Vec<String>,
        source: Option<String>,
    ) -> Result<Note> {
        check_agent("author", author)?;
        let title = title.trim();
        let body = body.trim();
        if title.is_empty() || body.is_empty() {
            bail!("a note needs a title and a body");
        }
        refuse_secret(&format!("{title}\n{body}"))?;
        if let Some(source) = &source {
            refuse_secret(source)?;
        }
        for tag in &tags {
            if tag.trim().is_empty() {
                bail!("a tag is empty");
            }
            refuse_secret(tag)?;
        }
        let mut notes = self.read()?;
        let id = notes.last().map(|note| note.id + 1).unwrap_or(1);
        let note = Note {
            id,
            at: crate::model::now_utc(),
            author: author.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            tags,
            source,
            status: "open".into(),
            promoted_to: None,
            ledger_id: None,
        };
        notes.push(note.clone());
        self.write(&notes)?;
        Ok(note)
    }

    pub fn list(&self) -> Result<Vec<Note>> {
        self.read()
    }

    pub fn show(&self, id: u64) -> Result<Note> {
        self.read()?
            .into_iter()
            .find(|note| note.id == id)
            .with_context(|| format!("no note {id}"))
    }

    pub fn drop(&self, id: u64) -> Result<Note> {
        let mut notes = self.read()?;
        let note = notes
            .iter_mut()
            .find(|note| note.id == id)
            .with_context(|| format!("no note {id}"))?;
        note.status = "dropped".into();
        let dropped = note.clone();
        self.write(&notes)?;
        Ok(dropped)
    }

    /// Check the fresh summary. The note body stays in the file.
    pub fn summary_for_ledger(&self, id: u64, summary: &str) -> Result<String> {
        let note = self.show(id)?;
        if note.status == "dropped" {
            bail!("note {id} is dropped");
        }
        let summary = summary.trim();
        if summary.is_empty() || summary.chars().count() > SUMMARY_MAX {
            bail!("a promotion summary is at most {SUMMARY_MAX} characters");
        }
        if summary == note.body || summary == note.title {
            bail!("a promotion summary is written fresh");
        }
        refuse_secret(summary)?;
        Ok(format!("source: note:{id} {summary}"))
    }

    pub fn mark_promoted(&self, id: u64, promoted_to: &str) -> Result<Note> {
        let mut notes = self.read()?;
        let note = notes
            .iter_mut()
            .find(|note| note.id == id)
            .with_context(|| format!("no note {id}"))?;
        note.status = "promoted".into();
        note.promoted_to = Some(promoted_to.to_string());
        let marked = note.clone();
        self.write(&notes)?;
        Ok(marked)
    }

    fn read(&self) -> Result<Vec<Note>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = std::fs::File::open(&self.path)
            .with_context(|| format!("read {}", self.path.display()))?;
        let mut notes = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            notes.push(serde_json::from_str(&line)?);
        }
        Ok(notes)
    }

    fn write(&self, notes: &[Note]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
            .with_context(|| format!("write {}", self.path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
        }
        for note in notes {
            serde_json::to_writer(&mut file, note)?;
            file.write_all(b"\n")?;
        }
        Ok(())
    }
}

/// Block when gitleaks-shaped text, an AWS key id, a secret assignment, or a high-entropy assignment hits.
/// An email and a bare AKIA prefix do not refuse.
pub fn refuse_secret(text: &str) -> Result<()> {
    if let Some(detector) = secret_detector(text) {
        bail!("the note was refused ({detector})");
    }
    Ok(())
}

pub fn secret_detector(text: &str) -> Option<&'static str> {
    if aws_key_id(text) {
        return Some("aws_key_id");
    }
    if gitleaks(text) {
        return Some("gitleaks");
    }
    if entropy_assignment(text) {
        return Some("entropy");
    }
    if secret_assignment(text) {
        return Some("secret_assignment");
    }
    None
}

fn aws_key_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 20 <= bytes.len() {
        if &bytes[i..i + 4] == b"AKIA" && bytes[i + 4..i + 20].iter().all(|b| b.is_ascii_alphanumeric())
        {
            let end = i + 20;
            let bounded = (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
                && (end == bytes.len() || !bytes[end].is_ascii_alphanumeric());
            if bounded {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn gitleaks(text: &str) -> bool {
    if text.contains("-----BEGIN ") && text.contains("PRIVATE KEY-----") {
        return true;
    }
    if text.contains("hooks.slack.com/services/") {
        return true;
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 24 <= bytes.len() {
        if &bytes[i..i + 4] == b"ghp_" && bytes[i + 4..i + 24].iter().all(|b| b.is_ascii_alphanumeric()) {
            return true;
        }
        i += 1;
    }
    false
}

fn secret_assignment(text: &str) -> bool {
    assignment_values(text).into_iter().any(|(key, value)| {
        secret_key(&key) && value.chars().count() >= 16
    })
}

fn entropy_assignment(text: &str) -> bool {
    assignment_values(text).into_iter().any(|(key, value)| {
        secret_key(&key) && value.chars().count() >= 20 && shannon(&value) >= 3.5
    })
}

fn secret_key(key: &str) -> bool {
    matches!(
        key,
        "token"
            | "api_key"
            | "apikey"
            | "secret"
            | "password"
            | "aws_secret_access_key"
            | "access_key"
    )
}

fn assignment_values(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_start_matches('$').to_ascii_lowercase();
        let rest = rest.trim();
        let value = if let Some(quoted) = rest.strip_prefix('"') {
            quoted.split('"').next().unwrap_or("").to_string()
        } else if let Some(quoted) = rest.strip_prefix('\'') {
            quoted.split('\'').next().unwrap_or("").to_string()
        } else {
            continue;
        };
        found.push((key, value));
    }
    found
}

fn shannon(value: &str) -> f64 {
    if value.is_empty() {
        return 0.0;
    }
    let mut counts = std::collections::BTreeMap::<char, usize>::new();
    for ch in value.chars() {
        *counts.entry(ch).or_default() += 1;
    }
    let len = value.chars().count() as f64;
    counts.values().map(|count| {
        let p = *count as f64 / len;
        -p * p.log2()
    }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sea_cases_match() {
        let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/notes-secret-refusal.json"));
        let raw = std::fs::read_to_string(path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
        for case in doc["cases"].as_array().unwrap() {
            let text = case["text"].as_str().unwrap();
            let expect = case["expect"].as_str().unwrap();
            let hit = secret_detector(text);
            if expect == "refuse" {
                assert!(hit.is_some(), "{} passed: {text}", case["id"]);
            } else {
                assert!(hit.is_none(), "{} refused as {}: {text}", case["id"], hit.unwrap_or(""));
            }
        }
    }

    #[test]
    fn promote_keeps_the_body_out_of_the_ledger_line() {
        let dir = tempfile::tempdir().unwrap();
        let notes = Notes::open(dir.path().join("notes.jsonl"));
        let note = notes
            .create("LEAD", "Later", "Look at cost tags later. This is not a gate.", vec![], None)
            .unwrap();
        let line = notes.summary_for_ledger(note.id, "Cost tags can wait.").unwrap();
        assert!(line.starts_with("source: note:1 "));
        assert!(!line.contains(&note.body));
        assert!(notes.summary_for_ledger(note.id, &note.body).is_err());
        assert!(notes
            .create(
                "LEAD",
                "Later",
                "A plain idea.",
                vec!["ghp_EXAMPLEEXAMPLEEXAMPLEEXAMPLEEXAMPLEX".into()],
                None,
            )
            .is_err());
    }
}
