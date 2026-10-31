//! Auto character creation

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterClass {
    Warrior,
    Magician,
    Bowman,
    Thief,
    Pirate,
}

pub struct AutoChar {
    pub enabled: bool,
    pub class: CharacterClass,
    pub name_prefix: String,
    pub skip_intro: bool,
}

impl AutoChar {
    pub fn new() -> Self {
        Self {
            enabled: false,
            class: CharacterClass::Warrior,
            name_prefix: "Neon".to_string(),
            skip_intro: true,
        }
    }
}
