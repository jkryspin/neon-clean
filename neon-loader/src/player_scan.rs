//! Player scanning for Player Evasion

use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Player {
    pub name: String,
    pub x: i16,
    pub y: i16,
    pub distance: f64,
    pub is_trusted: bool,
}

pub struct PlayerScanner {
    trusted_igns: HashSet<String>,
    scan_distance: f64, // Max distance to scan for players
    min_distance_alert: f64, // Distance to trigger evasion
}

impl PlayerScanner {
    pub fn new() -> Self {
        Self {
            trusted_igns: HashSet::new(),
            scan_distance: 500.0,
            min_distance_alert: 200.0,
        }
    }

    pub fn add_trusted_ign(&mut self, ign: &str) {
        self.trusted_igns.insert(ign.to_lowercase());
    }

    pub fn remove_trusted_ign(&mut self, ign: &str) {
        self.trusted_igns.remove(&ign.to_lowercase());
    }

    pub fn set_scan_distance(&mut self, distance: f64) {
        self.scan_distance = distance;
    }

    pub fn set_alert_distance(&mut self, distance: f64) {
        self.min_distance_alert = distance;
    }

    /// Scan for nearby players (placeholder - would read from memory)
    pub fn scan_players(&self, player_pos: (i16, i16)) -> Vec<Player> {
        // Placeholder: return empty for now
        // In full implementation, this would scan player layers in memory
        // similar to mob scanning but with player-specific layer structures
        vec![]
    }

    /// Check if any players are nearby (within alert distance)
    pub fn players_nearby(&self, player_pos: (i16, i16)) -> Vec<Player> {
        let all_players = self.scan_players(player_pos);
        all_players.into_iter()
            .filter(|p| p.distance < self.min_distance_alert && !p.is_trusted)
            .collect()
    }

    /// Calculate distance between two positions
    pub fn distance(pos1: (i16, i16), pos2: (i16, i16)) -> f64 {
        let dx = pos1.0 as i32 - pos2.0 as i32;
        let dy = pos1.1 as i32 - pos2.1 as i32;
        ((dx * dx + dy * dy) as f64).sqrt()
    }

    /// Check if an IGN is trusted
    pub fn is_trusted(&self, ign: &str) -> bool {
        self.trusted_igns.contains(&ign.to_lowercase())
    }

    pub fn trusted_count(&self) -> usize {
        self.trusted_igns.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distance() {
        let dist = PlayerScanner::distance((0, 0), (3, 4));
        assert!((dist - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_trusted_ign() {
        let mut scanner = PlayerScanner::new();
        scanner.add_trusted_ign("Kryspin");
        assert!(scanner.is_trusted("kryspin"));
        assert!(scanner.is_trusted("KRYSPIN"));
    }
}
