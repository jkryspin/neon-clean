//! Freeze list: periodically rewrite a value so the game cannot change it.
//!
//! Writes go through `Process::write`, which widens read-only pages once and remembers
//! the old protection, so freezing an image-page field (as the stats object is) is safe.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::value::VType;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub addr: u64,
    pub vtype: String,
    /// Value as text so it round-trips through JSON and cheat tables unchanged.
    pub value: String,
    #[serde(default = "default_interval")]
    pub interval_ms: u64,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(skip)]
    pub last: Option<Instant>,
    #[serde(skip)]
    pub writes: u64,
    #[serde(skip)]
    pub errors: u64,
}

fn default_interval() -> u64 {
    100
}

fn default_true() -> bool {
    true
}

impl Entry {
    pub fn vtype(&self) -> VType {
        VType::parse(&self.vtype).unwrap_or(VType::U32)
    }

    pub fn bytes(&self) -> Option<Vec<u8>> {
        self.vtype().encode(&self.value).ok()
    }
}

/// Ids come from one counter so sessions can share a list.
pub fn next_id(counter: &Arc<AtomicU64>) -> u64 {
    counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Entries that are due now, cloned so callers can drop the lock.
pub fn due(entries: &mut [Entry], now: Instant) -> Vec<Entry> {
    let mut out = Vec::new();
    for e in entries.iter_mut() {
        if !e.active {
            continue;
        }
        if e.last.map(|t| now.duration_since(t) >= Duration::from_millis(e.interval_ms)).unwrap_or(true)
        {
            e.last = Some(now);
            out.push(e.clone());
        }
    }
    out
}
