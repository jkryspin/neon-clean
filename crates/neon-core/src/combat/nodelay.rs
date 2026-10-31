//! No Delay - Attack delay removed, reposition at cap
//!
//! Feature 8: Remove attack delay and reposition when at attack cap

use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoDelayConfig {
    pub enabled: bool,
    pub attack_cap: u32,          // Number of attacks before reposition
    pub reposition_distance: i16, // How far to move when repositioning
    pub maintain_range: bool,     // Try to maintain optimal range
}

impl Default for NoDelayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            attack_cap: 10,
            reposition_distance: 50,
            maintain_range: true,
        }
    }
}

pub struct NoDelay {
    config: NoDelayConfig,
    pub active: bool,
    attack_count: u32,
    last_reposition: Option<Instant>,
    total_repositions: u32,
}

impl NoDelay {
    pub fn new(config: NoDelayConfig) -> Self {
        Self {
            active: config.enabled,
            config,
            attack_count: 0,
            last_reposition: None,
            total_repositions: 0,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(NoDelayConfig::default())
    }

    pub fn enable(&mut self) {
        self.active = true;
        self.config.enabled = true;
    }

    pub fn disable(&mut self) {
        self.active = false;
        self.config.enabled = false;
        self.attack_count = 0;
    }

    pub fn toggle(&mut self) {
        self.active = !self.active;
        self.config.enabled = self.active;
    }

    pub fn set_attack_cap(&mut self, cap: u32) {
        self.config.attack_cap = cap.max(1);
    }

    /// Record an attack
    pub fn record_attack(&mut self) {
        self.attack_count += 1;
    }

    /// Check if we should reposition
    pub fn should_reposition(&self) -> bool {
        self.active && self.attack_count >= self.config.attack_cap
    }

    /// Get reposition offset
    pub fn get_reposition_offset(
        &self,
        mob_x: i16,
        mob_y: i16,
        player_x: i16,
        player_y: i16,
    ) -> (i16, i16) {
        if !self.should_reposition() {
            return (0, 0);
        }

        // Calculate direction to mob
        let dx = mob_x as i32 - player_x as i32;
        let dy = mob_y as i32 - player_y as i32;

        // Move perpendicular to maintain range
        let dist = ((dx as f64 * dx as f64 + dy as f64 * dy as f64).sqrt());
        if dist == 0.0 {
            return (self.config.reposition_distance, 0);
        }

        let nx = dy as f64 / dist; // Normal vector
        let ny = -dx as f64 / dist;

        let offset = self.config.reposition_distance as f64;
        ((nx * offset) as i16, (ny * offset) as i16)
    }

    /// Reset attack count after reposition
    pub fn reset_after_reposition(&mut self) {
        self.attack_count = 0;
        self.last_reposition = Some(Instant::now());
        self.total_repositions += 1;
    }

    pub fn get_stats(&self) -> NoDelayStats {
        NoDelayStats {
            active: self.active,
            attack_count: self.attack_count,
            attack_cap: self.config.attack_cap,
            total_repositions: self.total_repositions,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoDelayStats {
    pub active: bool,
    pub attack_count: u32,
    pub attack_cap: u32,
    pub total_repositions: u32,
}
