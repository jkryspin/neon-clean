//! IGN whitelist: trusted players

use std::collections::HashSet;

pub struct IgnWhitelist {
    pub igns: HashSet<String>,
}

impl IgnWhitelist {
    pub fn new() -> Self {
        Self {
            igns: HashSet::new(),
        }
    }

    pub fn add(&mut self, ign: String) {
        self.igns.insert(ign.to_lowercase());
    }

    pub fn remove(&mut self, ign: &str) -> bool {
        self.igns.remove(&ign.to_lowercase())
    }

    pub fn is_whitelisted(&self, ign: &str) -> bool {
        self.igns.contains(&ign.to_lowercase())
    }
}
