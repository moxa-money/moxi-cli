use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

use crate::model::check_owner;

pub fn load(root: &Path) -> Result<BTreeMap<String, String>> {
    let path = root.join("owners.toml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut owners = BTreeMap::new();
    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((repo, agent)) = line.split_once('=') else {
            bail!("{}:{} expected repo = AGENT", path.display(), line_no + 1);
        };
        let repo = repo.trim();
        let agent = agent.trim();
        if repo.is_empty() {
            bail!("{}:{} missing repo", path.display(), line_no + 1);
        }
        check_owner(agent).with_context(|| format!("{}:{}", path.display(), line_no + 1))?;
        if owners.insert(repo.to_string(), agent.to_string()).is_some() {
            bail!("{}:{} duplicate repo {repo}", path.display(), line_no + 1);
        }
    }
    if owners.is_empty() {
        bail!("{} has no repo owners", path.display());
    }
    Ok(owners)
}

pub fn owner<'a>(owners: &'a BTreeMap<String, String>, repo: &str) -> Result<&'a str> {
    owners.get(repo).map(String::as_str).with_context(|| {
        format!("repo {repo} has no owner in owners.toml")
    })
}
