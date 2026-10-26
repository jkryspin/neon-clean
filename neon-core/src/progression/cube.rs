//! Auto Cube - Roll until conditions met
//!
//! Feature 12: Automatically cube equipment until AND/OR conditions are satisfied

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CubeCondition {
    StatIncrease(u32), // Stat ID that must increase
    StatValue(u32, i32), // Stat ID must reach this value
    StatExists(u32), // Stat ID must exist
    AnyStat, // Any stat increase
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CubeConfig {
    pub enabled: bool,
    pub and_conditions: Vec<CubeCondition>, // All must be satisfied
    pub or_conditions: Vec<CubeCondition>, // At least one must be satisfied
    pub max_rolls: u32, // Maximum rolls to attempt
    pub stop_on_degrade: bool, // Stop if stats degrade
    pub equipment_ids: Vec<u32>, // Equipment to cube
}

impl Default for CubeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            and_conditions: vec![],
            or_conditions: vec![],
            max_rolls: 10,
            stop_on_degrade: false,
            equipment_ids: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentStats {
    pub stats: Vec<(u32, i32)>, // (stat_id, value)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CubeStats {
    pub total_sessions: u32,
    pub total_rolls: u32,
    pub successful_rolls: u32,
    pub failed_rolls: u32,
}

pub struct AutoCube {
    config: CubeConfig,
    pub enabled: bool,
    stats: CubeStats,
    current_rolls: u32,
    previous_stats: Option<EquipmentStats>,
}

impl AutoCube {
    pub fn new(config: CubeConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: CubeStats::default(),
            current_rolls: 0,
            previous_stats: None,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(CubeConfig::default())
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

    pub fn add_and_condition(&mut self, condition: CubeCondition) {
        self.config.and_conditions.push(condition);
    }

    pub fn add_or_condition(&mut self, condition: CubeCondition) {
        self.config.or_conditions.push(condition);
    }

    pub fn set_max_rolls(&mut self, max: u32) {
        self.config.max_rolls = max;
    }

    /// Start a new cube session
    pub fn start_session(&mut self) {
        self.stats.total_sessions += 1;
        self.current_rolls = 0;
        self.previous_stats = None;
    }

    /// Record a roll result
    pub fn record_roll(&mut self, new_stats: EquipmentStats, success: bool) {
        self.current_rolls += 1;
        self.stats.total_rolls += 1;

        if success {
            self.stats.successful_rolls += 1;
        } else {
            self.stats.failed_rolls += 1;
        }

        self.previous_stats = Some(new_stats);
    }

    /// Check if conditions are satisfied
    pub fn conditions_satisfied(&self, stats: &EquipmentStats) -> bool {
        // Check AND conditions (all must pass)
        for condition in &self.config.and_conditions {
            if !self.check_condition(condition, stats) {
                return false;
            }
        }

        // Check OR conditions (at least one must pass)
        if !self.config.or_conditions.is_empty() {
            let or_satisfied = self.config.or_conditions.iter()
                .any(|c| self.check_condition(c, stats));
            if !or_satisfied {
                return false;
            }
        }

        true
    }

    fn check_condition(&self, condition: &CubeCondition, stats: &EquipmentStats) -> bool {
        match condition {
            CubeCondition::StatIncrease(stat_id) => {
                if let Some(ref prev) = self.previous_stats {
                    let prev_val = prev.stats.iter()
                        .find(|(id, _)| id == stat_id)
                        .map(|(_, v)| *v)
                        .unwrap_or(0);
                    let curr_val = stats.stats.iter()
                        .find(|(id, _)| id == stat_id)
                        .map(|(_, v)| *v)
                        .unwrap_or(0);
                    curr_val > prev_val
                } else {
                    false
                }
            }
            CubeCondition::StatValue(stat_id, target) => {
                stats.stats.iter()
                    .find(|(id, _)| id == stat_id)
                    .map(|(_, v)| *v >= *target)
                    .unwrap_or(false)
            }
            CubeCondition::StatExists(stat_id) => {
                stats.stats.iter().any(|(id, _)| id == stat_id)
            }
            CubeCondition::AnyStat => {
                if let Some(ref prev) = self.previous_stats {
                    stats.stats.len() > prev.stats.len() ||
                    stats.stats.iter().zip(prev.stats.iter()).any(|((_, a), (_, b))| a > b)
                } else {
                    false
                }
            }
        }
    }

    /// Check if we should continue rolling
    pub fn should_continue(&self) -> bool {
        self.current_rolls < self.config.max_rolls
    }

    pub fn get_stats(&self) -> CubeStats {
        self.stats.clone()
    }
}
