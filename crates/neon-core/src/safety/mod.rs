//! Safety module: player evasion, whitelist, panic mode

pub mod evasion;
pub mod panic;
pub mod whitelist;

pub use evasion::PlayerEvasion;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyState {
    pub evasion_active: bool,
    pub panic_mode: bool,
    pub nearby_players: Vec<NearbyPlayer>,
    pub ign_whitelist: Vec<String>,
    pub evasion_action: EvasionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearbyPlayer {
    pub name: String,
    pub distance: u32,
    pub is_whitelisted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvasionAction {
    None,
    ChangeChannel,
    DisableFeatures,
    Logout,
    Terminate,
}

impl Default for SafetyState {
    fn default() -> Self {
        Self {
            evasion_active: false,
            panic_mode: false,
            nearby_players: vec![],
            ign_whitelist: vec![],
            evasion_action: EvasionAction::None,
        }
    }
}
