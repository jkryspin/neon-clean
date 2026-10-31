//! Movement module: rusher, pathfinding, teleport, unlocks

pub mod rusher;
pub mod spawn;
pub mod unlocks;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovementState {
    pub current_map: u32,
    pub position: Position,
    pub camera: Position,
    pub is_moving: bool,
    pub teleport_enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Waypoint {
    pub map_id: u32,
    pub x: i32,
    pub y: i32,
    pub portal: Option<u32>, // portal ID to use
}

impl Default for MovementState {
    fn default() -> Self {
        Self {
            current_map: 0,
            position: Position { x: 0, y: 0 },
            camera: Position { x: 0, y: 0 },
            is_moving: false,
            teleport_enabled: false,
        }
    }
}
