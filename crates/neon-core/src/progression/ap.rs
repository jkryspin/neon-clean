//! Auto AP - Ability points assign themselves
//!
//! Feature 15: Automatically assign ability points based on class and preferences

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
pub enum StatType {
    Strength,
    Dexterity,
    Intelligence,
    Luck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApDistribution {
    pub stat: StatType,
    pub priority: u8,           // Higher = more points
    pub max_value: Option<u32>, // Stop at this value
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoApConfig {
    pub enabled: bool,
    pub distribution: Vec<ApDistribution>, // Priority order
    pub auto_collect: bool,                // Auto collect AP when available
    pub notify_when_full: bool,            // Notify when all stats maxed
}

impl Default for AutoApConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            distribution: vec![
                ApDistribution {
                    stat: StatType::Strength,
                    priority: 1,
                    max_value: None,
                },
                ApDistribution {
                    stat: StatType::Dexterity,
                    priority: 2,
                    max_value: None,
                },
                ApDistribution {
                    stat: StatType::Intelligence,
                    priority: 3,
                    max_value: None,
                },
                ApDistribution {
                    stat: StatType::Luck,
                    priority: 4,
                    max_value: None,
                },
            ],
            auto_collect: true,
            notify_when_full: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentStats {
    pub strength: u32,
    pub dexterity: u32,
    pub intelligence: u32,
    pub luck: u32,
    pub available_ap: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoApStats {
    pub total_points_assigned: u32,
    pub sessions_completed: u32,
}

pub struct AutoAp {
    config: AutoApConfig,
    pub enabled: bool,
    stats: AutoApStats,
}

impl AutoAp {
    pub fn new(config: AutoApConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: AutoApStats::default(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(AutoApConfig::default())
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

    pub fn set_distribution(&mut self, distribution: Vec<ApDistribution>) {
        self.config.distribution = distribution;
    }

    pub fn add_stat_priority(&mut self, stat: StatType, priority: u8) {
        // Remove existing entry for this stat
        self.config.distribution.retain(|d| d.stat != stat);
        // Add with new priority
        self.config.distribution.push(ApDistribution {
            stat,
            priority,
            max_value: None,
        });
        // Sort by priority
        self.config.distribution.sort_by_key(|d| d.priority);
    }

    /// Get the next stat to assign points to
    pub fn next_stat(&self, current: &CurrentStats) -> Option<StatType> {
        if current.available_ap == 0 {
            return None;
        }

        for dist in &self.config.distribution {
            let current_value = match dist.stat {
                StatType::Strength => current.strength,
                StatType::Dexterity => current.dexterity,
                StatType::Intelligence => current.intelligence,
                StatType::Luck => current.luck,
            };

            // Check if maxed
            if let Some(max) = dist.max_value {
                if current_value >= max {
                    continue;
                }
            }

            // Check if at stat cap (usually 99999)
            if current_value >= 99999 {
                continue;
            }

            return Some(dist.stat);
        }

        None
    }

    /// Record a point assignment
    pub fn record_assignment(&mut self, stat: StatType, points: u32) {
        self.stats.total_points_assigned += points;
    }

    /// Check if all stats are maxed
    pub fn all_maxed(&self, current: &CurrentStats) -> bool {
        for dist in &self.config.distribution {
            let current_value = match dist.stat {
                StatType::Strength => current.strength,
                StatType::Dexterity => current.dexterity,
                StatType::Intelligence => current.intelligence,
                StatType::Luck => current.luck,
            };

            // Check if not at max
            if let Some(max) = dist.max_value {
                if current_value < max {
                    return false;
                }
            }
            if current_value < 99999 {
                return false;
            }
        }

        true
    }

    pub fn get_stats(&self) -> AutoApStats {
        self.stats.clone()
    }
}
