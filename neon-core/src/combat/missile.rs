//! Missile Storm - One skill, fired as many missiles
//!
//! Feature 6: Multiply missile count for skills like Crossbow Shot

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissileStormConfig {
    pub enabled: bool,
    pub skill_id: u16, // Skill to apply missile storm to
    pub missile_count: u8, // How many missiles to fire
    pub spread_angle: f64, // Degrees of spread
}

impl Default for MissileStormConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            skill_id: 0,
            missile_count: 5,
            spread_angle: 30.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Missile {
    pub angle: f64, // Degrees from target direction
    pub target_x: i16,
    pub target_y: i16,
}

pub struct MissileStorm {
    config: MissileStormConfig,
    pub active: bool,
    missiles_fired: u32,
}

impl MissileStorm {
    pub fn new(config: MissileStormConfig) -> Self {
        Self {
            active: config.enabled,
            config,
            missiles_fired: 0,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(MissileStormConfig::default())
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

    pub fn set_missile_count(&mut self, count: u8) {
        self.config.missile_count = count.min(20); // Cap at 20
    }

    pub fn set_spread(&mut self, angle: f64) {
        self.config.spread_angle = angle.min(90.0); // Cap at 90 degrees
    }

    /// Generate missile targets for a given target position
    pub fn generate_missiles(&mut self, target_x: i16, target_y: i16, player_x: i16, player_y: i16) -> Vec<Missile> {
        let mut missiles = vec![];
        let count = self.config.missile_count as usize;
        let spread = self.config.spread_angle;

        // Calculate base direction to target
        let dx = target_x as f64 - player_x as f64;
        let dy = target_y as f64 - player_y as f64;
        let base_angle = dy.atan2(dx) * 180.0 / std::f64::consts::PI;

        // Generate missiles with spread
        let angle_step = if count > 1 {
            spread / (count - 1) as f64
        } else {
            0.0
        };

        for i in 0..count {
            let angle_offset = (i as f64 - (count as f64 - 1.0) / 2.0) * angle_step;
            let missile_angle = base_angle + angle_offset;

            // Calculate missile target (extend beyond original target)
            let range = 400.0; // Missile range
            let mx = (player_x as f64 + missile_angle.to_radians().cos() * range) as i16;
            let my = (player_y as f64 + missile_angle.to_radians().sin() * range) as i16;

            missiles.push(Missile {
                angle: angle_offset,
                target_x: mx,
                target_y: my,
            });
        }

        self.missiles_fired += count as u32;
        missiles
    }

    pub fn get_stats(&self) -> MissileStormStats {
        MissileStormStats {
            active: self.active,
            skill_id: self.config.skill_id,
            missile_count: self.config.missile_count,
            spread_angle: self.config.spread_angle,
            total_missiles_fired: self.missiles_fired,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissileStormStats {
    pub active: bool,
    pub skill_id: u16,
    pub missile_count: u8,
    pub spread_angle: f64,
    pub total_missiles_fired: u32,
}
