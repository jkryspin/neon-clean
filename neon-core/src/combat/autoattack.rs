//! Auto attack: timed attack key press

use std::time::Duration;

pub struct AutoAttack {
    pub enabled: bool,
    pub interval_ms: u32,
}

impl AutoAttack {
    pub fn new() -> Self {
        Self {
            enabled: false,
            interval_ms: 100,
        }
    }

    pub fn set_interval(&mut self, ms: u32) {
        self.interval_ms = ms.max(50);
    }

    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms as u64)
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }
}
