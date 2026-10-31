//! Panic mode: one key kills everything

pub struct PanicMode {
    pub active: bool,
    pub features_before_panic: PanicState,
}

#[derive(Debug, Default, Clone)]
pub struct PanicState {
    pub kami_enabled: bool,
    pub skill_injection: bool,
    pub god_mode: bool,
    pub auto_attack: bool,
    pub no_delay: bool,
}

impl PanicMode {
    pub fn new() -> Self {
        Self {
            active: false,
            features_before_panic: PanicState::default(),
        }
    }

    pub fn trigger(&mut self, state: PanicState) {
        self.features_before_panic = state;
        self.active = true;
    }

    pub fn restore(&mut self) -> PanicState {
        let state = self.features_before_panic.clone();
        self.active = false;
        state
    }
}
