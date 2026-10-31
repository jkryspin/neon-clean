//! Live control from the web UI (or anything that can write a file).
//!
//! The UI writes `neon-control[.<session>].json`; the loader polls its modification time a few times a
//! second. `paused`/`stop` act immediately; `settings` is a *partial* set of options applied to the running
//! brain, so thresholds, mob lists, macros and the like change without restarting anything.

use crate::brain::{BrainConfig, MacroCfg, SkillCfg};
use crate::plan::Plan;
use crate::rules::Rule;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub kami: Option<bool>,
    pub auto_pot: Option<bool>,
    pub auto_ap: Option<bool>,
    pub evasion: Option<bool>,
    pub loot: Option<bool>,
    pub auto_revive: Option<bool>,
    pub max_deaths: Option<u32>,
    pub hp_pot_below: Option<f64>,
    pub mp_pot_below: Option<f64>,
    pub flee_hp_below: Option<f64>,
    pub resume_hp_above: Option<f64>,
    pub attack_range_x: Option<i32>,
    pub attack_range_y: Option<i32>,
    pub stand_off_x: Option<i32>,
    pub evasion_distance: Option<i32>,
    pub gm_evasion: Option<bool>,
    /// Extra staff character names (case-insensitive) to pause for.
    pub gm_names: Option<Vec<String>>,
    /// Mob template ids to fight; an empty list means "anything".
    pub mobs: Option<Vec<u32>>,
    /// Maps the bot may act in; empty = anywhere.
    pub only_map: Option<Vec<u32>>,
    /// Map to grind in (0 = none).
    pub home_map: Option<u32>,
    /// Friends whose presence never pauses the bot.
    pub trusted: Option<Vec<String>>,
    pub macros: Option<Vec<MacroCfg>>,
    pub rotation: Option<Vec<SkillCfg>>,
}

impl Settings {
    pub fn apply(&self, cfg: &mut BrainConfig) {
        if let Some(v) = self.kami {
            cfg.kami = v;
        }
        if let Some(v) = self.auto_pot {
            cfg.auto_pot = v;
        }
        if let Some(v) = self.auto_ap {
            cfg.auto_ap = v;
        }
        if let Some(v) = self.evasion {
            cfg.evasion = v;
        }
        if let Some(v) = self.loot {
            cfg.loot = v;
        }
        if let Some(v) = self.auto_revive {
            cfg.auto_revive = v;
        }
        if let Some(v) = self.max_deaths {
            cfg.max_deaths = v.min(1000);
        }
        if let Some(v) = self.hp_pot_below {
            cfg.hp_pot_below = v.clamp(0.0, 100.0);
        }
        if let Some(v) = self.mp_pot_below {
            cfg.mp_pot_below = v.clamp(0.0, 100.0);
        }
        if let Some(v) = self.flee_hp_below {
            cfg.flee_hp_below = v.clamp(0.0, 100.0);
        }
        if let Some(v) = self.resume_hp_above {
            cfg.resume_hp_above = v.clamp(0.0, 100.0);
        }
        if let Some(v) = self.attack_range_x {
            cfg.attack_range_x = v.clamp(10, 400);
        }
        if let Some(v) = self.attack_range_y {
            cfg.attack_range_y = v.clamp(5, 300);
        }
        if let Some(v) = self.stand_off_x {
            cfg.stand_off_x = v.clamp(5, 300);
        }
        if let Some(v) = self.evasion_distance {
            cfg.evasion_distance = v.clamp(50, 2000);
        }
        if let Some(v) = self.gm_evasion {
            cfg.gm_evasion = v;
        }
        if let Some(v) = &self.gm_names {
            cfg.gm_names = v.iter().map(|s| s.to_lowercase()).collect();
        }
        if let Some(v) = &self.mobs {
            cfg.allowed_mobs = if v.is_empty() {
                None
            } else {
                Some(v.iter().copied().collect::<HashSet<_>>())
            };
        }
        if let Some(v) = &self.only_map {
            cfg.allowed_maps = if v.is_empty() {
                None
            } else {
                Some(v.iter().copied().collect::<HashSet<_>>())
            };
        }
        if let Some(v) = self.home_map {
            cfg.home_map = (v != 0).then_some(v);
        }
        if let Some(v) = &self.trusted {
            cfg.trusted_igns = v.iter().map(|s| s.to_lowercase()).collect();
        }
        if let Some(v) = &self.macros {
            cfg.macros = v.clone();
        }
        if let Some(v) = &self.rotation {
            cfg.rotation = v.clone();
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Control {
    /// Bumped by the writer on every change (also lets a re-sent identical command register).
    pub seq: u64,
    pub paused: bool,
    pub stop: bool,
    pub settings: Settings,
    /// A new schedule/plan; an empty `steps` list clears the current one.
    pub plan: Option<Plan>,
    /// Replaces the rule list when present (an empty list clears it).
    pub rules: Option<Vec<Rule>>,
}

pub struct ControlWatcher {
    path: PathBuf,
    mtime: Option<SystemTime>,
    last_check: Instant,
    last_seq: Option<u64>,
}

impl ControlWatcher {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            mtime: None,
            last_check: Instant::now() - Duration::from_secs(1),
            last_seq: None,
        }
    }

    /// Returns a new command when the file changed since the last call. Rate-limited to ~4 checks/s.
    pub fn poll(&mut self) -> Option<Control> {
        if self.last_check.elapsed() < Duration::from_millis(250) {
            return None;
        }
        self.last_check = Instant::now();
        let m = std::fs::metadata(&self.path).ok()?.modified().ok()?;
        if self.mtime == Some(m) {
            return None;
        }
        self.mtime = Some(m);
        let text = std::fs::read_to_string(&self.path).ok()?;
        let c: Control = serde_json::from_str(&text).ok()?; // half-written file: try again next change
        if self.last_seq == Some(c.seq) {
            return None;
        }
        self.last_seq = Some(c.seq);
        Some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_settings_only_touch_what_they_name() {
        let mut cfg = BrainConfig::default();
        let before = cfg.clone();
        let s: Settings =
            serde_json::from_str(r#"{"flee_hp_below": 60, "mobs": [2,3], "home_map": 10001030}"#)
                .unwrap();
        s.apply(&mut cfg);
        assert_eq!(cfg.flee_hp_below, 60.0);
        assert_eq!(cfg.allowed_mobs, Some([2u32, 3].into_iter().collect()));
        assert_eq!(cfg.home_map, Some(10001030));
        assert_eq!(cfg.resume_hp_above, before.resume_hp_above);
        assert_eq!(cfg.attack_range_x, before.attack_range_x);
    }

    #[test]
    fn empty_lists_and_zero_clear_restrictions() {
        let mut cfg = BrainConfig {
            home_map: Some(5),
            allowed_mobs: Some([1u32].into_iter().collect()),
            ..BrainConfig::default()
        };
        let s: Settings =
            serde_json::from_str(r#"{"mobs": [], "only_map": [], "home_map": 0}"#).unwrap();
        s.apply(&mut cfg);
        assert!(cfg.allowed_mobs.is_none() && cfg.allowed_maps.is_none() && cfg.home_map.is_none());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let mut cfg = BrainConfig::default();
        let s: Settings = serde_json::from_str(
            r#"{"flee_hp_below": 900, "attack_range_x": 100000, "stand_off_x": -3}"#,
        )
        .unwrap();
        s.apply(&mut cfg);
        assert_eq!(cfg.flee_hp_below, 100.0);
        assert_eq!(cfg.attack_range_x, 400);
        assert_eq!(cfg.stand_off_x, 5);
    }

    #[test]
    fn watcher_returns_each_new_command_once() {
        let dir = std::env::temp_dir().join(format!("neon-ctl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.json");
        let mut w = ControlWatcher::new(path.clone());
        assert!(w.poll().is_none(), "no file yet");
        std::fs::write(&path, r#"{"seq":1,"paused":true}"#).unwrap();
        w.last_check -= Duration::from_secs(1);
        let c = w.poll().expect("first command");
        assert!(c.paused && !c.stop);
        w.last_check -= Duration::from_secs(1);
        assert!(w.poll().is_none(), "unchanged");
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&path, r#"{"seq":2,"stop":true}"#).unwrap();
        w.last_check -= Duration::from_secs(1);
        assert!(w.poll().expect("second").stop);
        // A half-written (invalid) file is ignored, not an error.
        std::fs::write(&path, r#"{"seq":3,"pa"#).unwrap();
        w.last_check -= Duration::from_secs(1);
        assert!(w.poll().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
