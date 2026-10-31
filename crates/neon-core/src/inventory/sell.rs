//! Auto Sell - Clear items at shops
//!
//! Feature 21: Automatically sell items at NPC shops

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSellConfig {
    pub enabled: bool,
    pub item_ids: HashSet<u32>,         // Items to sell (empty = all)
    pub exclude_item_ids: HashSet<u32>, // Items to keep
    pub min_price: u32,                 // Minimum price to accept
    pub auto_approach_shop: bool,       // Automatically approach shop NPC
    pub rush_to_shop: bool,             // Use rusher to get to shop
}

impl Default for AutoSellConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            item_ids: HashSet::new(),
            exclude_item_ids: HashSet::new(),
            min_price: 1,
            auto_approach_shop: false,
            rush_to_shop: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoSellStats {
    pub items_sold: u32,
    pub meso_earned: u64,
    pub shop_visits: u32,
}

pub struct AutoSell {
    config: AutoSellConfig,
    pub enabled: bool,
    stats: AutoSellStats,
    in_shop: bool,
}

impl AutoSell {
    pub fn new(config: AutoSellConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: AutoSellStats::default(),
            in_shop: false,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(AutoSellConfig::default())
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

    pub fn add_item(&mut self, item_id: u32) {
        self.config.item_ids.insert(item_id);
    }

    pub fn exclude_item(&mut self, item_id: u32) {
        self.config.exclude_item_ids.insert(item_id);
    }

    pub fn clear_lists(&mut self) {
        self.config.item_ids.clear();
        self.config.exclude_item_ids.clear();
    }

    /// Check if an item should be sold
    pub fn should_sell(&self, item_id: u32, price: u32) -> bool {
        if !self.enabled {
            return false;
        }

        // Check if excluded
        if self.config.exclude_item_ids.contains(&item_id) {
            return false;
        }

        // Check if in sell list (if list is not empty)
        if !self.config.item_ids.is_empty() && !self.config.item_ids.contains(&item_id) {
            return false;
        }

        // Check minimum price
        if price < self.config.min_price {
            return false;
        }

        true
    }

    /// Record a sale
    pub fn record_sale(&mut self, count: u32, price: u32) {
        self.stats.items_sold += count;
        self.stats.meso_earned += (count as u64) * (price as u64);
    }

    /// Record a shop visit
    pub fn record_shop_visit(&mut self) {
        self.stats.shop_visits += 1;
        self.in_shop = true;
    }

    pub fn leave_shop(&mut self) {
        self.in_shop = false;
    }

    pub fn is_in_shop(&self) -> bool {
        self.in_shop
    }

    pub fn get_stats(&self) -> AutoSellStats {
        self.stats.clone()
    }
}
