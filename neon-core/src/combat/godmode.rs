//! God Mode - Stop taking damage
//!
//! Feature 4: Prevent damage packets from affecting the player
//! Feature 4b: Boss variant - tuned for boss fights

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GodModeConfig {
    pub enabled: bool,
    pub boss_variant: bool, // Different behavior for bosses
    pub allow_1hp_drops: bool, // Allow dropping to 1 HP (some bosses check this)
    pub damage_threshold: u32, // Only block damage above this (boss variant)
}

impl Default for GodModeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            boss_variant: false,
            allow_1hp_drops: false,
            damage_threshold: 0,
        }
    }
}

pub struct GodMode {
    config: GodModeConfig,
    pub active: bool,
    damage_blocked: u64,
    damage_count: u32,
}

impl GodMode {
    pub fn new(config: GodModeConfig) -> Self {
        Self {
            active: config.enabled,
            config,
            damage_blocked: 0,
            damage_count: 0,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(GodModeConfig::default())
    }

    pub fn boss_mode() -> Self {
        Self::new(GodModeConfig {
            enabled: true,
            boss_variant: true,
            allow_1hp_drops: true,
            damage_threshold: 1000, // Block most damage but allow small hits
        })
    }

    pub fn enable(&mut self) {
        self.active = true;
        self.config.enabled = true;
    }

    pub fn disable(&mut self) {
        self.active = false;
        self.config.enabled = false;
    }

    pub fn toggle(&mut self) {
        self.active = !self.active;
        self.config.enabled = self.active;
    }

    pub fn set_boss_variant(&mut self, boss: bool) {
        self.config.boss_variant = boss;
    }

    /// Check if damage should be blocked
    pub fn should_block_damage(&self, damage: u32, current_hp: u32, _max_hp: u32) -> bool {
        if !self.active {
            return false;
        }

        // Boss variant: allow some damage through
        if self.config.boss_variant {
            // Allow damage below threshold
            if damage < self.config.damage_threshold {
                return false;
            }
            // Allow 1 HP drops if configured
            if self.config.allow_1hp_drops && current_hp - damage <= 1 {
                return false;
            }
        }

        // Standard god mode: block all damage that would hit
        damage > 0
    }

    /// Process a damage packet (for packet hooking)
    pub fn process_damage_packet(&mut self, damage: u32, current_hp: u32) -> u32 {
        if self.should_block_damage(damage, current_hp, current_hp + damage) {
            self.damage_blocked += damage as u64;
            self.damage_count += 1;
            0 // Block all damage
        } else {
            damage // Let damage through
        }
    }

    /// Get stats
    pub fn get_stats(&self) -> GodModeStats {
        GodModeStats {
            active: self.active,
            boss_variant: self.config.boss_variant,
            damage_blocked: self.damage_blocked,
            damage_count: self.damage_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GodModeStats {
    pub active: bool,
    pub boss_variant: bool,
    pub damage_blocked: u64,
    pub damage_count: u32,
}
