//! One listener per agent. An agent wakes only when a new open notice is
//! addressed to that agent. Mail they sent, a note, and an ack do not wake
//! them. FOUNDER wakes on any inbox change so the dash can refresh. The notice
//! body stays out of the line.

use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{stdout, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::model::check_agent;
use crate::store::Store;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ListenSnap {
    pub to_open: BTreeSet<u64>,
    pub from_open: BTreeSet<u64>,
    /// Channel posts. They are not inbox mail. FOUNDER's dash refreshes when this changes.
    #[serde(default)]
    pub channel: BTreeSet<u64>,
}

impl ListenSnap {
    pub fn capture(store: &Store, agent: &str) -> Result<Self> {
        let to = store.messages(None, Some(agent), None, false)?;
        let from = store.messages(Some(agent), None, None, false)?;
        let posted = store.channel_messages(None, None, None, false, 3)?;
        Ok(Self {
            to_open: to.into_iter().map(|row| row.id).collect(),
            from_open: from.into_iter().map(|row| row.id).collect(),
            channel: posted
                .into_iter()
                .filter(|row| row.kind == "notice")
                .map(|row| row.id)
                .collect(),
        })
    }
}

/// An agent turn starts only for a new notice addressed to that agent.
/// FOUNDER's dash refreshes when any open notice appears or closes.
pub fn inbox_wakes(agent: &str, prev: &ListenSnap, next: &ListenSnap) -> bool {
    wakes(agent, prev, next, false)
}

/// The plan owner also wakes when a need they sent leaves the open set.
/// That is a spoke marking it did. A new outbound need does not wake them.
pub fn orch_wakes(agent: &str, prev: &ListenSnap, next: &ListenSnap) -> bool {
    wakes(agent, prev, next, true)
}

fn wakes(agent: &str, prev: &ListenSnap, next: &ListenSnap, orch: bool) -> bool {
    if agent == "FOUNDER" {
        return prev != next;
    }
    if next.to_open.difference(&prev.to_open).next().is_some() {
        return true;
    }
    orch && prev.from_open.difference(&next.from_open).next().is_some()
}

pub fn wake_line(agent: &str) -> String {
    wake_prompt(
        agent,
        "Check comms for {agent}. Run comms messages --to {agent} --json. ackn a tell that is a fact. An answered row is work for the asker. Reply to a need, or mark it did when the work is done.",
    )
}

pub fn orch_wake_line(agent: &str) -> String {
    wake_prompt(
        agent,
        "Orchestrator check for {agent}. Run comms messages --to {agent} --json. If the inbox is empty, a spoke left a need answered. That answered row is work for the asker. Show that plan and send the next slice. Do not wait on an ackn.",
    )
}

fn wake_prompt(agent: &str, prompt: &str) -> String {
    format!(
        "AGENT_LOOP_WAKE_comms-{agent} {{\"prompt\":\"{}\"}}",
        prompt.replace("{agent}", agent)
    )
}

struct ListenLock {
    file: fs::File,
}

impl Drop for ListenLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn claim(store: &Store, agent: &str) -> Result<ListenLock> {
    fs::create_dir_all(store.data_dir())?;
    let path = store.data_dir().join(format!("listen-{agent}.pid"));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    if file.try_lock_exclusive().is_err() {
        bail!("comms listen for {agent} is already running");
    }
    file.set_len(0)?;
    writeln!(file, "{}", std::process::id())?;
    Ok(ListenLock { file })
}

fn snap_path(store: &Store, agent: &str) -> PathBuf {
    store.data_dir().join(format!("listen-{agent}.json"))
}

fn load_snap(store: &Store, agent: &str) -> Option<ListenSnap> {
    let raw = fs::read(snap_path(store, agent)).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn save_snap(store: &Store, agent: &str, snap: &ListenSnap) -> Result<()> {
    let path = snap_path(store, agent);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec(snap)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn prompt_line(agent: &str, orch: bool) -> String {
    if orch {
        orch_wake_line(agent)
    } else {
        wake_line(agent)
    }
}

fn log_stamp(store: &Store) -> Option<(u64, SystemTime)> {
    let meta = fs::metadata(store.data_dir().join("events.jsonl")).ok()?;
    Some((
        meta.len(),
        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    ))
}

/// Follow the ledger until the process is stopped. `refresh` is how often the
/// open inbox is read again even when the log file does not change.
pub fn run(store: &Store, agent: &str, refresh: Duration, orch: bool) -> Result<()> {
    check_agent("listen agent", agent)?;
    if refresh < Duration::from_secs(1) {
        bail!("listen refresh is at least 1 second");
    }
    let _lock = claim(store, agent)?;
    let mut out = stdout();
    let fresh = ListenSnap::capture(store, agent)?;
    if let Some(saved) = load_snap(store, agent) {
        if wakes(agent, &saved, &fresh, orch) {
            writeln!(out, "{}", prompt_line(agent, orch))?;
            out.flush()?;
        }
    }
    let mut snap = fresh;
    save_snap(store, agent, &snap)?;
    let mut stamp = log_stamp(store);
    let mut last_full = std::time::Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let next_stamp = log_stamp(store);
        let due = last_full.elapsed() >= refresh;
        if next_stamp == stamp && !due {
            continue;
        }
        stamp = next_stamp;
        last_full = std::time::Instant::now();
        let next = ListenSnap::capture(store, agent)?;
        if next == snap {
            continue;
        }
        if wakes(agent, &snap, &next, orch) {
            writeln!(out, "{}", prompt_line(agent, orch))?;
            out.flush()?;
        }
        snap = next;
        save_snap(store, agent, &snap)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("owners.toml"),
            "example-platform = PLATFORM\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"comms\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let store = Store::new(dir.path().to_path_buf());
        store.init().unwrap();
        (dir, store)
    }

    fn plan(dir: &Path, store: &Store) -> u64 {
        let path = dir.join("plan.md");
        fs::write(&path, "plan").unwrap();
        store
            .plan_open(
                "PLATFORM-1".into(),
                path,
                "example-platform".into(),
                vec![],
                "A plan".into(),
                vec![],
                vec![],
            )
            .unwrap()
            .id
    }

    #[test]
    fn a_new_notice_changes_the_inbox_and_an_answer_clears_it() {
        let (dir, store) = fixture();
        let id = plan(dir.path(), &store);
        let before = ListenSnap::capture(&store, "FOUNDER").unwrap();
        assert!(before.to_open.is_empty());
        let sent = store
            .tell(
                id,
                "LEAD".into(),
                Some("FOUNDER".into()),
                false,
                "tell".into(),
                "hello".into(),
                vec![],
                vec![],
                crate::model::DeskNeed::default(),
            )
            .unwrap();
        let open = ListenSnap::capture(&store, "FOUNDER").unwrap();
        assert_ne!(before, open);
        assert!(open.to_open.contains(&sent[0].id));
        store
            .ack(sent[0].id, "FOUNDER".into(), "ackn".into(), "ok".into(), vec![])
            .unwrap();
        let closed = ListenSnap::capture(&store, "FOUNDER").unwrap();
        assert!(!closed.to_open.contains(&sent[0].id));
        let from_ent = ListenSnap::capture(&store, "LEAD").unwrap();
        assert!(!from_ent.from_open.contains(&sent[0].id));
    }

    #[test]
    fn an_agent_wakes_only_when_inbound_mail_arrives() {
        let prev = ListenSnap {
            to_open: BTreeSet::from([1]),
            from_open: BTreeSet::from([2]),
            channel: BTreeSet::new(),
        };
        let inbound = ListenSnap {
            to_open: BTreeSet::from([1, 3]),
            from_open: BTreeSet::from([2]),
            channel: BTreeSet::new(),
        };
        assert!(inbox_wakes("LEAD", &prev, &inbound));
        let sent = ListenSnap {
            to_open: BTreeSet::from([1]),
            from_open: BTreeSet::from([2, 4]),
            channel: BTreeSet::new(),
        };
        assert!(!inbox_wakes("LEAD", &prev, &sent));
        let acked = ListenSnap {
            to_open: BTreeSet::new(),
            from_open: BTreeSet::from([2]),
            channel: BTreeSet::new(),
        };
        assert!(!inbox_wakes("GUARD", &prev, &acked));
        assert!(inbox_wakes("FOUNDER", &prev, &sent));
        assert!(inbox_wakes("FOUNDER", &prev, &acked));
        let posted = ListenSnap {
            to_open: BTreeSet::from([1]),
            from_open: BTreeSet::from([2]),
            channel: BTreeSet::from([9]),
        };
        assert!(!inbox_wakes("LEAD", &prev, &posted));
        assert!(inbox_wakes("FOUNDER", &prev, &posted));
        let answered = ListenSnap {
            to_open: BTreeSet::from([1]),
            from_open: BTreeSet::new(),
            channel: BTreeSet::new(),
        };
        assert!(!inbox_wakes("LEAD", &prev, &answered));
        assert!(orch_wakes("LEAD", &prev, &answered));
        assert!(!orch_wakes("LEAD", &prev, &sent));
    }

    #[test]
    fn the_wake_line_names_the_agent_and_not_the_body() {
        let line = wake_line("FOUNDER");
        assert!(line.starts_with("AGENT_LOOP_WAKE_comms-FOUNDER "));
        assert!(!line.contains("hello"));
        assert!(line.contains("comms messages --to FOUNDER"));
        assert!(line.contains("answered"));
        assert!(orch_wake_line("LEAD").contains("answered"));
    }

    #[test]
    fn the_saved_snap_reloads() {
        let (_dir, store) = fixture();
        let snap = ListenSnap {
            to_open: BTreeSet::from([7]),
            from_open: BTreeSet::from([8]),
            channel: BTreeSet::new(),
        };
        save_snap(&store, "FOUNDER", &snap).unwrap();
        assert_eq!(load_snap(&store, "FOUNDER"), Some(snap));
    }
}
