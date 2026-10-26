//! Rules: "when X, do Y". A small, safe scripting layer (no code execution): triggers over the game state,
//! actions that press a key, pause or stop the bot, send an alert or change settings. Rules are plain JSON,
//! editable live from the control panel and shareable as text.
//!
//! The engine is pure (state in, effects out) so every rule is unit-tested with fake clocks.

use crate::control::Settings;
use neon_state::GameState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    HpBelow { pct: f64 },
    HpAbove { pct: f64 },
    MpBelow { pct: f64 },
    LevelAtLeast { level: u16 },
    InMap { map: u32 },
    NotInMap { map: u32 },
    /// At least `count` mobs within `within_px`.
    MobsNear { count: u32, within_px: i32 },
    /// A mob-free stretch: nothing within `within_px` for `ms` (the farm spot is empty).
    NoMobsFor { ms: u64, within_px: i32 },
    StrangerNear { within_px: i32 },
    /// Every `ms` milliseconds.
    EveryMs { ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Do {
    Tap { key: String },
    /// Hold the bot still (resume from the panel).
    Pause,
    /// End the run.
    Stop,
    Notify { text: String },
    /// Change settings, e.g. switch the mob list or the map.
    Set { settings: Settings },
}

fn yes() -> bool { true }
fn default_cooldown() -> u64 { 5_000 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub when: Trigger,
    pub then: Vec<Do>,
    /// Minimum gap between two firings.
    #[serde(default = "default_cooldown")]
    pub cooldown_ms: u64,
    /// Fire only when the condition becomes true, not on every tick while it stays true.
    #[serde(default = "yes")]
    pub edge: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Tap(String),
    Pause,
    Stop,
    Notify(String),
    Set(Settings),
}

#[derive(Default)]
struct RuleState {
    was_true: bool,
    last_fired: Option<u64>,
    empty_since: Option<u64>,
}

#[derive(Default)]
pub struct RuleEngine {
    rules: Vec<Rule>,
    st: Vec<RuleState>,
}

impl RuleEngine {
    pub fn new(rules: Vec<Rule>) -> Self {
        let st = rules.iter().map(|_| RuleState::default()).collect();
        Self { rules, st }
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Evaluate every enabled rule against this state.
    pub fn eval(&mut self, s: &GameState) -> Vec<Effect> {
        let now = s.ts_ms;
        let mut out = vec![];
        for (rule, st) in self.rules.iter().zip(self.st.iter_mut()) {
            if !rule.enabled {
                st.was_true = false;
                continue;
            }
            let cond = Self::holds(&rule.when, s, st, now);
            let rising = cond && !st.was_true;
            st.was_true = cond;
            let periodic = matches!(rule.when, Trigger::EveryMs { .. });
            let wanted = cond && (rising || !rule.edge || periodic);
            let cooled = st.last_fired.map_or(true, |t| now.saturating_sub(t) >= rule.cooldown_ms);
            if wanted && cooled {
                st.last_fired = Some(now);
                for a in &rule.then {
                    out.push(match a {
                        Do::Tap { key } => Effect::Tap(key.clone()),
                        Do::Pause => Effect::Pause,
                        Do::Stop => Effect::Stop,
                        Do::Notify { text } => Effect::Notify(format!("rule '{}': {}", rule.name, text)),
                        Do::Set { settings } => Effect::Set(settings.clone()),
                    });
                }
            }
        }
        out
    }

    fn holds(t: &Trigger, s: &GameState, st: &mut RuleState, now: u64) -> bool {
        let p = &s.player;
        match t {
            Trigger::HpBelow { pct } => p.hp_percent().map_or(false, |v| v < *pct),
            Trigger::HpAbove { pct } => p.hp_percent().map_or(false, |v| v > *pct),
            Trigger::MpBelow { pct } => p.mp_percent().map_or(false, |v| v < *pct),
            Trigger::LevelAtLeast { level } => p.level.map_or(false, |l| l >= *level),
            Trigger::InMap { map } => p.map_id == Some(*map),
            Trigger::NotInMap { map } => p.map_id.map_or(false, |m| m != *map),
            Trigger::MobsNear { count, within_px } => {
                s.mobs.iter().filter(|m| (m.x - p.x).abs() <= *within_px && (m.y - p.y).abs() <= *within_px).count() as u32 >= *count
            }
            Trigger::NoMobsFor { ms, within_px } => {
                let any = s.mobs.iter().any(|m| (m.x - p.x).abs() <= *within_px && (m.y - p.y).abs() <= *within_px);
                if any {
                    st.empty_since = None;
                    false
                } else {
                    let since = *st.empty_since.get_or_insert(now);
                    now.saturating_sub(since) >= *ms
                }
            }
            Trigger::StrangerNear { within_px } => s.others.iter().any(|o| (o.x - p.x).abs() <= *within_px && (o.y - p.y).abs() <= *within_px),
            Trigger::EveryMs { .. } => true, // gated by the cooldown (= the period)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neon_state::{Capabilities, Mob, OtherPlayer, Player};

    fn st(ts: u64, hp: u32, mobs: &[(i32, i32)]) -> GameState {
        GameState {
            ts_ms: ts,
            caps: Capabilities { position: true, ..Default::default() },
            player: Player { x: 0, y: 0, map_id: Some(7), level: Some(9), hp: Some(hp), max_hp: Some(100), mp: Some(50), max_mp: Some(100), ..Default::default() },
            mobs: mobs.iter().map(|&(x, y)| Mob { x, y, id: None, hp_percent: None, oid: None, age_ms: None }).collect(),
            ..Default::default()
        }
    }

    fn rule(when: Trigger, then: Vec<Do>) -> Rule {
        Rule { name: "r".into(), enabled: true, when, then, cooldown_ms: 1000, edge: true }
    }

    #[test]
    fn edge_rules_fire_once_when_the_condition_becomes_true() {
        let mut e = RuleEngine::new(vec![rule(Trigger::HpBelow { pct: 50.0 }, vec![Do::Notify { text: "low".into() }])]);
        assert!(e.eval(&st(0, 80, &[])).is_empty());
        assert_eq!(e.eval(&st(100, 40, &[])).len(), 1, "crossed below 50%");
        assert!(e.eval(&st(5000, 30, &[])).is_empty(), "still low: no repeat");
        assert!(e.eval(&st(6000, 90, &[])).is_empty());
        assert_eq!(e.eval(&st(9000, 20, &[])).len(), 1, "fires again after recovering");
    }

    #[test]
    fn level_triggered_rules_repeat_on_their_cooldown() {
        let mut r = rule(Trigger::HpBelow { pct: 50.0 }, vec![Do::Tap { key: "F2".into() }]);
        r.edge = false;
        r.cooldown_ms = 1000;
        let mut e = RuleEngine::new(vec![r]);
        assert_eq!(e.eval(&st(0, 10, &[])), vec![Effect::Tap("F2".into())]);
        assert!(e.eval(&st(500, 10, &[])).is_empty(), "inside the cooldown");
        assert_eq!(e.eval(&st(1100, 10, &[])).len(), 1);
    }

    #[test]
    fn every_ms_is_a_timer() {
        let mut r = rule(Trigger::EveryMs { ms: 2000 }, vec![Do::Tap { key: "F5".into() }]);
        r.cooldown_ms = 2000;
        let mut e = RuleEngine::new(vec![r]);
        let fired = |e: &mut RuleEngine, t| e.eval(&st(t, 100, &[])).len();
        assert_eq!((fired(&mut e, 0), fired(&mut e, 1000), fired(&mut e, 2000), fired(&mut e, 3000), fired(&mut e, 4100)), (1, 0, 1, 0, 1));
    }

    #[test]
    fn map_level_and_crowd_triggers() {
        let mut e = RuleEngine::new(vec![
            rule(Trigger::InMap { map: 7 }, vec![Do::Notify { text: "in 7".into() }]),
            rule(Trigger::NotInMap { map: 7 }, vec![Do::Pause]),
            rule(Trigger::LevelAtLeast { level: 10 }, vec![Do::Stop]),
            rule(Trigger::MobsNear { count: 3, within_px: 100 }, vec![Do::Tap { key: "F6".into() }]),
        ]);
        let none = e.eval(&st(0, 100, &[(50, 0), (60, 0)]));
        assert_eq!(none, vec![Effect::Notify("rule 'r': in 7".into())], "level 9, 2 mobs, in map 7");
        let mut s = st(10_000, 100, &[(50, 0), (60, 0), (70, 10), (900, 0)]);
        s.player.level = Some(10);
        let fx = e.eval(&s);
        assert!(fx.contains(&Effect::Stop) && fx.contains(&Effect::Tap("F6".into())), "{fx:?}");
    }

    #[test]
    fn no_mobs_for_waits_out_the_quiet_period_and_resets_when_one_appears() {
        let mut e = RuleEngine::new(vec![rule(Trigger::NoMobsFor { ms: 3000, within_px: 200 }, vec![Do::Notify { text: "empty".into() }])]);
        assert!(e.eval(&st(0, 100, &[])).is_empty());
        assert!(e.eval(&st(2000, 100, &[])).is_empty());
        assert_eq!(e.eval(&st(3100, 100, &[])).len(), 1);
        assert!(e.eval(&st(3200, 100, &[(10, 0)])).is_empty(), "a mob appears: timer resets");
        assert!(e.eval(&st(5000, 100, &[])).is_empty(), "quiet again, but not for 3 s yet");
    }

    #[test]
    fn stranger_trigger_and_settings_action() {
        let settings: Settings = serde_json::from_str(r#"{"mobs":[2,3]}"#).unwrap();
        let mut e = RuleEngine::new(vec![rule(Trigger::StrangerNear { within_px: 300 }, vec![Do::Set { settings: settings.clone() }])]);
        let mut s = st(0, 100, &[]);
        s.others = vec![OtherPlayer { name: Some("x".into()), x: 100, y: 0 }];
        assert_eq!(e.eval(&s), vec![Effect::Set(settings)]);
    }

    #[test]
    fn disabled_rules_never_fire_and_json_round_trips() {
        let json = r#"[{"name":"buff","when":{"kind":"every_ms","ms":60000},"then":[{"kind":"tap","key":"F5"}],"cooldown_ms":60000},
                      {"name":"off","enabled":false,"when":{"kind":"hp_below","pct":99},"then":[{"kind":"stop"}]}]"#;
        let rules: Vec<Rule> = serde_json::from_str(json).unwrap();
        assert!(rules[0].edge && rules[0].enabled && !rules[1].enabled, "defaults: enabled, edge");
        let again: Vec<Rule> = serde_json::from_str(&serde_json::to_string(&rules).unwrap()).unwrap();
        assert_eq!(again, rules);
        let mut e = RuleEngine::new(rules);
        let fx = e.eval(&st(0, 10, &[]));
        assert_eq!(fx, vec![Effect::Tap("F5".into())], "the disabled hp rule stays silent: {fx:?}");
    }
}
