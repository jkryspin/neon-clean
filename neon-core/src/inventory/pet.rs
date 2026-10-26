//! Pet Loot - Pet teleports to drops for instant pickup
//!
//! Feature 23: Use pet's teleport ability to quickly collect drops

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetLootConfig {
    pub enabled: bool,
    pub auto_teleport: bool, // Auto teleport to drops
    pub max_distance: f64, // Max distance to teleport (pixels)
    pub priority_items: Vec<u32>, // Item IDs to prioritize
    pub skip_common: bool, // Skip common items
    pub only_valuable: bool, // Only teleport to valuable items
    pub min_meso_value: u64, // Minimum meso value to teleport for
}

impl Default for PetLootConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_teleport: true,
            max_distance: 500.0,
            priority_items: vec![],
            skip_common: false,
            only_valuable: false,
            min_meso_value: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropItem {
    pub item_id: u32,
    pub x: i16,
    pub y: i16,
    pub quantity: u32,
    pub meso_value: u64,
    pub is_common: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PetLootStats {
    pub teleports_used: u32,
    pub items_collected: u32,
    pub total_meso_value: u64,
}

pub struct PetLoot {
    config: PetLootConfig,
    pub enabled: bool,
    stats: PetLootStats,
    pending_drops: Vec<DropItem>,
}

impl PetLoot {
    pub fn new(config: PetLootConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: PetLootStats::default(),
            pending_drops: vec![],
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(PetLootConfig::default())
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

    pub fn add_priority_item(&mut self, item_id: u32) {
        if !self.config.priority_items.contains(&item_id) {
            self.config.priority_items.push(item_id);
        }
    }

    pub fn set_min_value(&mut self, value: u64) {
        self.config.min_meso_value = value;
    }

    /// Add a drop to the pending list
    pub fn add_drop(&mut self, drop: DropItem) {
        self.pending_drops.push(drop);
    }

    /// Get the next drop to teleport to
    pub fn next_drop(&self, player_x: i16, player_y: i16) -> Option<&DropItem> {
        if self.pending_drops.is_empty() {
            return None;
        }

        // Sort by priority and distance
        self.pending_drops.iter()
            .filter(|d| self.should_teleport_to(d, player_x, player_y))
            .min_by_key(|d| {
                // Priority items first, then by distance
                let priority = if self.config.priority_items.contains(&d.item_id) {
                    0
                } else {
                    1
                };
                let dist = self.distance_to(d, player_x, player_y);
                (priority, dist as u64)
            })
    }

    fn should_teleport_to(&self, drop: &DropItem, player_x: i16, player_y: i16) -> bool {
        // Check distance
        if self.distance_to(drop, player_x, player_y) > self.config.max_distance {
            return false;
        }

        // Check if common and skip_common is set
        if self.config.skip_common && drop.is_common {
            return false;
        }

        // Check if valuable and only_valuable is set
        if self.config.only_valuable && drop.meso_value < self.config.min_meso_value {
            return false;
        }

        true
    }

    fn distance_to(&self, drop: &DropItem, player_x: i16, player_y: i16) -> f64 {
        let dx = drop.x as i32 - player_x as i32;
        let dy = drop.y as i32 - player_y as i32;
        ((dx * dx + dy * dy) as f64).sqrt()
    }

    /// Record a teleport usage
    pub fn record_teleport(&mut self) {
        self.stats.teleports_used += 1;
    }

    /// Record an item collection
    pub fn record_collection(&mut self, item_id: u32, quantity: u32, meso_value: u64) {
        self.stats.items_collected += quantity;
        self.stats.total_meso_value += meso_value;

        // Remove collected item from pending drops
        self.pending_drops.retain(|d| d.item_id != item_id);
    }

    pub fn get_stats(&self) -> PetLootStats {
        self.stats.clone()
    }

    pub fn pending_count(&self) -> usize {
        self.pending_drops.len()
    }
}
