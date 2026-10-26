//! Movement unlocks: infinite flash jump, no air check

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MovementUnlocks {
    pub infinite_flash_jump: bool,
    pub no_air_check: bool,
}

impl MovementUnlocks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn toggle_flash_jump(&mut self) {
        self.infinite_flash_jump = !self.infinite_flash_jump;
    }

    pub fn toggle_air_check(&mut self) {
        self.no_air_check = !self.no_air_check;
    }
}
