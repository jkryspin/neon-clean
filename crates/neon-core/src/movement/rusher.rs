//! Rusher - Pathfind map to map, routes by level
//!
//! Feature 27: Navigate between maps using waypoint-based pathfinding

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Waypoint {
    pub map_id: u32,
    pub x: i16,
    pub y: i16,
    pub portal: Option<String>, // Portal name or index
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub name: String,
    pub waypoints: Vec<Waypoint>,
    pub min_level: u16,
    pub max_level: u16,
}

impl Route {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            waypoints: vec![],
            min_level: 1,
            max_level: 200,
        }
    }

    pub fn add_waypoint(&mut self, map_id: u32, x: i16, y: i16) {
        self.waypoints.push(Waypoint {
            map_id,
            x,
            y,
            portal: None,
        });
    }

    pub fn add_portal_waypoint(&mut self, map_id: u32, x: i16, y: i16, portal: &str) {
        self.waypoints.push(Waypoint {
            map_id,
            x,
            y,
            portal: Some(portal.to_string()),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.waypoints.is_empty()
    }

    pub fn current_waypoint(
        &self,
        current_map: u32,
        current_x: i16,
        current_y: i16,
    ) -> Option<usize> {
        for (i, wp) in self.waypoints.iter().enumerate() {
            if wp.map_id == current_map {
                // Check if we're close to this waypoint
                let dx = wp.x as i32 - current_x as i32;
                let dy = wp.y as i32 - current_y as i32;
                let dist = ((dx * dx + dy * dy) as f64).sqrt();
                if dist < 50.0 {
                    return Some(i);
                }
            }
        }
        None
    }

    pub fn next_waypoint(&self, current_index: usize) -> Option<&Waypoint> {
        self.waypoints.get(current_index.saturating_add(1))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RusherConfig {
    pub enabled: bool,
    pub routes: HashMap<String, Route>,
    pub current_route: Option<String>,
    pub auto_portal: bool, // Auto-use portals
    pub walk_speed: f64,   // Pixels per second
}

impl Default for RusherConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            routes: HashMap::new(),
            current_route: None,
            auto_portal: true,
            walk_speed: 200.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RusherState {
    pub current_route: Option<String>,
    pub current_waypoint: usize,
    pub total_distance: f64,
    pub time_spent: u64, // milliseconds
}

pub struct Rusher {
    config: RusherConfig,
    pub enabled: bool,
    state: RusherState,
}

impl Rusher {
    pub fn new(config: RusherConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            state: RusherState::default(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(RusherConfig::default())
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

    /// Register a route
    pub fn register_route(&mut self, route: Route) {
        self.config.routes.insert(route.name.clone(), route);
    }

    /// Start following a route
    pub fn start_route(&mut self, name: &str) -> bool {
        if self.config.routes.contains_key(name) {
            self.state.current_route = Some(name.to_string());
            self.state.current_waypoint = 0;
            self.config.current_route = Some(name.to_string());
            true
        } else {
            false
        }
    }

    /// Stop current route
    pub fn stop_route(&mut self) {
        self.state.current_route = None;
        self.state.current_waypoint = 0;
        self.config.current_route = None;
    }

    /// Get current route
    pub fn current_route(&self) -> Option<&Route> {
        if let Some(ref name) = self.state.current_route {
            self.config.routes.get(name)
        } else {
            None
        }
    }

    /// Get next waypoint to move to
    pub fn next_waypoint(&self) -> Option<&Waypoint> {
        let route = self.current_route()?;
        route.waypoints.get(self.state.current_waypoint)
    }

    /// Check if we've reached the current waypoint
    pub fn reached_waypoint(&self, x: i16, y: i16) -> bool {
        let wp = match self.next_waypoint() {
            Some(wp) => wp,
            None => return false,
        };
        let dx = wp.x as i32 - x as i32;
        let dy = wp.y as i32 - y as i32;
        let dist = ((dx * dx + dy * dy) as f64).sqrt();
        dist < 30.0 // Within 30 pixels
    }

    /// Advance to next waypoint
    pub fn advance_waypoint(&mut self) {
        self.state.current_waypoint += 1;
    }

    /// Check if route is complete
    pub fn route_complete(&self) -> bool {
        let route = match self.current_route() {
            Some(r) => r,
            None => return true, // No route = complete
        };
        self.state.current_waypoint >= route.waypoints.len()
    }

    /// Calculate distance to next waypoint
    pub fn distance_to_next(&self, x: i16, y: i16) -> Option<f64> {
        let wp = self.next_waypoint()?;
        let dx = wp.x as i32 - x as i32;
        let dy = wp.y as i32 - y as i32;
        Some(((dx * dx + dy * dy) as f64).sqrt())
    }

    /// List all route names
    pub fn list_routes(&self) -> Vec<String> {
        self.config.routes.keys().cloned().collect()
    }

    pub fn get_stats(&self) -> RusherStats {
        RusherStats {
            enabled: self.enabled,
            current_route: self.state.current_route.clone(),
            current_waypoint: self.state.current_waypoint,
            total_routes: self.config.routes.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RusherStats {
    pub enabled: bool,
    pub current_route: Option<String>,
    pub current_waypoint: usize,
    pub total_routes: usize,
}
