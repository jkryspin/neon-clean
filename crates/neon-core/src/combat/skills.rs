//! Skill Injection - Packet-level skill casting
//!
//! Feature 2: Inject skill packets directly for precise control
//! Supports custom delays, safe mode, and rotation integration

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConfig {
    pub skill_id: u16,
    pub name: String,
    pub cooldown_ms: u64,
    pub custom_delay_ms: u64, // Extra delay after casting
    pub target_type: TargetType,
    pub charge_time_ms: u64, // For chargeable skills
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TargetType {
    Player,
    Cursor,
    Mob(u32), // Specific mob ID
    NearestMob,
    Boss,
}

impl Default for TargetType {
    fn default() -> Self {
        TargetType::Cursor
    }
}

impl SkillConfig {
    pub fn new(skill_id: u16, name: &str) -> Self {
        Self {
            skill_id,
            name: name.to_string(),
            cooldown_ms: 0,
            custom_delay_ms: 0,
            target_type: TargetType::default(),
            charge_time_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInjectionConfig {
    pub enabled: bool,
    pub safe_mode: bool, // Don't cast on cooldown
    pub default_delay_ms: u64,
    pub skills: HashMap<u16, SkillConfig>,
}

impl Default for SkillInjectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            safe_mode: true,
            default_delay_ms: 50,
            skills: HashMap::new(),
        }
    }
}

pub struct SkillInjection {
    config: SkillInjectionConfig,
    cooldowns: HashMap<u16, u128>, // skill_id -> expires_at (ms)
    skills_cast: u32,
    last_cast: Option<Instant>,
}

impl SkillInjection {
    pub fn new(config: SkillInjectionConfig) -> Self {
        Self {
            config,
            cooldowns: HashMap::new(),
            skills_cast: 0,
            last_cast: None,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(SkillInjectionConfig::default())
    }

    pub fn enable(&mut self) {
        self.config.enabled = true;
    }

    pub fn disable(&mut self) {
        self.config.enabled = false;
    }

    pub fn set_safe_mode(&mut self, safe: bool) {
        self.config.safe_mode = safe;
    }

    /// Register a skill with its cooldown
    pub fn register_skill(&mut self, config: SkillConfig) {
        self.config.skills.insert(config.skill_id, config);
    }

    /// Check if a skill can be cast
    pub fn can_cast(&self, skill_id: u16) -> bool {
        if !self.config.safe_mode {
            return true;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis();
        if let Some(&expires) = self.cooldowns.get(&skill_id) {
            return now >= expires;
        }
        true
    }

    /// Get remaining cooldown for a skill
    pub fn get_cooldown(&self, skill_id: u16) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis();
        if let Some(&expires) = self.cooldowns.get(&skill_id) {
            if now < expires {
                return (expires - now) as u64;
            }
        }
        0
    }

    /// Cast a skill (creates the packet, doesn't send)
    pub fn cast_skill(
        &mut self,
        skill_id: u16,
        target_x: i16,
        target_y: i16,
    ) -> Option<SkillPacket> {
        if !self.can_cast(skill_id) {
            return None;
        }

        // Set cooldown
        let cooldown = self
            .config
            .skills
            .get(&skill_id)
            .map(|s| s.cooldown_ms)
            .unwrap_or(0);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis();
        let expires = now + cooldown as u128;
        self.cooldowns.insert(skill_id, expires);

        self.skills_cast += 1;
        self.last_cast = Some(Instant::now());

        Some(SkillPacket {
            skill_id,
            target_x,
            target_y,
            tick: 0, // Will be filled by packet hook
        })
    }

    /// Get delay after casting a skill
    pub fn get_skill_delay(&self, skill_id: u16) -> u64 {
        self.config
            .skills
            .get(&skill_id)
            .map(|s| s.custom_delay_ms)
            .unwrap_or(self.config.default_delay_ms)
    }

    /// Clear all cooldowns
    pub fn clear_cooldowns(&mut self) {
        self.cooldowns.clear();
    }

    /// Get stats
    pub fn get_stats(&self) -> SkillInjectionStats {
        SkillInjectionStats {
            enabled: self.config.enabled,
            safe_mode: self.config.safe_mode,
            skills_cast: self.skills_cast,
            registered_skills: self.config.skills.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPacket {
    pub skill_id: u16,
    pub target_x: i16,
    pub target_y: i16,
    pub tick: u32,
}

impl SkillPacket {
    /// Create a new skill packet
    pub fn new(skill_id: u16, target_x: i16, target_y: i16) -> Self {
        Self {
            skill_id,
            target_x,
            target_y,
            tick: 0,
        }
    }

    /// Serialize to bytes (simplified - actual packet structure is more complex)
    pub fn to_bytes(&self) -> Vec<u8> {
        // Simplified packet: [length:2][opcode:2][skill_id:2][x:2][y:2][tick:4]
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x000Cu16.to_le_bytes()); // Length
        bytes.extend_from_slice(&0x2720u16.to_le_bytes()); // Skill opcode
        bytes.extend_from_slice(&self.skill_id.to_le_bytes());
        bytes.extend_from_slice(&self.target_x.to_le_bytes());
        bytes.extend_from_slice(&self.target_y.to_le_bytes());
        bytes.extend_from_slice(&self.tick.to_le_bytes());
        bytes
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInjectionStats {
    pub enabled: bool,
    pub safe_mode: bool,
    pub skills_cast: u32,
    pub registered_skills: usize,
}
