//! Discovery file that lets the MCP bridge find a running engine.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    pub started_ms: u64,
}

pub fn path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("neon").join("ce").join("session.json")
}

impl Session {
    pub fn write(&self) -> std::io::Result<()> {
        let p = path();
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(p, serde_json::to_vec_pretty(self).unwrap())
    }

    pub fn load() -> Option<Session> {
        let text = std::fs::read_to_string(path()).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn remove() {
        let _ = std::fs::remove_file(path());
    }
}
