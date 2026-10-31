//! Utility module: lie detector, scheduler, runes, potions, buffs, NPC

pub mod buffs;
pub mod lie_detector;
pub mod npc;
pub mod pet_care;
pub mod potions;
pub mod runes;
pub mod scheduler;

pub use lie_detector::{LieDetector, LieDetectorAnswer};
pub use potions::AutoPot;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityState {
    pub lie_detector_active: bool,
    pub scheduler: SchedulerState,
    pub potions: PotionState,
    pub buffs: BuffState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerState {
    pub active: bool,
    pub current_step: Option<usize>,
    pub steps: Vec<SchedulerStep>,
    pub next_reset: Option<u64>, // unix timestamp
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerStep {
    pub action: SchedulerAction,
    pub condition: Option<SchedulerCondition>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchedulerAction {
    Grind,
    Rush(u32), // map_id
    Sell,
    Logout,
    RunScript(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchedulerCondition {
    Time(u32, u32), // hour, minute
    Exp(u64),
    Meso(u64),
    Hour(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PotionState {
    pub auto_hp: bool,
    pub auto_mp: bool,
    pub hp_threshold: u8,
    pub mp_threshold: u8,
    pub current_hp: u32,
    pub current_mp: u32,
    pub max_hp: u32,
    pub max_mp: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuffState {
    pub auto_buff: bool,
    pub active_buffs: Vec<Buff>,
    pub buff_cooldowns: std::collections::HashMap<u16, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Buff {
    pub skill_id: u16,
    pub duration_ms: u64,
    pub expires_at: u64,
}

impl Default for UtilityState {
    fn default() -> Self {
        Self {
            lie_detector_active: false,
            scheduler: SchedulerState {
                active: false,
                current_step: None,
                steps: vec![],
                next_reset: None,
            },
            potions: PotionState {
                auto_hp: false,
                auto_mp: false,
                hp_threshold: 50,
                mp_threshold: 50,
                current_hp: 0,
                current_mp: 0,
                max_hp: 1,
                max_mp: 1,
            },
            buffs: BuffState {
                auto_buff: false,
                active_buffs: vec![],
                buff_cooldowns: std::collections::HashMap::new(),
            },
        }
    }
}
