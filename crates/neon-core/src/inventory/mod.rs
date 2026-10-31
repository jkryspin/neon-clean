//! Inventory module: item filter, auto sell, auto drop, pet loot

pub mod drop;
pub mod filter;
pub mod pet;
pub mod sell;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryState {
    pub items: Vec<InventoryItem>,
    pub equipped: EquippedItems,
    pub filter_applied: bool,
    pub items_sold: u32,
    pub meso_earned: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryItem {
    pub id: u32,
    pub quantity: u32,
    pub slot: u16,
    pub is_equipped: bool,
    pub is_potential: bool,
    pub potential: Option<Potential>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquippedItems {
    pub head: Option<InventoryItem>,
    pub chest: Option<InventoryItem>,
    pub hands: Option<InventoryItem>,
    pub legs: Option<InventoryItem>,
    pub feet: Option<InventoryItem>,
    pub weapon: Option<InventoryItem>,
    pub accessory1: Option<InventoryItem>,
    pub accessory2: Option<InventoryItem>,
    pub accessory3: Option<InventoryItem>,
    pub ring1: Option<InventoryItem>,
    pub ring2: Option<InventoryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Potential {
    pub stats: Vec<(String, i32)>,
    pub is_master: bool,
}

impl Default for InventoryState {
    fn default() -> Self {
        Self {
            items: vec![],
            equipped: EquippedItems {
                head: None,
                chest: None,
                hands: None,
                legs: None,
                feet: None,
                weapon: None,
                accessory1: None,
                accessory2: None,
                accessory3: None,
                ring1: None,
                ring2: None,
            },
            filter_applied: false,
            items_sold: 0,
            meso_earned: 0,
        }
    }
}
