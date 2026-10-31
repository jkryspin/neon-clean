//! Mob Disarm - Mobs stop attacking back
//!
//! Feature 10: Prevent mobs from dealing damage or attacking

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobDisarmConfig {
    pub enabled: bool,
    pub disarm_all: bool,    // Disarm all mobs on map
    pub only_aggro: bool,    // Only disarm mobs that have aggro
    pub preserve_boss: bool, // Don't disarm bosses
}

impl Default for MobDisarmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            disarm_all: true,
            only_aggro: false,
            preserve_boss: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MobDisarmStats {
    pub mobs_disarmed: u32,
    pub damage_blocked: u64,
}

pub struct MobDisarm {
    config: MobDisarmConfig,
    pub active: bool,
    stats: MobDisarmStats,
    disarmed_mobs: Vec<u32>, // Mob IDs that are disarmed
}

impl MobDisarm {
    pub fn new(config: MobDisarmConfig) -> Self {
        Self {
            active: config.enabled,
            config,
            stats: MobDisarmStats::default(),
            disarmed_mobs: vec![],
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(MobDisarmConfig::default())
    }

    pub fn enable(&mut self) {
        self.active = true;
        self.config.enabled = true;
    }

    pub fn disable(&mut self) {
        self.active = false;
        self.config.enabled = false;
        self.disarmed_mobs.clear();
    }

    pub fn toggle(&mut self) {
        self.active = !self.active;
        self.config.enabled = self.active;
    }

    /// Check if a mob should be disarmed
    pub fn should_disarm(&self, mob_id: u32, is_boss: bool, has_aggro: bool) -> bool {
        if !self.active {
            return false;
        }

        // Preserve bosses if configured
        if self.config.preserve_boss && is_boss {
            return false;
        }

        // Only aggro mobs if configured
        if self.config.only_aggro && !has_aggro {
            return false;
        }

        true
    }

    /// Disarm a mob
    pub fn disarm_mob(&mut self, mob_id: u32) -> bool {
        if self.disarmed_mobs.contains(&mob_id) {
            return false; // Already disarmed
        }

        self.disarmed_mobs.push(mob_id);
        self.stats.mobs_disarmed += 1;
        true
    }

    /// Re-arm a mob (when it respawns or leaves)
    pub fn rearm_mob(&mut self, mob_id: u32) {
        self.disarmed_mobs.retain(|&id| id != mob_id);
    }

    /// Clear all disarmed mobs
    pub fn clear_disarmed(&mut self) {
        self.disarmed_mobs.clear();
    }

    /// Get count of disarmed mobs
    pub fn disarmed_count(&self) -> usize {
        self.disarmed_mobs.len()
    }

    pub fn get_stats(&self) -> MobDisarmStats {
        self.stats.clone()
    }
}
