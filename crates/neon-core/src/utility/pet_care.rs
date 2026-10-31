//! Auto pet care: feed and resummon pets

pub struct AutoPetCare {
    pub enabled: bool,
    pub auto_feed: bool,
    pub auto_resummon: bool,
}

impl AutoPetCare {
    pub fn new() -> Self {
        Self {
            enabled: false,
            auto_feed: true,
            auto_resummon: true,
        }
    }
}
