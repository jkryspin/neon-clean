//! Movement Unlocks - Infinite flash jump, no air check
//!
//! Feature 9: Unlock movement restrictions for enhanced mobility

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovementUnlocksConfig {
    pub enabled: bool,
    pub infinite_flash_jump: bool, // No cooldown on flash jump
    pub no_air_check: bool,        // Use skills in air without restrictions
    pub infinite_dash: bool,       // No cooldown on dash skills
    pub ignore_stun: bool,         // Ignore stun effects
    pub ignore_sleep: bool,        // Ignore sleep effects
}

impl Default for MovementUnlocksConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            infinite_flash_jump: false,
            no_air_check: false,
            infinite_dash: false,
            ignore_stun: false,
            ignore_sleep: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MovementUnlocksStats {
    pub flash_jumps_used: u32,
    pub air_skills_used: u32,
    pub dashes_used: u32,
    pub stuns_ignored: u32,
    pub sleeps_ignored: u32,
}

pub struct MovementUnlocks {
    config: MovementUnlocksConfig,
    pub enabled: bool,
    stats: MovementUnlocksStats,
}

impl MovementUnlocks {
    pub fn new(config: MovementUnlocksConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: MovementUnlocksStats::default(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(MovementUnlocksConfig::default())
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

    /// Enable infinite flash jump
    pub fn enable_infinite_flash_jump(&mut self) {
        self.config.infinite_flash_jump = true;
    }

    /// Disable infinite flash jump
    pub fn disable_infinite_flash_jump(&mut self) {
        self.config.infinite_flash_jump = false;
    }

    /// Enable no air check
    pub fn enable_no_air_check(&mut self) {
        self.config.no_air_check = true;
    }

    /// Disable no air check
    pub fn disable_no_air_check(&mut self) {
        self.config.no_air_check = false;
    }

    /// Check if flash jump should have cooldown removed
    pub fn should_remove_flash_jump_cooldown(&self) -> bool {
        self.enabled && self.config.infinite_flash_jump
    }

    /// Check if air skill should be allowed
    pub fn should_allow_air_skill(&self, skill_id: u16) -> bool {
        if !self.enabled || !self.config.no_air_check {
            return false;
        }
        // All skills allowed in air when no_air_check is enabled
        true
    }

    /// Check if stun should be ignored
    pub fn should_ignore_stun(&self) -> bool {
        self.enabled && self.config.ignore_stun
    }

    /// Check if sleep should be ignored
    pub fn should_ignore_sleep(&self) -> bool {
        self.enabled && self.config.ignore_sleep
    }

    /// Record a flash jump usage
    pub fn record_flash_jump(&mut self) {
        self.stats.flash_jumps_used += 1;
    }

    /// Record an air skill usage
    pub fn record_air_skill(&mut self) {
        self.stats.air_skills_used += 1;
    }

    /// Record a dash usage
    pub fn record_dash(&mut self) {
        self.stats.dashes_used += 1;
    }

    /// Record a stun ignore
    pub fn record_stun_ignore(&mut self) {
        self.stats.stuns_ignored += 1;
    }

    /// Record a sleep ignore
    pub fn record_sleep_ignore(&mut self) {
        self.stats.sleeps_ignored += 1;
    }

    pub fn get_stats(&self) -> MovementUnlocksStats {
        self.stats.clone()
    }
}
