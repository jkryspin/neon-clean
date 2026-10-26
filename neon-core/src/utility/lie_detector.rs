//! Lie Detector - Solve anti-macro checks
//!
//! Feature 24: Automatically answer lie detector questions

use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Copy)]
pub enum LieDetectorAnswer {
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LieDetectorConfig {
    pub enabled: bool,
    pub auto_answer: bool,
    pub disable_during_check: bool, // Disable other features during lie detector
    pub disabled_features: Vec<String>, // Features to disable during check
}

impl Default for LieDetectorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_answer: true,
            disable_during_check: true,
            disabled_features: vec!["kami".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LieDetectorStats {
    pub checks_encountered: u32,
    pub checks_answered: u32,
    pub correct_answers: u32,
}

pub struct LieDetector {
    config: LieDetectorConfig,
    pub enabled: bool,
    stats: LieDetectorStats,
    is_active: bool,
    activation_time: Option<Instant>,
    features_disabled: bool,
}

impl LieDetector {
    pub fn new(config: LieDetectorConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: LieDetectorStats::default(),
            is_active: false,
            activation_time: None,
            features_disabled: false,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(LieDetectorConfig::default())
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

    /// Lie detector appeared
    pub fn activate(&mut self) {
        self.is_active = true;
        self.activation_time = Some(Instant::now());
        self.stats.checks_encountered += 1;

        if self.config.disable_during_check {
            self.features_disabled = true;
        }
    }

    /// Answer the lie detector
    pub fn answer(&mut self, answer: LieDetectorAnswer) -> bool {
        if !self.is_active {
            return false;
        }

        self.stats.checks_answered += 1;
        // Assume correct for now (would need verification)
        self.stats.correct_answers += 1;

        self.is_active = false;
        self.activation_time = None;

        if self.config.disable_during_check {
            self.features_disabled = false;
        }

        true
    }

    /// Get suggested answer (simplified heuristic)
    pub fn get_answer(&self) -> Option<LieDetectorAnswer> {
        if !self.is_active {
            return None;
        }

        // Simple heuristic: alternate answers
        // In reality, would need to parse the question
        if self.stats.checks_encountered % 2 == 0 {
            Some(LieDetectorAnswer::Left)
        } else {
            Some(LieDetectorAnswer::Right)
        }
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn features_disabled(&self) -> bool {
        self.features_disabled
    }

    pub fn get_stats(&self) -> LieDetectorStats {
        self.stats.clone()
    }

    pub fn time_active(&self) -> Option<std::time::Duration> {
        self.activation_time.map(|t| t.elapsed())
    }
}
