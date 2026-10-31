//! Auto relaunch: restart client if it dies

use std::time::Duration;

pub struct AutoRelaunch {
    pub enabled: bool,
    pub delay_seconds: u64,
    pub max_relaunches: u32,
}

impl AutoRelaunch {
    pub fn new() -> Self {
        Self {
            enabled: false,
            delay_seconds: 30,
            max_relaunches: 5,
        }
    }

    pub fn delay(&self) -> Duration {
        Duration::from_secs(self.delay_seconds)
    }
}
