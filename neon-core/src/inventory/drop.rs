//! Auto Drop - Loot everything, drop the junk
//!
//! Feature 22: Automatically drop unwanted items from drops

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoDropConfig {
    pub enabled: bool,
    pub drop_all: bool, // Drop all items
    pub keep_item_ids: HashSet<u32>, // Items to keep
    pub drop_item_ids: HashSet<u32>, // Items to drop
    pub keep_quest_counters: bool, // Keep quest counter items
    pub keep_equipment: bool, // Keep equipment
    pub keep_consumables: bool, // Keep potions, scrolls, etc.
    pub min_drop_quantity: u32, // Only drop if quantity >= this
}

impl Default for AutoDropConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            drop_all: false,
            keep_item_ids: HashSet::new(),
            drop_item_ids: HashSet::new(),
            keep_quest_counters: true,
            keep_equipment: true,
            keep_consumables: true,
            min_drop_quantity: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoDropStats {
    pub items_dropped: u32,
    pub items_kept: u32,
    pub total_quantity_dropped: u32,
}

pub struct AutoDrop {
    config: AutoDropConfig,
    pub enabled: bool,
    stats: AutoDropStats,
}

impl AutoDrop {
    pub fn new(config: AutoDropConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: AutoDropStats::default(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(AutoDropConfig::default())
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

    pub fn set_drop_all(&mut self, drop_all: bool) {
        self.config.drop_all = drop_all;
    }

    pub fn add_keep_item(&mut self, item_id: u32) {
        self.config.keep_item_ids.insert(item_id);
    }

    pub fn add_drop_item(&mut self, item_id: u32) {
        self.config.drop_item_ids.insert(item_id);
    }

    /// Check if an item should be dropped
    pub fn should_drop(&self, item_id: u32, quantity: u32, is_quest: bool, is_equipment: bool, is_consumable: bool) -> bool {
        if !self.enabled {
            return false;
        }

        // Drop all if configured
        if self.config.drop_all {
            // But respect keep lists
            if self.config.keep_item_ids.contains(&item_id) {
                return false;
            }
            if self.config.keep_quest_counters && is_quest {
                return false;
            }
            if self.config.keep_equipment && is_equipment {
                return false;
            }
            if self.config.keep_consumables && is_consumable {
                return false;
            }
            return quantity >= self.config.min_drop_quantity;
        }

        // Check explicit drop list
        if self.config.drop_item_ids.contains(&item_id) {
            return quantity >= self.config.min_drop_quantity;
        }

        // Check explicit keep list
        if self.config.keep_item_ids.contains(&item_id) {
            return false;
        }

        // Default: don't drop
        false
    }

    /// Record a drop
    pub fn record_drop(&mut self, quantity: u32) {
        self.stats.items_dropped += 1;
        self.stats.total_quantity_dropped += quantity;
    }

    /// Record an item kept
    pub fn record_kept(&mut self, quantity: u32) {
        self.stats.items_kept += 1;
    }

    pub fn get_stats(&self) -> AutoDropStats {
        self.stats.clone()
    }
}
