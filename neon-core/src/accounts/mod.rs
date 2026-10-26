//! Accounts module: login, relaunch, char create, offline mode

pub mod login;
pub mod relaunch;
pub mod char_create;
pub mod offline;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    pub logged_in: bool,
    pub character_name: Option<String>,
    pub world: String,
    pub channel: u32,
    pub offline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameProfile {
    pub id: String,
    pub name: String,
    pub username: String,
    pub encrypted_password: Vec<u8>,
    pub character_slot: u32,
    pub world: String,
    pub channel: u32,
    pub proxy: Option<String>,
}

impl Default for AccountState {
    fn default() -> Self {
        Self {
            logged_in: false,
            character_name: None,
            world: "Windia".to_string(),
            channel: 1,
            offline: false,
        }
    }
}
