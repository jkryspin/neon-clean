//! Spawn Points - Land on chosen coordinates
//!
//! Feature 31: Set and use custom spawn points per map

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnPoint {
    pub map_id: u32,
    pub x: i16,
    pub y: i16,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnPointsConfig {
    pub enabled: bool,
    pub points: HashMap<u32, SpawnPoint>, // Map ID -> spawn point
    pub auto_use: bool,                   // Auto use spawn point when entering map
}

impl Default for SpawnPointsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            points: HashMap::new(),
            auto_use: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpawnPointsStats {
    pub points_used: u32,
    pub points_set: u32,
}

pub struct SpawnPoints {
    config: SpawnPointsConfig,
    pub enabled: bool,
    stats: SpawnPointsStats,
}

impl SpawnPoints {
    pub fn new(config: SpawnPointsConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: SpawnPointsStats::default(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(SpawnPointsConfig::default())
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

    /// Set a spawn point for a map
    pub fn set_spawn_point(&mut self, map_id: u32, x: i16, y: i16, name: &str) {
        self.config.points.insert(
            map_id,
            SpawnPoint {
                map_id,
                x,
                y,
                name: name.to_string(),
            },
        );
        self.stats.points_set += 1;
    }

    /// Get spawn point for a map
    pub fn get_spawn_point(&self, map_id: u32) -> Option<&SpawnPoint> {
        self.config.points.get(&map_id)
    }

    /// Remove spawn point for a map
    pub fn remove_spawn_point(&mut self, map_id: u32) {
        self.config.points.remove(&map_id);
    }

    /// Use spawn point (move to coordinates)
    pub fn use_spawn_point(&mut self, map_id: u32) -> Option<(i16, i16)> {
        let coords = self.get_spawn_point(map_id).map(|p| (p.x, p.y))?;
        self.stats.points_used += 1;
        Some(coords)
    }

    /// List all spawn points
    pub fn list_points(&self) -> Vec<&SpawnPoint> {
        self.config.points.values().collect()
    }

    pub fn get_stats(&self) -> SpawnPointsStats {
        self.stats.clone()
    }

    pub fn point_count(&self) -> usize {
        self.config.points.len()
    }
}
