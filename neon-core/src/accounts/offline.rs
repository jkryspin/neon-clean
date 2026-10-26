//! Offline mode: log in without appearing online

pub struct OfflineMode {
    pub enabled: bool,
}

impl OfflineMode {
    pub fn new() -> Self {
        Self { enabled: false }
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }
}
