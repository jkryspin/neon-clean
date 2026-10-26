//! Auto Pot - Automatic potion consumption
//!
//! Feature 28: Consume HP/MP potions when below thresholds

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PotionConfig {
    pub hp_threshold_percent: u8, // Consume HP pot when below this %
    pub mp_threshold_percent: u8, // Consume MP pot when below this %
    pub hp_key: String, // Key binding for HP potion (default: F2)
    pub mp_key: String, // Key binding for MP potion (default: F3)
    pub max_hp_potions_per_minute: u8, // Rate limit
    pub max_mp_potions_per_minute: u8, // Rate limit
    pub only_when_attacking: bool, // Only pot when in combat
}

impl Default for PotionConfig {
    fn default() -> Self {
        Self {
            hp_threshold_percent: 50,
            mp_threshold_percent: 30,
            hp_key: "f2".to_string(),
            mp_key: "f3".to_string(),
            max_hp_potions_per_minute: 10,
            max_mp_potions_per_minute: 10,
            only_when_attacking: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PotionStats {
    pub hp_potions_used: u32,
    pub mp_potions_used: u32,
    pub last_hp_pot_ms: u128,
    pub last_mp_pot_ms: u128,
}

pub struct AutoPot {
    config: PotionConfig,
    pub enabled: bool,
    stats: PotionStats,
    hp_potions_this_minute: u8,
    mp_potions_this_minute: u8,
    minute_start_ms: u128,
}

impl AutoPot {
    pub fn new(config: PotionConfig) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_millis();
        Self {
            config,
            enabled: false,
            stats: PotionStats::default(),
            hp_potions_this_minute: 0,
            mp_potions_this_minute: 0,
            minute_start_ms: now,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(PotionConfig::default())
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }

    /// Check if HP potion should be consumed
    pub fn should_use_hp_pot(&mut self, current_hp: u32, max_hp: u32, in_combat: bool) -> bool {
        if !self.enabled {
            return false;
        }

        // Check if in combat (if configured)
        if self.config.only_when_attacking && !in_combat {
            return false;
        }

        // Check threshold
        let hp_percent = if max_hp > 0 {
            (current_hp as f64 / max_hp as f64) * 100.0
        } else {
            100.0
        };

        if hp_percent >= self.config.hp_threshold_percent as f64 {
            return false;
        }

        // Check rate limit inline
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_millis();
        if now - self.minute_start_ms >= 60000 {
            self.hp_potions_this_minute = 0;
            self.minute_start_ms = now;
        }
        self.hp_potions_this_minute < self.config.max_hp_potions_per_minute
    }

    /// Check if MP potion should be consumed
    pub fn should_use_mp_pot(&mut self, current_mp: u32, max_mp: u32, in_combat: bool) -> bool {
        if !self.enabled {
            return false;
        }

        // Check if in combat (if configured)
        if self.config.only_when_attacking && !in_combat {
            return false;
        }

        // Check threshold
        let mp_percent = if max_mp > 0 {
            (current_mp as f64 / max_mp as f64) * 100.0
        } else {
            100.0
        };

        if mp_percent >= self.config.mp_threshold_percent as f64 {
            return false;
        }

        // Check rate limit inline
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_millis();
        if now - self.minute_start_ms >= 60000 {
            self.mp_potions_this_minute = 0;
            self.minute_start_ms = now;
        }
        self.mp_potions_this_minute < self.config.max_mp_potions_per_minute
    }

    /// Record HP potion usage
    pub fn record_hp_pot(&mut self) {
        self.stats.hp_potions_used += 1;
        self.hp_potions_this_minute += 1;
        self.stats.last_hp_pot_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_millis();
    }

    /// Record MP potion usage
    pub fn record_mp_pot(&mut self) {
        self.stats.mp_potions_used += 1;
        self.mp_potions_this_minute += 1;
        self.stats.last_mp_pot_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_millis();
    }

    /// Get the key to press for HP potion
    pub fn hp_potion_key(&self) -> &str {
        &self.config.hp_key
    }

    /// Get the key to press for MP potion
    pub fn mp_potion_key(&self) -> &str {
        &self.config.mp_key
    }

    pub fn get_stats(&self) -> PotionStats {
        self.stats.clone()
    }
}
