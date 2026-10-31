//! Auto NPC - Click through NPC dialogs
//!
//! Feature 30: Automatically click through NPC dialogs and notifications

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoNpcConfig {
    pub enabled: bool,
    pub auto_accept: bool,            // Auto accept dialogs
    pub auto_close: bool,             // Auto close notifications
    pub click_delay_ms: u64,          // Delay between clicks
    pub ignore_patterns: Vec<String>, // Dialog patterns to ignore
}

impl Default for AutoNpcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_accept: true,
            auto_close: true,
            click_delay_ms: 100,
            ignore_patterns: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoNpcStats {
    pub dialogs_closed: u32,
    pub notifications_closed: u32,
    pub buttons_clicked: u32,
}

pub struct AutoNpc {
    config: AutoNpcConfig,
    pub enabled: bool,
    stats: AutoNpcStats,
}

impl AutoNpc {
    pub fn new(config: AutoNpcConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: AutoNpcStats::default(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(AutoNpcConfig::default())
    }

    pub fn enable(&mut self) {
        self.enabled = true;
        self.config.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.config.enabled = false;
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        self.config.enabled = self.enabled;
    }

    /// Record a dialog close
    pub fn record_dialog_close(&mut self) {
        self.stats.dialogs_closed += 1;
    }

    /// Record a notification close
    pub fn record_notification_close(&mut self) {
        self.stats.notifications_closed += 1;
    }

    /// Record a button click
    pub fn record_button_click(&mut self) {
        self.stats.buttons_clicked += 1;
    }

    pub fn should_ignore(&self, text: &str) -> bool {
        self.config
            .ignore_patterns
            .iter()
            .any(|pattern| text.contains(pattern))
    }

    pub fn get_stats(&self) -> AutoNpcStats {
        self.stats.clone()
    }
}
