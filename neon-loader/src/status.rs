//! Live status snapshot for the web UI: written atomically as JSON a couple of
//! times a second, so the UI never needs a connection to the bot itself.

use crate::brain::{BrainConfig, BrainStats, Mode};
use crate::nav::MapGeom;
use crate::rules::Rule;
use neon_state::{exp_remaining, exp_to_next, GameState};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const MAX_EVENTS: usize = 80;
const MAX_SERIES: usize = 360;
const SERIES_EVERY_MS: u128 = 5_000;
const WRITE_EVERY_MS: u128 = 500;

#[derive(Serialize, Clone)]
struct Event {
    t: u64,
    msg: String,
}

#[derive(Serialize, Clone)]
struct Sample {
    t: u64,
    /// level + fractional progress through it, so one number charts leveling.
    progress: f64,
}

/// Compact map outline for the UI: platforms as [x1,y1,x2,y2], ladders as [x,y1,y2,is_ladder].
#[derive(Serialize, Clone, Default)]
struct Outline {
    platforms: Vec<[i32; 4]>,
    ladders: Vec<[i32; 4]>,
}

/// Session progress derived from the EXP counter.
#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct Progress {
    /// EXP earned this session (deaths do not subtract).
    pub exp_gained: u64,
    pub exp_per_hour: f64,
    /// Minutes until the next level at the current rate.
    pub eta_next_minutes: Option<u64>,
    /// Minutes until `target_level` at the current rate.
    pub eta_target_minutes: Option<u64>,
    pub session_minutes: f64,
}

/// Accumulates EXP gained from the (level, exp) readings, across level-ups.
#[derive(Default)]
pub struct ExpTracker {
    last: Option<(u16, u64)>,
    pub gained: u64,
}

impl ExpTracker {
    pub fn feed(&mut self, level: Option<u16>, exp: Option<u64>) {
        let (Some(l), Some(e)) = (level, exp) else { return };
        if let Some((pl, pe)) = self.last {
            if l == pl && e >= pe {
                self.gained += e - pe;
            } else if l == pl + 1 {
                // Finished the old level and carried over into the new one.
                self.gained += exp_to_next(pl).unwrap_or(0).saturating_sub(pe) + e;
            }
            // Same level, lower exp = death penalty; level down or a big jump = bad read: just re-anchor.
        }
        self.last = Some((l, e));
    }

    pub fn progress(&self, session_minutes: f64, level: Option<u16>, exp: Option<u64>, target: Option<u16>) -> Progress {
        let rate_per_min = if session_minutes >= 1.0 { self.gained as f64 / session_minutes } else { 0.0 };
        let eta = |remaining: Option<u64>| -> Option<u64> {
            (rate_per_min > 0.0).then(|| remaining.map(|r| (r as f64 / rate_per_min).ceil() as u64)).flatten()
        };
        let next = level.zip(exp).and_then(|(l, e)| exp_remaining(l, e, l + 1));
        let to_target = level.zip(exp).zip(target).and_then(|((l, e), t)| exp_remaining(l, e, t));
        Progress {
            exp_gained: self.gained,
            exp_per_hour: rate_per_min * 60.0,
            eta_next_minutes: eta(next),
            eta_target_minutes: eta(to_target),
            session_minutes,
        }
    }
}

/// Appends one JSON line per sample to `<dir>/<session>.jsonl` so sessions can be compared later.
pub struct HistoryLog {
    path: PathBuf,
    last: Option<Instant>,
    every: std::time::Duration,
}

impl HistoryLog {
    pub fn new(dir: PathBuf, session: &str, every: std::time::Duration) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self { path: dir.join(format!("{session}.jsonl")), last: None, every }
    }

    /// Append `record` when the interval has elapsed (or `force`).
    pub fn append(&mut self, record: &serde_json::Value, force: bool) {
        if !force && self.last.map_or(false, |t| t.elapsed() < self.every) {
            return;
        }
        self.last = Some(Instant::now());
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(f, "{record}");
        }
    }
}

#[derive(Serialize)]
struct Snapshot<'a> {
    progress: Progress,
    geometry: &'a Outline,
    updated: u64,
    started: u64,
    running: bool,
    dry_run: bool,
    mode: &'a str,
    pause_reason: &'a str,
    config: Option<&'a BrainConfig>,
    plan: &'a Option<PlanInfo>,
    rules: &'a [Rule],
    session: &'a str,
    target_level: Option<u16>,
    stats: &'a BrainStats,
    kills_per_hour: f64,
    /// Frames per second arriving from the stream.
    fps: f64,
    /// Age of a frame when the bot gets it (stream read -> decision), ms.
    latency_ms: f64,
    state: Option<&'a GameState>,
    events: &'a VecDeque<Event>,
    series: &'a VecDeque<Sample>,
    /// Shadow simulation: where the movement model thinks the character is, and how far off it runs.
    sim: &'a Option<SimInfo>,
}

/// Shadow-simulation snapshot for the panel.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SimInfo {
    pub x: f64,
    pub y: f64,
    pub grounded: bool,
    pub summary: String,
}

/// Which step of the running plan we are in.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PlanInfo {
    pub name: String,
    pub step: usize,
    pub total: usize,
    pub what: String,
}

/// Extra fields the executor hands over with each update.
pub struct StatusExtra<'a> {
    pub pause_reason: &'a str,
    pub config: &'a BrainConfig,
    pub plan: Option<(String, usize, usize, String)>,
    pub rules: &'a [Rule],
}

pub struct StatusWriter {
    path: PathBuf,
    dry_run: bool,
    target_level: Option<u16>,
    started_ms: u64,
    started: Instant,
    last_write: Option<Instant>,
    last_sample: Option<Instant>,
    fps_window: (Instant, u32, f64),
    latency: f64,
    extra_reason: String,
    extra_config: Option<BrainConfig>,
    plan: Option<PlanInfo>,
    rules_copy: Vec<Rule>,
    session: String,
    events: VecDeque<Event>,
    series: VecDeque<Sample>,
    exp: ExpTracker,
    history: Option<HistoryLog>,
    maps_dir: PathBuf,
    outline: Outline,
    outline_map: Option<u32>,
    sim: Option<SimInfo>,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl StatusWriter {
    pub fn new(path: PathBuf, maps_dir: PathBuf, dry_run: bool, target_level: Option<u16>) -> Self {
        Self {
            path,
            dry_run,
            target_level,
            started_ms: now_ms(),
            started: Instant::now(),
            last_write: None,
            last_sample: None,
            fps_window: (Instant::now(), 0, 0.0),
            latency: 0.0,
            extra_reason: String::new(),
            extra_config: None,
            plan: None,
            rules_copy: vec![],
            session: "main".into(),
            events: VecDeque::new(),
            series: VecDeque::new(),
            exp: ExpTracker::default(),
            history: None,
            maps_dir,
            outline: Outline::default(),
            outline_map: None,
            sim: None,
        }
    }

    pub fn with_history(mut self, dir: PathBuf, every: std::time::Duration) -> Self {
        self.history = Some(HistoryLog::new(dir, &self.session, every));
        self
    }

    /// Replace the plan progress shown in the panel (the final snapshot shows the plan as finished).
    pub fn set_plan(&mut self, plan: Option<(String, usize, usize, String)>) {
        self.plan = plan.map(|(name, step, total, what)| PlanInfo { name, step, total, what });
    }

    pub fn set_sim(&mut self, sim: Option<SimInfo>) {
        self.sim = sim;
    }

    pub fn set_session(&mut self, name: &str) {
        self.session = name.to_string();
    }

    pub fn target_level(&self) -> Option<u16> {
        self.target_level
    }

    pub fn event(&mut self, msg: impl Into<String>) {
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(Event { t: now_ms(), msg: msg.into() });
    }

    pub fn update(&mut self, state: &GameState, stats: &BrainStats, mode: Mode, running: bool, latency_ms: f64, extra: StatusExtra<'_>) {
        self.extra_reason = extra.pause_reason.to_string();
        self.extra_config = Some(extra.config.clone());
        if self.rules_copy.as_slice() != extra.rules {
            self.rules_copy = extra.rules.to_vec();
        }
        self.plan = extra.plan.map(|(name, step, total, what)| PlanInfo { name, step, total, what });
        self.latency = latency_ms;
        self.exp.feed(state.player.level, state.player.exp);
        if let Some(h) = self.history.as_mut() {
            let p = self.exp.progress(self.started.elapsed().as_secs_f64() / 60.0, state.player.level, state.player.exp, self.target_level);
            h.append(
                &serde_json::json!({
                    "t": now_ms(), "session": self.session, "level": state.player.level, "exp": state.player.exp,
                    "exp_gained": p.exp_gained, "exp_per_hour": p.exp_per_hour.round(), "kills": stats.mobs_killed,
                    "deaths": stats.deaths, "attacks": stats.attacks, "hp": state.player.hp, "map": state.player.map_id,
                }),
                false,
            );
        }
        self.fps_window.1 += 1;
        let el = self.fps_window.0.elapsed().as_secs_f64();
        if el >= 2.0 {
            self.fps_window = (Instant::now(), 0, self.fps_window.1 as f64 / el);
        }
        if self.last_sample.map_or(true, |t| t.elapsed().as_millis() >= SERIES_EVERY_MS) {
            if let (Some(l), Some(e), Some(n)) =
                (state.player.level, state.player.exp, state.player.exp_needed.or_else(|| state.player.level.and_then(exp_to_next)))
            {
                if n > 0 {
                    if self.series.len() == MAX_SERIES {
                        self.series.pop_front();
                    }
                    self.series.push_back(Sample { t: now_ms(), progress: l as f64 + (e as f64 / n as f64).min(0.999) });
                    self.last_sample = Some(Instant::now());
                }
            }
        }
        if state.player.map_id.is_some() && state.player.map_id != self.outline_map {
            self.outline_map = state.player.map_id;
            self.outline = state
                .player
                .map_id
                .and_then(|m| MapGeom::load(&self.maps_dir, m))
                .map(|g| Outline {
                    platforms: g.footholds.iter().filter(|f| f.x1 != f.x2).map(|f| [f.x1, f.y1, f.x2, f.y2]).collect(),
                    ladders: g.ladders.iter().map(|l| [l.x, l.y1, l.y2, l.ladder as i32]).collect(),
                })
                .unwrap_or_default();
        }
        if self.last_write.map_or(false, |t| t.elapsed().as_millis() < WRITE_EVERY_MS) {
            return;
        }
        self.last_write = Some(Instant::now());
        self.write(Some(state), stats, mode, running);
    }

    /// Final snapshot when the run ends: `running: false`, plus a closing history record.
    pub fn finish(&mut self, state: Option<&GameState>, stats: &BrainStats, mode: Mode) {
        self.write(state, stats, mode, false);
        if let Some(h) = self.history.as_mut() {
            let p = self.exp.progress(self.started.elapsed().as_secs_f64() / 60.0, state.and_then(|s| s.player.level), state.and_then(|s| s.player.exp), self.target_level);
            h.append(
                &serde_json::json!({
                    "t": now_ms(), "session": self.session, "event": "end", "level": state.and_then(|s| s.player.level),
                    "exp_gained": p.exp_gained, "kills": stats.mobs_killed, "deaths": stats.deaths,
                }),
                true,
            );
        }
    }

    pub fn write(&self, state: Option<&GameState>, stats: &BrainStats, mode: Mode, running: bool) {
        let hours = self.started.elapsed().as_secs_f64() / 3600.0;
        let (lvl, exp) = state.map_or((None, None), |s| (s.player.level, s.player.exp));
        let snap = Snapshot {
            progress: self.exp.progress(hours * 60.0, lvl, exp, self.target_level),
            geometry: &self.outline,
            updated: now_ms(),
            started: self.started_ms,
            running,
            dry_run: self.dry_run,
            pause_reason: &self.extra_reason,
            config: self.extra_config.as_ref(),
            plan: &self.plan,
            rules: &self.rules_copy,
            session: &self.session,
            mode: match mode {
                Mode::Running => "running",
                Mode::Paused => "paused",
            },
            target_level: self.target_level,
            stats,
            fps: self.fps_window.2,
            latency_ms: self.latency,
            kills_per_hour: if hours > 0.003 { stats.mobs_killed as f64 / hours } else { 0.0 },
            state,
            events: &self.events,
            series: &self.series,
            sim: &self.sim,
        };
        let Ok(json) = serde_json::to_vec(&snap) else { return };
        let tmp = self.path.with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_accumulates_within_a_level_and_across_a_level_up() {
        let mut t = ExpTracker::default();
        t.feed(Some(7), Some(100));
        t.feed(Some(7), Some(160)); // +60
        t.feed(Some(7), Some(559)); // +399
        assert_eq!(t.gained, 459);
        t.feed(Some(8), Some(30)); // finished level 7 (560): +1, then 30 into level 8
        assert_eq!(t.gained, 459 + 1 + 30);
    }

    #[test]
    fn a_death_penalty_or_bad_read_does_not_count_and_does_not_wreck_the_baseline() {
        let mut t = ExpTracker::default();
        t.feed(Some(7), Some(400));
        t.feed(Some(7), Some(350)); // died: exp dropped
        assert_eq!(t.gained, 0);
        t.feed(Some(7), Some(380)); // +30 from the new baseline
        assert_eq!(t.gained, 30);
        t.feed(Some(12), Some(5)); // garbage level jump
        assert_eq!(t.gained, 30);
        t.feed(None, None);
        t.feed(Some(12), Some(25));
        assert_eq!(t.gained, 50);
    }

    #[test]
    fn eta_uses_the_session_rate() {
        let mut t = ExpTracker::default();
        t.feed(Some(7), Some(0));
        t.feed(Some(7), Some(300)); // 300 exp so far
        // 30 minutes in: 600 exp/hour.
        let p = t.progress(30.0, Some(7), Some(300), Some(10));
        assert_eq!(p.exp_per_hour, 600.0);
        assert_eq!(p.eta_next_minutes, Some(26), "260 to go at 10/min");
        // 260 + 840 + 1242 = 2342 at 10/min
        assert_eq!(p.eta_target_minutes, Some(235));
        // Too early or nothing gained: no estimate.
        assert_eq!(t.progress(0.5, Some(7), Some(300), Some(10)).eta_next_minutes, None);
        assert_eq!(ExpTracker::default().progress(60.0, Some(7), Some(0), Some(10)).eta_target_minutes, None);
    }

    #[test]
    fn history_appends_json_lines_at_most_once_per_interval() {
        let dir = std::env::temp_dir().join(format!("neon-hist-{}", std::process::id()));
        let mut h = HistoryLog::new(dir.clone(), "t", std::time::Duration::from_secs(3600));
        h.append(&serde_json::json!({"a": 1}), false);
        h.append(&serde_json::json!({"a": 2}), false); // inside the interval: dropped
        h.append(&serde_json::json!({"a": 3}), true); // forced
        let text = std::fs::read_to_string(dir.join("t.jsonl")).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"a\":1") && lines[1].contains("\"a\":3"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
