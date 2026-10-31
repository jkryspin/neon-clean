//! Auto Flame - Reroll flames, keep upgrades
//!
//! Feature 13: Automatically reroll flame stats, keeping only upgrades

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FlameStat {
    MaxHp,
    MaxMp,
    AllStats,
    Attack,
    Defense,
    BossDamage,
    Custom(u32), // Stat ID
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlameConfig {
    pub enabled: bool,
    pub desired_stats: Vec<FlameStat>, // Stats we want
    pub min_stat_value: i32,           // Minimum value for desired stats
    pub max_rolls: u32,                // Maximum rolls
    pub stop_on_perfect: bool,         // Stop when all desired stats obtained
    pub equipment_ids: Vec<u32>,       // Equipment to flame
}

impl Default for FlameConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            desired_stats: vec![FlameStat::BossDamage],
            min_stat_value: 10,
            max_rolls: 5,
            stop_on_perfect: true,
            equipment_ids: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlameStatsData {
    pub stats: Vec<(FlameStat, i32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoFlameStats {
    pub total_sessions: u32,
    pub total_rolls: u32,
    pub successful_rolls: u32,
    pub items_completed: u32,
}

pub struct AutoFlame {
    config: FlameConfig,
    pub enabled: bool,
    stats: AutoFlameStats,
    current_rolls: u32,
    previous_flame: Option<FlameStatsData>,
}

impl AutoFlame {
    pub fn new(config: FlameConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: AutoFlameStats::default(),
            current_rolls: 0,
            previous_flame: None,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(FlameConfig::default())
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

    pub fn add_desired_stat(&mut self, stat: FlameStat) {
        if !self.config.desired_stats.contains(&stat) {
            self.config.desired_stats.push(stat);
        }
    }

    pub fn start_session(&mut self) {
        self.stats.total_sessions += 1;
        self.current_rolls = 0;
        self.previous_flame = None;
    }

    pub fn record_roll(&mut self, new_flame: FlameStatsData, keep: bool) {
        self.current_rolls += 1;
        self.stats.total_rolls += 1;

        if keep {
            self.stats.successful_rolls += 1;
            self.previous_flame = Some(new_flame.clone());
        }

        // Check if complete
        if self.config.stop_on_perfect && self.all_desired_stats_obtained(&new_flame) {
            self.stats.items_completed += 1;
        }
    }

    fn all_desired_stats_obtained(&self, flame: &FlameStatsData) -> bool {
        self.config.desired_stats.iter().all(|desired| {
            flame
                .stats
                .iter()
                .any(|(stat, value)| stat == desired && *value >= self.config.min_stat_value)
        })
    }

    pub fn should_keep(&self, new_flame: &FlameStatsData) -> bool {
        if let Some(ref prev) = self.previous_flame {
            // Keep if any stat improved
            for (new_stat, new_val) in &new_flame.stats {
                if let Some((_, prev_val)) = prev.stats.iter().find(|(s, _)| s == new_stat) {
                    if new_val > prev_val {
                        return true;
                    }
                }
            }
            false
        } else {
            // First roll, keep if has desired stats
            self.config.desired_stats.iter().any(|desired| {
                new_flame
                    .stats
                    .iter()
                    .any(|(stat, value)| stat == desired && *value >= self.config.min_stat_value)
            })
        }
    }

    pub fn should_continue(&self) -> bool {
        self.current_rolls < self.config.max_rolls && !self.config.stop_on_perfect
            || !self.all_desired_stats_obtained(self.previous_flame.as_ref().unwrap())
    }

    pub fn get_stats(&self) -> AutoFlameStats {
        self.stats.clone()
    }
}
