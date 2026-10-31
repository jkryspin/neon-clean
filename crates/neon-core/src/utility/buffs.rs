//! Auto Buff - Recast buffs before they expire
//!
//! Feature 29: Automatically recast buffs before they drop off

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuffConfig {
    pub skill_id: u16,
    pub name: String,
    pub duration_ms: u64,          // Expected duration
    pub recast_threshold_ms: u64,  // Recast when this many ms remain
    pub key_sequence: Vec<String>, // Keys to press to cast
}

impl BuffConfig {
    pub fn new(skill_id: u16, name: &str, duration_ms: u64) -> Self {
        Self {
            skill_id,
            name: name.to_string(),
            duration_ms,
            recast_threshold_ms: duration_ms / 10, // Default: recast at 10% remaining
            key_sequence: vec![],
        }
    }

    pub fn with_keys(mut self, keys: Vec<&str>) -> Self {
        self.key_sequence = keys.iter().map(|k| k.to_string()).collect();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuffState {
    pub skill_id: u16,
    pub active: bool,
    pub expires_at_ms: u128,
    pub casts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoBuffConfig {
    pub enabled: bool,
    pub buffs: HashMap<u16, BuffConfig>, // skill_id -> config
    pub check_interval_ms: u64,
}

impl Default for AutoBuffConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            buffs: HashMap::new(),
            check_interval_ms: 1000,
        }
    }
}

pub struct AutoBuff {
    config: AutoBuffConfig,
    pub enabled: bool,
    buff_states: HashMap<u16, BuffState>,
}

impl AutoBuff {
    pub fn new(config: AutoBuffConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            buff_states: HashMap::new(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(AutoBuffConfig::default())
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

    /// Register a buff to track
    pub fn register_buff(&mut self, config: BuffConfig) {
        let skill_id = config.skill_id;
        self.config.buffs.insert(skill_id, config);
        self.buff_states.insert(
            skill_id,
            BuffState {
                skill_id,
                active: false,
                expires_at_ms: 0,
                casts: 0,
            },
        );
    }

    /// Mark a buff as active (called when buff is detected)
    pub fn set_buff_active(&mut self, skill_id: u16) {
        if let Some(config) = self.config.buffs.get(&skill_id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(std::time::Duration::ZERO)
                .as_millis();

            if let Some(state) = self.buff_states.get_mut(&skill_id) {
                state.active = true;
                state.expires_at_ms = now + config.duration_ms as u128;
            }
        }
    }

    /// Check if a buff needs to be recast
    pub fn needs_recast(&self, skill_id: u16) -> bool {
        if !self.enabled {
            return false;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_millis();

        // Get buff state
        let state = match self.buff_states.get(&skill_id) {
            Some(s) => s,
            None => return false,
        };

        // Buff not active, should cast
        if !state.active {
            return true;
        }

        // Check if expiring soon
        if let Some(config) = self.config.buffs.get(&skill_id) {
            let time_remaining = state.expires_at_ms.saturating_sub(now);
            return time_remaining <= config.recast_threshold_ms as u128;
        }

        false
    }

    /// Record a buff cast
    pub fn record_cast(&mut self, skill_id: u16) {
        if let Some(state) = self.buff_states.get_mut(&skill_id) {
            state.casts += 1;
            state.active = true;

            if let Some(config) = self.config.buffs.get(&skill_id) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or(std::time::Duration::ZERO)
                    .as_millis();
                state.expires_at_ms = now + config.duration_ms as u128;
            }
        }
    }

    /// Get key sequence for a buff
    pub fn get_keys(&self, skill_id: u16) -> Option<&Vec<String>> {
        self.config.buffs.get(&skill_id).map(|c| &c.key_sequence)
    }

    /// List all registered buffs
    pub fn list_buffs(&self) -> Vec<(u16, &str, bool)> {
        self.buff_states
            .iter()
            .filter_map(|(&id, state)| {
                self.config
                    .buffs
                    .get(&id)
                    .map(|c| (id, c.name.as_str(), state.active))
            })
            .collect()
    }

    pub fn get_stats(&self) -> AutoBuffStats {
        AutoBuffStats {
            enabled: self.enabled,
            registered_count: self.config.buffs.len(),
            active_count: self.buff_states.values().filter(|s| s.active).count(),
            total_casts: self.buff_states.values().map(|s| s.casts).sum(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoBuffStats {
    pub enabled: bool,
    pub registered_count: usize,
    pub active_count: usize,
    pub total_casts: u32,
}
