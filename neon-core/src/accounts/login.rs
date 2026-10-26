//! Auto login: world, channel, character slot

pub struct AutoLogin {
    pub enabled: bool,
    pub world: String,
    pub channel: u32,
    pub character_slot: u32,
}

impl AutoLogin {
    pub fn new() -> Self {
        Self {
            enabled: false,
            world: "Windia".to_string(),
            channel: 1,
            character_slot: 1,
        }
    }

    pub fn set_world(&mut self, world: String) {
        self.world = world;
    }

    pub fn set_channel(&mut self, channel: u32) {
        self.channel = channel;
    }

    pub fn set_character_slot(&mut self, slot: u32) {
        self.character_slot = slot;
    }
}
