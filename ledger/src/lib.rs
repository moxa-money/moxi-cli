//! Coordination ledger. The JSONL log is the record. SQLite is a projection,
//! compiled into the `comms` binary.

pub mod listen;
pub mod model;
pub mod notes;
pub mod owners;
pub mod store;

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Project root that holds `owners.toml` and `Cargo.toml`.
pub fn find_root(start: &Path) -> Result<PathBuf> {
    let mut cur = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir()?.join(start)
    };
    if cur.is_file() {
        if let Some(parent) = cur.parent() {
            cur = parent.to_path_buf();
        }
    }
    loop {
        if is_comms_root(&cur) {
            return Ok(cur);
        }
        let nested = cur.join("ledger");
        if is_comms_root(&nested) {
            return Ok(nested);
        }
        match cur.parent() {
            Some(parent) => cur = parent.to_path_buf(),
            None => break,
        }
    }
    bail!(
        "ledger not found from {}. Pass --dir or set COMMS_DIR.",
        start.display()
    )
}

fn is_comms_root(path: &Path) -> bool {
    if !path.join("owners.toml").is_file() || !path.join("Cargo.toml").is_file() {
        return false;
    }
    std::fs::read_to_string(path.join("Cargo.toml"))
        .map(|text| text.contains("name = \"comms\""))
        .unwrap_or(false)
}

pub fn resolve_root(flag: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = flag {
        let dir = if dir.is_absolute() {
            dir.to_path_buf()
        } else {
            std::env::current_dir()?.join(dir)
        };
        if !is_comms_root(&dir) {
            bail!("{} is not a ledger root", dir.display());
        }
        return Ok(dir);
    }
    if let Some(env) = std::env::var_os("COMMS_DIR") {
        let dir = PathBuf::from(env);
        if !is_comms_root(&dir) {
            bail!("COMMS_DIR {} is not a ledger root", dir.display());
        }
        return Ok(dir);
    }
    find_root(&std::env::current_dir().context("current directory")?)
}
