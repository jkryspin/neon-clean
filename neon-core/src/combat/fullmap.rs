//! Full Map Attack: hit all mobs on the map simultaneously
//!
//! Feature 5: Cast AoE skills at multiple positions to hit everything

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullMapConfig {
    pub enabled: bool,
    pub skill_id: u16,
    pub positions: Vec<(i16, i16)>, // Positions to cast at
    pub auto_positions: bool, // Auto-generate positions
}

impl Default for FullMapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            skill_id: 0,
            positions: vec![],
            auto_positions: true,
        }
    }
}

pub struct FullMapAttack {
    config: FullMapConfig,
    pub active: bool,
    casts_per_cycle: u32,
}

impl FullMapAttack {
    pub fn new(config: FullMapConfig) -> Self {
        Self {
            active: config.enabled,
            config,
            casts_per_cycle: 0,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(FullMapConfig::default())
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

    pub fn set_skill(&mut self, skill_id: u16) {
        self.config.skill_id = skill_id;
    }

    /// Generate positions for full map coverage
    pub fn generate_positions(&mut self, map_width: i16, map_height: i16, skill_range: i16) {
        self.config.positions.clear();

        let spacing = skill_range * 2;
        let start_x = -map_width / 2;
        let start_y = -map_height / 2;

        for y in (start_y..map_height / 2).step_by(spacing as _) {
            for x in (start_x..map_width / 2).step_by(spacing as _) {
                self.config.positions.push((x, y));
            }
        }

        self.casts_per_cycle = self.config.positions.len() as u32;
    }

    /// Add a custom position
    pub fn add_position(&mut self, x: i16, y: i16) {
        self.config.positions.push((x, y));
        self.casts_per_cycle = self.config.positions.len() as u32;
    }

    /// Get all positions to cast at
    pub fn get_positions(&self) -> &Vec<(i16, i16)> {
        &self.config.positions
    }

    /// Clear positions
    pub fn clear_positions(&mut self) {
        self.config.positions.clear();
        self.casts_per_cycle = 0;
    }

    pub fn get_stats(&self) -> FullMapStats {
        FullMapStats {
            active: self.active,
            skill_id: self.config.skill_id,
            position_count: self.config.positions.len(),
            casts_per_cycle: self.casts_per_cycle,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullMapStats {
    pub active: bool,
    pub skill_id: u16,
    pub position_count: usize,
    pub casts_per_cycle: u32,
}
