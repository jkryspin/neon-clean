//! Auto Inner Ability - Circulate to grade and line goals
//!
//! Feature 14: Automatically circulate inner ability stones to desired grades and lines

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
pub enum AbilityGrade {
    Normal,
    Rare,
    Epic,
    Legendary,
}

impl AbilityGrade {
    pub fn value(&self) -> u8 {
        match self {
            AbilityGrade::Normal => 1,
            AbilityGrade::Rare => 2,
            AbilityGrade::Epic => 3,
            AbilityGrade::Legendary => 4,
        }
    }
}

impl Default for AbilityGrade {
    fn default() -> Self {
        AbilityGrade::Normal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InnerAbilityConfig {
    pub enabled: bool,
    pub min_grade: AbilityGrade, // Minimum grade to keep
    pub desired_stats: Vec<u32>, // Stat IDs we want
    pub max_lines: u8, // Maximum lines to aim for
    pub max_circulates: u32, // Maximum circulate attempts
    pub lock_matching: bool, // Lock stats that match desired
}

impl Default for InnerAbilityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_grade: AbilityGrade::Rare,
            desired_stats: vec![],
            max_lines: 5,
            max_circulates: 20,
            lock_matching: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbilityStat {
    pub stat_id: u32,
    pub value: i32,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InnerAbilityData {
    pub grade: AbilityGrade,
    pub stats: Vec<AbilityStat>,
    pub lines: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InnerAbilityStats {
    pub total_sessions: u32,
    pub total_circulates: u32,
    pub items_completed: u32,
    pub best_grade: AbilityGrade,
}

pub struct AutoInnerAbility {
    config: InnerAbilityConfig,
    pub enabled: bool,
    stats: InnerAbilityStats,
    current_circulates: u32,
    current_ability: Option<InnerAbilityData>,
}

impl AutoInnerAbility {
    pub fn new(config: InnerAbilityConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: InnerAbilityStats::default(),
            current_circulates: 0,
            current_ability: None,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(InnerAbilityConfig::default())
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

    pub fn set_min_grade(&mut self, grade: AbilityGrade) {
        self.config.min_grade = grade;
    }

    pub fn add_desired_stat(&mut self, stat_id: u32) {
        if !self.config.desired_stats.contains(&stat_id) {
            self.config.desired_stats.push(stat_id);
        }
    }

    pub fn start_session(&mut self) {
        self.stats.total_sessions += 1;
        self.current_circulates = 0;
        self.current_ability = None;
    }

    pub fn record_circulate(&mut self, new_ability: InnerAbilityData) {
        self.current_circulates += 1;
        self.stats.total_circulates += 1;

        // Update best grade
        if new_ability.grade.value() > self.stats.best_grade.value() {
            self.stats.best_grade = new_ability.grade;
        }

        if self.should_keep(&new_ability) {
            self.current_ability = Some(new_ability);
        }
    }

    pub fn should_keep(&self, ability: &InnerAbilityData) -> bool {
        // Check grade
        if ability.grade.value() < self.config.min_grade.value() {
            return false;
        }

        // Check lines
        if ability.lines > self.config.max_lines {
            return false;
        }

        // If we have a current ability, compare
        if let Some(ref current) = self.current_ability {
            // Keep if better grade
            if ability.grade.value() > current.grade.value() {
                return true;
            }
            // Keep if same grade but more desired stats
            let current_desired = current.stats.iter()
                .filter(|s| self.config.desired_stats.contains(&s.stat_id))
                .count();
            let new_desired = ability.stats.iter()
                .filter(|s| self.config.desired_stats.contains(&s.stat_id))
                .count();
            if new_desired > current_desired {
                return true;
            }
            return false;
        }

        true
    }

    pub fn should_continue(&self) -> bool {
        self.current_circulates < self.config.max_circulates
    }

    pub fn get_stats(&self) -> InnerAbilityStats {
        self.stats.clone()
    }

    pub fn current_ability(&self) -> Option<&InnerAbilityData> {
        self.current_ability.as_ref()
    }
}
