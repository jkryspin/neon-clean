//! Neon bot engine: state in, actions out, carried by Windows input.

pub mod bot;
pub mod brain;
pub mod char_build;
pub mod control;
pub mod input;
pub mod nav;
pub mod notify;
pub mod party;
pub mod plan;
pub mod quest;
pub mod rules;
pub mod runner;
pub mod shop;
pub mod sim;
pub mod stats;
pub mod status;
pub mod ui;
pub mod xkeys;

pub use bot::{Bot, DryRun, Keys};
pub use brain::{Brain, BrainConfig};
