//! Rotation Builder - Generate skill rotations from map spawns
//!
//! Feature 3: Record and replay skill sequences, auto-generate from mob spawns

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationStep {
    pub skill_id: u16,
    pub delay_ms: u64,
    pub target_type: TargetType,
    pub condition: Option<String>, // Optional condition (e.g., "boss_alive")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TargetType {
    Cursor,
    NearestMob,
    SpecificMob(u32),
    Player,
}

impl Default for TargetType {
    fn default() -> Self {
        TargetType::Cursor
    }
}

impl RotationStep {
    pub fn new(skill_id: u16, delay_ms: u64) -> Self {
        Self {
            skill_id,
            delay_ms,
            target_type: TargetType::default(),
            condition: None,
        }
    }

    pub fn with_target(mut self, target: TargetType) -> Self {
        self.target_type = target;
        self
    }

    pub fn with_condition(mut self, condition: String) -> Self {
        self.condition = Some(condition);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rotation {
    pub name: String,
    pub steps: Vec<RotationStep>,
    pub loop_rotation: bool,
    pub total_duration_ms: u64,
}

impl Rotation {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            steps: vec![],
            loop_rotation: true,
            total_duration_ms: 0,
        }
    }

    pub fn add_step(&mut self, step: RotationStep) {
        self.steps.push(step);
    }

    pub fn calculate_duration(&mut self) {
        self.total_duration_ms = self.steps.iter().map(|s| s.delay_ms).sum();
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationBuilderConfig {
    pub enabled: bool,
    pub recording: bool,
    pub default_delay_ms: u64,
    pub auto_generate: bool, // Generate from mob spawns
}

impl Default for RotationBuilderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            recording: false,
            default_delay_ms: 100,
            auto_generate: false,
        }
    }
}

pub struct RotationBuilder {
    config: RotationBuilderConfig,
    rotations: Vec<Rotation>,
    current_rotation: Option<String>,
    recording_steps: VecDeque<RotationStep>,
}

impl RotationBuilder {
    pub fn new(config: RotationBuilderConfig) -> Self {
        Self {
            config,
            rotations: vec![],
            current_rotation: None,
            recording_steps: VecDeque::new(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(RotationBuilderConfig::default())
    }

    pub fn enable(&mut self) {
        self.config.enabled = true;
    }

    pub fn disable(&mut self) {
        self.config.enabled = false;
    }

    /// Start recording a new rotation
    pub fn start_recording(&mut self, name: &str) {
        self.config.recording = true;
        self.recording_steps.clear();
        self.current_rotation = Some(name.to_string());
    }

    /// Stop recording and save the rotation
    pub fn stop_recording(&mut self) -> Option<Rotation> {
        if !self.config.recording {
            return None;
        }

        let name = self.current_rotation.take().unwrap_or_else(|| "unnamed".to_string());
        let mut rotation = Rotation::new(&name);

        while let Some(step) = self.recording_steps.pop_front() {
            rotation.add_step(step);
        }

        rotation.calculate_duration();
        self.rotations.push(rotation.clone());
        Some(rotation)
    }

    /// Record a skill cast
    pub fn record_skill(&mut self, skill_id: u16, delay_ms: u64) {
        if !self.config.recording {
            return;
        }

        let step = RotationStep::new(skill_id, delay_ms.max(self.config.default_delay_ms));
        self.recording_steps.push_back(step);
    }

    /// Add a step to current recording
    pub fn add_step(&mut self, step: RotationStep) {
        if self.config.recording {
            self.recording_steps.push_back(step);
        }
    }

    /// Get current rotation
    pub fn get_current_rotation(&self) -> Option<&Rotation> {
        if let Some(ref name) = self.current_rotation {
            self.rotations.iter().find(|r| r.name == *name)
        } else {
            self.rotations.last()
        }
    }

    /// Set active rotation by name
    pub fn set_rotation(&mut self, name: &str) -> bool {
        if let Some(idx) = self.rotations.iter().position(|r| r.name == name) {
            self.current_rotation = Some(name.to_string());
            true
        } else {
            false
        }
    }

    /// List all rotation names
    pub fn list_rotations(&self) -> Vec<String> {
        self.rotations.iter().map(|r| r.name.clone()).collect()
    }

    /// Generate a rotation based on mob spawns (simplified)
    pub fn generate_from_spawns(&mut self, mob_ids: &[u32], skill_ids: &[u16]) -> Rotation {
        let mut rotation = Rotation::new("auto_generated");

        for skill_id in skill_ids {
            let step = RotationStep::new(*skill_id, 150)
                .with_target(TargetType::NearestMob);
            rotation.add_step(step);
        }

        rotation.calculate_duration();
        rotation
    }

    /// Clear all rotations
    pub fn clear(&mut self) {
        self.rotations.clear();
        self.recording_steps.clear();
        self.current_rotation = None;
    }
}
