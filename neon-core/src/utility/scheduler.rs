//! Scheduler - Steps, reset-day tasks, safety rules
//!
//! Feature 25: Schedule tasks for specific times and reset days

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScheduledTask {
    MoveToMap(u32),
    UsePortal(String),
    UseItem(u32),
    PressKey(String),
    WaitForMap(u32),
    WaitForTime(u64), // Unix timestamp
    Logout,
    Relaunch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleStep {
    pub task: ScheduledTask,
    pub time: Option<u64>, // Unix timestamp (None = immediate)
    pub repeat: bool,
    pub condition: Option<String>, // Optional condition
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyRule {
    pub condition: String, // e.g., "hp < 20%"
    pub action: ScheduledTask,
    pub priority: u8, // Higher = checked first
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    pub enabled: bool,
    pub steps: Vec<ScheduleStep>,
    pub safety_rules: Vec<SafetyRule>,
    pub reset_day_tasks: Vec<ScheduleStep>, // Tasks for monthly reset
    pub current_step: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            steps: vec![],
            safety_rules: vec![],
            reset_day_tasks: vec![],
            current_step: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SchedulerStats {
    pub steps_completed: u32,
    pub safety_triggers: u32,
    pub reset_days_completed: u32,
}

pub struct Scheduler {
    config: SchedulerConfig,
    pub enabled: bool,
    stats: SchedulerStats,
    last_reset_day: u64, // Unix timestamp
}

impl Scheduler {
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: SchedulerStats::default(),
            last_reset_day: 0,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(SchedulerConfig::default())
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

    pub fn add_step(&mut self, step: ScheduleStep) {
        self.config.steps.push(step);
    }

    pub fn add_safety_rule(&mut self, rule: SafetyRule) {
        self.config.safety_rules.push(rule);
    }

    pub fn add_reset_day_task(&mut self, task: ScheduleStep) {
        self.config.reset_day_tasks.push(task);
    }

    /// Get the next step to execute
    pub fn next_step(&self) -> Option<&ScheduleStep> {
        if self.config.current_step >= self.config.steps.len() {
            return None;
        }
        self.config.steps.get(self.config.current_step)
    }

    /// Advance to next step
    pub fn advance_step(&mut self) {
        self.config.current_step += 1;
        self.stats.steps_completed += 1;
    }

    /// Reset to beginning
    pub fn reset(&mut self) {
        self.config.current_step = 0;
    }

    /// Check if reset day tasks should run
    pub fn should_run_reset_day(&self, current_time: u64) -> bool {
        // Simplified: check if more than 30 days since last reset
        (current_time - self.last_reset_day) > (30 * 24 * 60 * 60)
    }

    /// Mark reset day as completed
    pub fn mark_reset_day(&mut self) {
        self.last_reset_day = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_secs();
        self.stats.reset_days_completed += 1;
    }

    /// Check safety rules (simplified)
    pub fn check_safety(&mut self, hp_percent: f64) -> Option<&ScheduledTask> {
        for rule in &self.config.safety_rules {
            // Simple condition parsing
            if rule.condition.contains("hp <") && hp_percent < 20.0 {
                self.stats.safety_triggers += 1;
                return Some(&rule.action);
            }
        }
        None
    }

    pub fn get_stats(&self) -> SchedulerStats {
        self.stats.clone()
    }

    pub fn steps_remaining(&self) -> usize {
        self.config.steps.len().saturating_sub(self.config.current_step)
    }
}
