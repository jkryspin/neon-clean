//! Item Filter - Whitelist or blacklist drops
//!
//! Feature 20: Filter items by ID, category, or meso value

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FilterMode {
    Whitelist, // Only keep whitelisted items
    Blacklist, // Drop blacklisted items
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemFilterConfig {
    pub enabled: bool,
    pub mode: FilterMode,
    pub item_ids: HashSet<u32>, // Item IDs to filter
    pub categories: HashSet<u32>, // Item categories to filter
    pub min_meso_value: u64, // Minimum meso value to keep
    pub max_meso_value: u64, // Maximum meso value to keep (for blacklist)
    pub keep_quest_items: bool,
    pub keep_equipped: bool,
}

impl Default for ItemFilterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: FilterMode::Blacklist,
            item_ids: HashSet::new(),
            categories: HashSet::new(),
            min_meso_value: 0,
            max_meso_value: u64::MAX,
            keep_quest_items: true,
            keep_equipped: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: u32,
    pub category: u32,
    pub quantity: u32,
    pub meso_value: u64,
    pub is_quest: bool,
    pub is_equipped: bool,
}

pub struct ItemFilter {
    config: ItemFilterConfig,
    pub enabled: bool,
    items_kept: u32,
    items_dropped: u32,
    total_meso_value: u64,
}

impl ItemFilter {
    pub fn new(config: ItemFilterConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            items_kept: 0,
            items_dropped: 0,
            total_meso_value: 0,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(ItemFilterConfig::default())
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

    pub fn set_mode(&mut self, mode: FilterMode) {
        self.config.mode = mode;
    }

    pub fn add_item_id(&mut self, item_id: u32) {
        self.config.item_ids.insert(item_id);
    }

    pub fn remove_item_id(&mut self, item_id: u32) {
        self.config.item_ids.remove(&item_id);
    }

    pub fn add_category(&mut self, category: u32) {
        self.config.categories.insert(category);
    }

    pub fn set_meso_range(&mut self, min: u64, max: u64) {
        self.config.min_meso_value = min;
        self.config.max_meso_value = max;
    }

    /// Check if an item should be kept
    pub fn should_keep(&mut self, item: &Item) -> bool {
        if !self.enabled {
            return true; // Keep everything when disabled
        }

        // Always keep quest items if configured
        if self.config.keep_quest_items && item.is_quest {
            return true;
        }

        // Always keep equipped items if configured
        if self.config.keep_equipped && item.is_equipped {
            return true;
        }

        match self.config.mode {
            FilterMode::Whitelist => self.check_whitelist(item),
            FilterMode::Blacklist => self.check_blacklist(item),
        }
    }

    fn check_whitelist(&self, item: &Item) -> bool {
        // Check if item ID is whitelisted
        if self.config.item_ids.contains(&item.id) {
            return true;
        }

        // Check if category is whitelisted
        if self.config.categories.contains(&item.category) {
            return true;
        }

        // Check meso value
        if item.meso_value >= self.config.min_meso_value {
            return true;
        }

        false
    }

    fn check_blacklist(&self, item: &Item) -> bool {
        // Check if item ID is blacklisted
        if self.config.item_ids.contains(&item.id) {
            return false;
        }

        // Check if category is blacklisted
        if self.config.categories.contains(&item.category) {
            return false;
        }

        // Check meso value
        if item.meso_value < self.config.min_meso_value {
            return false;
        }

        if item.meso_value > self.config.max_meso_value {
            return false;
        }

        true
    }

    /// Record an item decision
    pub fn record_decision(&mut self, item: &Item, kept: bool) {
        if kept {
            self.items_kept += item.quantity;
            self.total_meso_value += item.meso_value * item.quantity as u64;
        } else {
            self.items_dropped += item.quantity;
        }
    }

    pub fn get_stats(&self) -> ItemFilterStats {
        ItemFilterStats {
            enabled: self.enabled,
            mode: match self.config.mode {
                FilterMode::Whitelist => "whitelist".to_string(),
                FilterMode::Blacklist => "blacklist".to_string(),
            },
            items_kept: self.items_kept,
            items_dropped: self.items_dropped,
            total_meso_value: self.total_meso_value,
            filter_size: self.config.item_ids.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemFilterStats {
    pub enabled: bool,
    pub mode: String,
    pub items_kept: u32,
    pub items_dropped: u32,
    pub total_meso_value: u64,
    pub filter_size: usize,
}
