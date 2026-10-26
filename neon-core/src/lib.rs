//! Neon Core
//!
//! Core bot functionality: combat, movement, inventory, utility, accounts, safety, packet handling.

pub mod anti_detect;
pub mod combat;
pub mod movement;
pub mod inventory;
pub mod utility;
pub mod accounts;
pub mod safety;
pub mod packet;
pub mod progression;

use serde::{Deserialize, Serialize};

/// Bot configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub anti_detect: AntiDetectConfig,
    pub combat: CombatConfig,
    pub movement: MovementConfig,
    pub inventory: InventoryConfig,
    pub utility: UtilityConfig,
    pub accounts: AccountsConfig,
    pub safety: SafetyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiDetectConfig {
    pub timing_jitter: f64,
    pub min_idle_seconds: u64,
    pub max_idle_seconds: u64,
    pub idle_frequency: u32,
    pub mistake_chance: f64,
    pub max_session_hours: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatConfig {
    pub kami_enabled: bool,
    pub skill_injection_enabled: bool,
    pub god_mode: bool,
    pub full_map_attack: bool,
    pub auto_attack: bool,
    pub no_delay: bool,
    pub mob_disarm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovementConfig {
    pub rusher_enabled: bool,
    pub infinite_flash_jump: bool,
    pub no_air_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryConfig {
    pub auto_sell: bool,
    pub auto_drop: bool,
    pub pet_loot: bool,
    pub filter_whitelist: Vec<u32>,
    pub filter_blacklist: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityConfig {
    pub lie_detector: bool,
    pub auto_pot: bool,
    pub auto_buff: bool,
    pub auto_npc: bool,
    pub scheduler_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsConfig {
    pub auto_login: bool,
    pub auto_relaunch: bool,
    pub offline_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    pub player_evasion: bool,
    pub panic_key: Option<String>,
    pub restore_key: Option<String>,
    pub ign_whitelist: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            anti_detect: AntiDetectConfig {
                timing_jitter: 0.25,
                min_idle_seconds: 30,
                max_idle_seconds: 180,
                idle_frequency: 50,
                mistake_chance: 0.05,
                max_session_hours: 5,
            },
            combat: CombatConfig {
                kami_enabled: false,
                skill_injection_enabled: false,
                god_mode: false,
                full_map_attack: false,
                auto_attack: false,
                no_delay: false,
                mob_disarm: false,
            },
            movement: MovementConfig {
                rusher_enabled: false,
                infinite_flash_jump: false,
                no_air_check: false,
            },
            inventory: InventoryConfig {
                auto_sell: false,
                auto_drop: false,
                pet_loot: false,
                filter_whitelist: vec![],
                filter_blacklist: vec![],
            },
            utility: UtilityConfig {
                lie_detector: false,
                auto_pot: false,
                auto_buff: false,
                auto_npc: false,
                scheduler_enabled: false,
            },
            accounts: AccountsConfig {
                auto_login: false,
                auto_relaunch: false,
                offline_mode: false,
            },
            safety: SafetyConfig {
                player_evasion: false,
                panic_key: None,
                restore_key: None,
                ign_whitelist: vec![],
            },
        }
    }
}
