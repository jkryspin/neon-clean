//! A scripted click sequence on one game window (Starforce, Cube, Flame, Inner Ability, ...).
//!
//! The windows' layouts are calibrated live once and written into the plan as reference-window pixels, like the
//! shop step. The sequence is run `repeat` times, with a pause after every click so the client can redraw.

use serde::{Deserialize, Serialize};

/// One click of a sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiClick {
    /// Reference-window pixels (1422x900 frame, like `shop` coordinates).
    pub at: (i32, i32),
    /// Pause after the click (ms), for the window to react.
    #[serde(default = "default_wait")]
    pub wait_ms: u64,
}

pub fn one() -> u32 {
    1
}

fn default_wait() -> u64 {
    400
}

/// What the bot is asked to run.
#[derive(Debug, Clone, PartialEq)]
pub struct UiJob {
    pub name: String,
    pub clicks: Vec<UiClick>,
    pub repeat: u32,
}

/// Most repeats a single step may ask for: a mistyped plan must not click for hours.
pub const MAX_REPEAT: u32 = 200;

impl UiJob {
    /// The clicks in the order they are made, `repeat` (clamped to 1..=MAX_REPEAT) times over.
    pub fn sequence(&self) -> Vec<&UiClick> {
        let n = self.repeat.clamp(1, MAX_REPEAT) as usize;
        (0..n).flat_map(|_| self.clicks.iter()).collect()
    }
}

/// Make the clicks on the live window; returns how many were made.
pub fn run(job: &UiJob) -> Result<usize, String> {
    let win = crate::shop::Win::find()?;
    let seq = job.sequence();
    for c in &seq {
        win.click(c.at)?;
        std::thread::sleep(std::time::Duration::from_millis(c.wait_ms));
    }
    Ok(seq.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(repeat: u32) -> UiJob {
        UiJob {
            name: "cube".into(),
            clicks: vec![
                UiClick {
                    at: (1, 2),
                    wait_ms: 1,
                },
                UiClick {
                    at: (3, 4),
                    wait_ms: 1,
                },
            ],
            repeat,
        }
    }

    #[test]
    fn the_sequence_repeats_in_order() {
        let ats: Vec<_> = job(2).sequence().iter().map(|c| c.at).collect();
        assert_eq!(ats, vec![(1, 2), (3, 4), (1, 2), (3, 4)]);
    }

    #[test]
    fn repeat_is_at_least_one_and_capped() {
        assert_eq!(job(0).sequence().len(), 2);
        assert_eq!(job(100_000).sequence().len(), 2 * MAX_REPEAT as usize);
    }
}
