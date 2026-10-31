//! Auto Starforce - Star to target with safeguard
//!
//! Feature 11: Automatically starforce equipment to target level

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarforceConfig {
    pub enabled: bool,
    pub target_star: i32,         // Target star level
    pub use_safeguard: bool,      // Use safeguard stones
    pub safeguard_threshold: i32, // Use safeguard below this star level
    pub max_meso_per_roll: u64,   // Maximum meso to spend per roll
    pub stop_on_failure: bool,    // Stop after a failure
    pub equipment_ids: Vec<u32>,  // Equipment IDs to starforce
}

impl Default for StarforceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            target_star: 10,
            use_safeguard: true,
            safeguard_threshold: 7,
            max_meso_per_roll: 1000000,
            stop_on_failure: false,
            equipment_ids: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarforceSession {
    pub equipment_id: u32,
    pub current_star: i32,
    pub target_star: i32,
    pub rolls_attempted: u32,
    pub rolls_successful: u32,
    pub meso_spent: u64,
    pub safeguard_used: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StarforceStats {
    pub total_sessions: u32,
    pub total_rolls: u32,
    pub successful_rolls: u32,
    pub total_meso_spent: u64,
    pub items_completed: u32,
}

pub struct AutoStarforce {
    config: StarforceConfig,
    pub enabled: bool,
    stats: StarforceStats,
    current_session: Option<StarforceSession>,
}

impl AutoStarforce {
    pub fn new(config: StarforceConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: StarforceStats::default(),
            current_session: None,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(StarforceConfig::default())
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

    pub fn set_target_star(&mut self, star: i32) {
        self.config.target_star = star.clamp(-10, 20);
    }

    pub fn set_safeguard_threshold(&mut self, threshold: i32) {
        self.config.safeguard_threshold = threshold.clamp(-10, 20);
    }

    /// Start a new starforce session
    pub fn start_session(&mut self, equipment_id: u32, current_star: i32) -> StarforceSession {
        self.stats.total_sessions += 1;
        let session = StarforceSession {
            equipment_id,
            current_star,
            target_star: self.config.target_star,
            rolls_attempted: 0,
            rolls_successful: 0,
            meso_spent: 0,
            safeguard_used: false,
            complete: false,
        };
        self.current_session = Some(session.clone());
        session
    }

    /// Record a roll attempt
    pub fn record_roll(&mut self, success: bool, meso_cost: u64, used_safeguard: bool) {
        if let Some(ref mut session) = self.current_session {
            session.rolls_attempted += 1;
            session.meso_spent += meso_cost;
            session.safeguard_used = session.safeguard_used || used_safeguard;

            if success {
                session.rolls_successful += 1;
                session.current_star += 1;

                // Check if complete
                if session.current_star >= session.target_star {
                    session.complete = true;
                    self.stats.items_completed += 1;
                }
            } else if self.config.stop_on_failure {
                session.complete = true; // Stop on failure
            }

            self.stats.total_rolls += 1;
            if success {
                self.stats.successful_rolls += 1;
            }
            self.stats.total_meso_spent += meso_cost;
        }
    }

    /// Check if we should use safeguard
    pub fn should_use_safeguard(&self, current_star: i32) -> bool {
        self.config.use_safeguard && current_star < self.config.safeguard_threshold
    }

    /// Check if we should continue starforcing
    pub fn should_continue(&self) -> bool {
        if let Some(ref session) = self.current_session {
            !session.complete && session.current_star < self.config.target_star
        } else {
            false
        }
    }

    pub fn get_stats(&self) -> StarforceStats {
        self.stats.clone()
    }

    pub fn current_session(&self) -> Option<&StarforceSession> {
        self.current_session.as_ref()
    }
}
