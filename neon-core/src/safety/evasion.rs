//! Player Evasion - React to strangers
//!
//! Feature 42: Change channel, disable, logout, or terminate when players appear

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Copy)]
pub enum EvasionAction {
    None,           // Do nothing
    Alert,          // Just alert the user
    Disable,        // Disable bot features
    ChangeChannel,  // Change to a different channel
    Logout,         // Logout from game
    Terminate,      // Terminate game process
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerEvasionConfig {
    pub enabled: bool,
    pub trusted_igns: HashSet<String>,
    pub scan_distance: f64,
    pub alert_distance: f64,
    pub action: EvasionAction,
    pub check_interval_ms: u64,
}

impl Default for PlayerEvasionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trusted_igns: HashSet::new(),
            scan_distance: 500.0,
            alert_distance: 200.0,
            action: EvasionAction::Disable,
            check_interval_ms: 1000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearbyPlayer {
    pub ign: String,
    pub distance: f64,
    pub is_trusted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayerEvasionStats {
    pub players_detected: u32,
    pub evasions_triggered: u32,
    pub channels_changed: u32,
    pub logouts: u32,
}

pub struct PlayerEvasion {
    config: PlayerEvasionConfig,
    pub enabled: bool,
    stats: PlayerEvasionStats,
    pub is_evasive: bool, // Currently in evasion mode
    nearby_players: Vec<NearbyPlayer>,
}

impl PlayerEvasion {
    pub fn new(config: PlayerEvasionConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: PlayerEvasionStats::default(),
            is_evasive: false,
            nearby_players: vec![],
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(PlayerEvasionConfig::default())
    }

    pub fn enable(&mut self) {
        self.enabled = true;
        self.config.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.config.enabled = false;
        self.is_evasive = false;
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        self.config.enabled = self.enabled;
    }

    pub fn add_trusted_ign(&mut self, ign: &str) {
        self.config.trusted_igns.insert(ign.to_lowercase());
    }

    pub fn remove_trusted_ign(&mut self, ign: &str) {
        self.config.trusted_igns.remove(&ign.to_lowercase());
    }

    pub fn set_action(&mut self, action: EvasionAction) {
        self.config.action = action;
    }

    pub fn set_alert_distance(&mut self, distance: f64) {
        self.config.alert_distance = distance;
    }

    /// Update nearby players (called from scanner)
    pub fn update_players(&mut self, players: Vec<NearbyPlayer>) {
        self.nearby_players = players;
    }

    /// Check if evasion should be triggered
    pub fn check_evasion(&mut self) -> bool {
        if !self.enabled {
            self.is_evasive = false;
            return false;
        }

        // Find untrusted players within alert distance
        let threat = self.nearby_players.iter()
            .find(|p| !p.is_trusted && p.distance < self.config.alert_distance);

        if threat.is_some() {
            self.stats.players_detected += 1;
            if !self.is_evasive {
                self.stats.evasions_triggered += 1;
                self.is_evasive = true;
            }
            true
        } else {
            self.is_evasive = false;
            false
        }
    }

    /// Get the action to take
    pub fn get_action(&self) -> EvasionAction {
        if self.is_evasive {
            self.config.action
        } else {
            EvasionAction::None
        }
    }

    /// Record a channel change
    pub fn record_channel_change(&mut self) {
        self.stats.channels_changed += 1;
    }

    /// Record a logout
    pub fn record_logout(&mut self) {
        self.stats.logouts += 1;
    }

    pub fn get_stats(&self) -> PlayerEvasionStats {
        self.stats.clone()
    }

    pub fn nearest_player(&self) -> Option<&NearbyPlayer> {
        self.nearby_players.iter()
            .filter(|p| !p.is_trusted)
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }
}
