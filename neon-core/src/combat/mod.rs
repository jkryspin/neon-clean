//! Combat module: skill injection, rotations, god mode, Kami grinding, etc.

pub mod kami;
pub mod skills;
pub mod rotation;
pub mod godmode;
pub mod fullmap;
pub mod missile;
pub mod autoattack;
pub mod nodelay;
pub mod unlocks;
pub mod disarm;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Combat state tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatState {
    pub skill_cooldowns: SkillCooldowns,
    pub current_rotation: Option<String>,
    pub mobs_targeted: Vec<MobTarget>,
    pub damage_dealt: u64,
    pub mobs_killed: u32,
    pub god_mode_active: bool,
    pub full_map_attack: bool,
    pub missile_storm_count: u8,
}

/// Skill cooldown tracking
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillCooldowns {
    pub skills: HashMap<u16, CooldownEntry>, // skill_id -> cooldown info
}

impl SkillCooldowns {
    pub fn new() -> Self {
        Self { skills: HashMap::new() }
    }

    /// Set a skill cooldown
    pub fn set_cooldown(&mut self, skill_id: u16, duration_ms: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis();
        self.skills.insert(skill_id, CooldownEntry {
            expires_at: now + duration_ms as u128,
            duration_ms,
        });
    }

    /// Check if a skill is on cooldown
    pub fn is_on_cooldown(&self, skill_id: u16) -> bool {
        if let Some(entry) = self.skills.get(&skill_id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis();
            return now < entry.expires_at;
        }
        false
    }

    /// Get remaining cooldown time in ms
    pub fn remaining_cooldown(&self, skill_id: u16) -> u64 {
        if let Some(entry) = self.skills.get(&skill_id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis();
            if now < entry.expires_at {
                return (entry.expires_at - now) as u64;
            }
        }
        0
    }

    /// Clear all cooldowns
    pub fn clear(&mut self) {
        self.skills.clear();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownEntry {
    pub expires_at: u128,
    pub duration_ms: u64,
}

/// Mob target for combat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobTarget {
    pub id: u32,
    pub x: i16,
    pub y: i16,
    pub hp_percent: u8,
    pub is_boss: bool,
    pub mob_id: u32, // Actual mob ID from game
}

impl Default for CombatState {
    fn default() -> Self {
        Self {
            skill_cooldowns: SkillCooldowns::default(),
            current_rotation: None,
            mobs_targeted: vec![],
            damage_dealt: 0,
            mobs_killed: 0,
            god_mode_active: false,
            full_map_attack: false,
            missile_storm_count: 1,
        }
    }
}

/// Combat engine for managing all combat features
pub struct CombatEngine {
    pub state: CombatState,
    pub config: CombatConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatConfig {
    // Feature toggles
    pub kami_enabled: bool,
    pub skill_injection_enabled: bool,
    pub god_mode: bool,
    pub god_mode_boss: bool, // Variant for bosses
    pub full_map_attack: bool,
    pub missile_storm: bool,
    pub missile_count: u8,
    pub auto_attack: bool,
    pub auto_attack_interval_ms: u64,
    pub no_delay: bool,
    pub movement_unlocks: bool,
    pub infinite_flash_jump: bool,
    pub no_air_check: bool,
    pub mob_disarm: bool,
    pub rotation_builder: bool,

    // Kami settings
    pub kami_targeting: TargetingMode,
    pub kami_pickup_items: bool,
    pub kami_avoid_mobs: Vec<u32>,
    pub kami_teleport_distance: f64,

    // Skill settings
    pub skill_delays: HashMap<u16, u64>, // skill_id -> delay_ms
    pub safe_mode: bool, // Don't use skills on cooldown
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TargetingMode {
    Nearest,
    Strongest,
    Random,
    Specific(Vec<u32>), // Mob IDs to target
    InRange(f64),
}

impl Default for TargetingMode {
    fn default() -> Self {
        TargetingMode::Nearest
    }
}

impl Default for CombatConfig {
    fn default() -> Self {
        Self {
            // Feature toggles
            kami_enabled: false,
            skill_injection_enabled: false,
            god_mode: false,
            god_mode_boss: false,
            full_map_attack: false,
            missile_storm: false,
            missile_count: 5,
            auto_attack: false,
            auto_attack_interval_ms: 100,
            no_delay: false,
            movement_unlocks: false,
            infinite_flash_jump: false,
            no_air_check: false,
            mob_disarm: false,
            rotation_builder: false,

            // Kami settings
            kami_targeting: TargetingMode::Nearest,
            kami_pickup_items: true,
            kami_avoid_mobs: vec![],
            kami_teleport_distance: 100.0,

            // Skill settings
            skill_delays: HashMap::new(),
            safe_mode: true,
        }
    }
}

impl CombatEngine {
    pub fn new(config: CombatConfig) -> Self {
        Self {
            state: CombatState::default(),
            config,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(CombatConfig::default())
    }

    /// Select the best mob to target based on current targeting mode
    pub fn select_target<'a>(&self, mobs: &'a [MobTarget]) -> Option<&'a MobTarget> {
        match self.config.kami_targeting {
            TargetingMode::Nearest => {
                mobs.first()
            }
            TargetingMode::Strongest => {
                mobs.iter().max_by_key(|m| m.hp_percent)
            }
            TargetingMode::Random => {
                if mobs.is_empty() {
                    return None;
                }
                let idx = rand::random::<usize>() % mobs.len();
                Some(&mobs[idx])
            }
            TargetingMode::Specific(ref ids) => {
                mobs.iter().find(|m| ids.contains(&m.mob_id))
            }
            TargetingMode::InRange(_max_dist) => {
                mobs.first()
            }
        }
    }

    /// Check if we should use a skill (respects cooldowns)
    pub fn can_use_skill(&self, skill_id: u16) -> bool {
        if !self.config.safe_mode {
            return true;
        }
        !self.state.skill_cooldowns.is_on_cooldown(skill_id)
    }

    /// Record skill usage and set cooldown
    pub fn use_skill(&mut self, skill_id: u16, cooldown_ms: u64) {
        self.state.skill_cooldowns.set_cooldown(skill_id, cooldown_ms);
    }

    /// Enable god mode
    pub fn enable_god_mode(&mut self, boss_variant: bool) {
        self.state.god_mode_active = true;
        self.config.god_mode = true;
        self.config.god_mode_boss = boss_variant;
    }

    /// Disable god mode
    pub fn disable_god_mode(&mut self) {
        self.state.god_mode_active = false;
        self.config.god_mode = false;
        self.config.god_mode_boss = false;
    }

    /// Toggle god mode
    pub fn toggle_god_mode(&mut self) {
        self.config.god_mode = !self.config.god_mode;
        self.state.god_mode_active = self.config.god_mode;
    }

    /// Enable full map attack
    pub fn enable_full_map(&mut self) {
        self.state.full_map_attack = true;
        self.config.full_map_attack = true;
    }

    /// Disable full map attack
    pub fn disable_full_map(&mut self) {
        self.state.full_map_attack = false;
        self.config.full_map_attack = false;
    }

    /// Set missile storm count
    pub fn set_missile_count(&mut self, count: u8) {
        self.state.missile_storm_count = count;
        self.config.missile_count = count;
    }

    /// Record mob kill
    pub fn record_kill(&mut self) {
        self.state.mobs_killed += 1;
    }

    /// Record damage dealt
    pub fn record_damage(&mut self, damage: u64) {
        self.state.damage_dealt += damage;
    }

    /// Get combat stats
    pub fn get_stats(&self) -> CombatStats {
        CombatStats {
            mobs_killed: self.state.mobs_killed,
            damage_dealt: self.state.damage_dealt,
            skills_used: self.state.skill_cooldowns.skills.len() as u32,
            god_mode: self.state.god_mode_active,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatStats {
    pub mobs_killed: u32,
    pub damage_dealt: u64,
    pub skills_used: u32,
    pub god_mode: bool,
}
