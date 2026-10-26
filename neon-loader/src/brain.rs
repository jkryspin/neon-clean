//! Decision logic: `GameState` in, `Action`s out.
//!
//! Pure and deterministic. Time comes from `GameState::ts_ms`, never the system
//! clock, so every behaviour is unit-testable by feeding synthetic states.
//! The executor (bot.rs) owns key presses, timing jitter and idle breaks.

use crate::nav::{Cmd, MapGeom, Nav, Portal, Tuning, Vert};
use neon_state::{Facing, GameState, Mob};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Hold this direction (or release all movement keys with `None`).
    Move(Option<Dir>),
    /// Hold Up/Down (ladder or rope), or release with `None`.
    Vert(Option<Vert>),
    /// Hold Down and tap jump: fall through the platform under us.
    DropDown,
    /// Press a direction briefly (ms) and release: fine positioning without overshoot.
    Nudge(Dir, u64),
    /// Click the "revive in town" button of the death dialog.
    Revive,
    /// Press and release a key once.
    Tap(String),
    /// Hold the basic attack key down (the client repeats the swing at the weapon's own speed) or let it go.
    Attack(bool),
    /// Something the user should know (feature disabled, alert, ...).
    Notify(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keymap {
    pub attack: String,
    pub jump: String,
    pub hp_pot: String,
    pub mp_pot: String,
    pub ap_key: String,
    /// Pickup key (MapleStory default: Z).
    pub loot: String,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            attack: "ctrl".into(),
            jump: "alt".into(),
            hp_pot: "F2".into(),
            mp_pot: "F3".into(),
            ap_key: "F1".into(),
            loot: "z".into(),
        }
    }
}

/// A key pressed on a timer: buffs, pet food, anything periodic.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MacroCfg {
    pub label: String,
    pub key: String,
    /// Press at most this often.
    pub every_ms: u64,
    /// Only while at least this much MP is left (percent); 0 = ignore MP.
    pub min_mp_pct: f64,
    /// Only while a mob is within reach (an attack buff), otherwise any time the bot is running.
    pub only_when_fighting: bool,
    pub enabled: bool,
}

/// An attack skill used in place of the basic attack whenever it is off cooldown.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillCfg {
    pub label: String,
    pub key: String,
    pub cooldown_ms: u64,
    pub min_mp_pct: f64,
    /// Only while at least this much HP is left (Slash Blast costs HP); 0 = ignore.
    pub min_hp_pct: f64,
    /// Only with at least this many mobs within `range_x` px ahead (an area attack); 0 = any time.
    pub min_mobs: u32,
    pub range_x: i32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainConfig {
    pub keys: Keymap,
    pub kami: bool,
    pub auto_pot: bool,
    pub auto_ap: bool,
    pub evasion: bool,
    pub loot: bool,

    /// Pot below this percent.
    pub hp_pot_below: f64,
    pub mp_pot_below: f64,
    /// Minimum gap between pots of the same kind (server-side pot cooldown).
    pub pot_cooldown_ms: u64,

    /// Horizontal / vertical reach of the attack, in map pixels.
    pub attack_range_x: i32,
    /// Walk up to this distance from a mob, then stop: close enough that a 64 px punch overlaps it
    /// even if it drifts, far enough not to stand inside it (contact damage).
    pub stand_off_x: i32,
    pub attack_range_y: i32,
    /// One swing of the weapon. The attack key is held, so the client sets the pace; this only counts swings
    /// (and paces rotation skills). Hand Axe, the Beginner's weapon: Fast (4), 720 ms (meowdb).
    pub attack_interval_ms: u64,
    /// A mob this far above us (and roughly aligned in x) needs a jump.
    pub jump_if_above: i32,
    /// Ignore mobs further than this (stale/false-positive reads).
    pub max_target_distance: i32,
    /// No progress for this long while chasing => give up on the target.
    pub stuck_ms: u64,
    pub blacklist_ms: u64,

    /// AP spent per level when the stream can't tell us how much is available.
    pub ap_per_level: u32,
    /// States older than this relative to the newest are considered stale.
    pub max_state_gap_ms: u64,

    pub trusted_igns: HashSet<String>,
    pub evasion_distance: i32,

    /// A drop this close horizontally (and on roughly the same floor) is worth walking to.
    pub loot_range_x: i32,
    pub loot_range_y: i32,
    /// Close enough to press the pickup key.
    pub loot_pickup_dist: i32,
    pub loot_interval_ms: u64,
    /// Give up on a drop after this many pickup presses without it disappearing.
    pub loot_max_tries: u32,
    pub loot_blacklist_ms: u64,

    /// Stop fighting below this HP percent; resume once HP recovers above `resume_hp_above`.
    /// (A level-7 character died to pigs because nothing stopped it.)
    pub flee_hp_below: f64,
    /// A town to walk to and rest in once out of potions (HP below `retreat_hp_below`); back when HP is high again.
    pub safe_map: Option<u32>,
    /// `allowed_mobs` is a preference (a plan's kill step), not a filter: with none of them in sight, fight others.
    #[serde(default)]
    pub prefer_allowed: bool,
    pub resume_hp_above: f64,
    /// Only fight these mob template ids (None = anything, including unidentified mobs).
    /// A level-7 Beginner gets chewed up by pigs and mushrooms; snails are safe.
    pub allowed_mobs: Option<HashSet<u32>>,
    /// Only act in these maps; anywhere else the bot stands still and says so.
    pub allowed_maps: Option<HashSet<u32>>,
    /// The map to grind in. Anywhere else the bot walks back through portals (after a death,
    /// a map change, a restart) instead of standing still.
    pub home_map: Option<u32>,
    /// Stand at this spot of the home map (a quest NPC): walk there and stop. Ignored while fighting.
    pub walk_to: Option<(i32, i32)>,
    /// How close (px, horizontally) `walk_to` has to get before the character stands still.
    pub walk_tol: i32,
    /// While recovering, hang this far above a rope's foot (px).
    pub refuge_height: i32,
    /// Click "revive in town" after a death instead of waiting for a human.
    pub auto_revive: bool,
    /// Never revive more than this many times per hour (a death loop means something is wrong).
    pub max_revives_per_hour: u32,
    /// Stop the whole run after this many deaths (0 = never): repeated deaths mean the setup is wrong.
    pub max_deaths: u32,
    /// Timed key presses (buffs etc.).
    pub macros: Vec<MacroCfg>,
    /// Attack skill rotation; the basic attack is used when none is ready.
    pub rotation: Vec<SkillCfg>,
    /// EXP a kill pays -> mob template, for mobs whose template the network never told us (they were in the map
    /// before the stream started listening). Only amounts unique to one mob (neon-calib/mob-info.json).
    #[serde(skip)]
    pub mob_by_exp: std::collections::HashMap<u64, u32>,
    /// While recovering, walk away from a mob this close (px).
    pub retreat_distance: i32,
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            keys: Keymap::default(),
            kami: true,
            auto_pot: true,
            auto_ap: true,
            evasion: false,
            loot: true,
            hp_pot_below: 50.0,
            mp_pot_below: 30.0,
            pot_cooldown_ms: 1200,
            // meowdb (formulas/targeting): barehanded punch reaches 64 px horizontally and 33 px
            // above the feet; a hit needs the boxes to overlap. 52 leaves room for position error.
            attack_range_x: 60,
            stand_off_x: 50,
            attack_range_y: 28,
            attack_interval_ms: 720,
            mob_by_exp: Default::default(),
            jump_if_above: 50,
            max_target_distance: 600,
            stuck_ms: 2500,
            blacklist_ms: 8000,
            ap_per_level: 5,
            max_state_gap_ms: 2000,
            trusted_igns: HashSet::new(),
            evasion_distance: 300,
            loot_range_x: 500,
            loot_range_y: 40,
            loot_pickup_dist: 30,
            loot_interval_ms: 450,
            loot_max_tries: 6,
            loot_blacklist_ms: 20_000,
            flee_hp_below: 50.0,
            safe_map: None,
            prefer_allowed: false,
            resume_hp_above: 85.0,
            allowed_mobs: None,
            allowed_maps: None,
            home_map: None,
            walk_to: None,
            walk_tol: WALK_TO_TOL,
            refuge_height: 110,
            auto_revive: false,
            max_revives_per_hour: 3,
            max_deaths: 0,
            macros: vec![],
            rotation: vec![],
            retreat_distance: 130,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BrainStats {
    pub attacks: u32,
    pub mobs_killed: u32,
    pub hp_pots: u32,
    pub mp_pots: u32,
    pub levels_gained: u32,
    pub ap_spent: u32,
    pub map_changes: u32,
    pub loot_presses: u32,
    pub drops_gone: u32,
    /// Position reads rejected as physically impossible.
    pub bad_positions: u32,
    pub deaths: u32,
    pub revives: u32,
    /// Kills by mob template id (0 = not identified).
    pub kills_by_mob: std::collections::BTreeMap<u32, u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Running,
    /// Paused because inputs are missing, dead, or a stranger is nearby.
    Paused,
}

pub struct Brain {
    cfg: BrainConfig,
    pub stats: BrainStats,
    pub mode: Mode,
    /// What the last tick decided, for the debug trace ("attack", "align Right 40", ...).
    pub decision: String,
    /// Why the bot is paused (empty while running).
    pub pause_reason: String,

    last_ts: Option<u64>,
    last_map: Option<u32>,
    last_level: Option<u16>,
    facing: Option<Facing>,
    moving: Option<Dir>,
    /// When the current movement key went down, and where the character was.
    move_since: u64,
    move_start_x: i32,
    /// Backing off for a jump's run-up: (direction, x to reach, since).
    retreat: Option<(Dir, i32, u64)>,
    /// The player's x this tick (for the retreat).
    cur_x: i32,
    /// The last raw x reading and when it last changed (readings arrive in ~15 px steps every ~100 ms).
    last_raw_x: i32,
    pos_changed_at: u64,

    last_hp_pot: Option<u64>,
    last_mp_pot: Option<u64>,
    last_attack: Option<u64>,
    last_jump: Option<u64>,
    /// Direction of the jump just taken and when: held while airborne.
    jump_hold: Option<(Dir, u64)>,
    last_loot: Option<u64>,
    loot_tries: std::collections::HashMap<(i32, i32), u32>,
    loot_blacklist: Vec<((i32, i32), u64)>,
    prev_drop_count: usize,

    fleeing: bool,
    pos_ok: Option<(i32, i32, u64)>,
    pos_pending: Option<(i32, i32, u32)>,
    nav: Option<Nav>,
    nav_map: Option<u32>,
    map_dir: Option<std::path::PathBuf>,
    held_vert: Option<(Vert, u64)>,
    last_drop: Option<u64>,
    last_nudge: Option<u64>,
    last_portal: Option<u64>,
    macro_last: std::collections::HashMap<usize, u64>,
    skill_last: std::collections::HashMap<usize, u64>,
    manual_pause: bool,
    schedule_pause: Option<String>,
    dead_since: Option<u64>,
    /// Mobs (server id -> where they stood) that took 14 swings without dying: ignored until they move.
    phantoms: std::collections::HashMap<u32, (i32, i32)>,
    /// Clearing the way while travelling: the mob being fought (server id or position) and swings at it, when the
    /// fight started and where we stood, and until when blockers are walked past (mobs respawning in the way).
    path_mob: Option<((i32, i32, u32), u32)>,
    /// Template of `path_mob` and the map it was in, to count it by kind when it dies (a map change empties the
    /// mob list: that is not a kill).
    path_mob_id: Option<u32>,
    path_mob_map: Option<u32>,
    /// Mobs that vanished after we hit them (when, template) and EXP changes (when), matched within KILL_EXP_MS:
    /// only a vanish with our EXP going up is our kill (on a busy map other players finish our mobs).
    pending_kills: Vec<(u64, u32)>,
    /// No other player in sight (we control the map's mobs; see `stale`).
    alone: bool,
    /// Spawn points of the current map (template, x, y), and when each was last visited while looking for a kind
    /// of mob that is nowhere in sight.
    spawns: Vec<(u32, i32, i32)>,
    spawn_visited: std::collections::HashMap<(i32, i32), u64>,
    /// Resting out of potions on a platform no mob spawns on: (segment, x, y); and until when not to try that again.
    safe_rest: Option<(usize, i32, i32)>,
    safe_rest_banned_until: u64,
    /// The last tick walked toward a spawn point (so a jump it started is carried through in the air).
    seeking: bool,
    /// The attack key is down / something this tick wants it down (it is let go on any tick that does not).
    attack_held: bool,
    attack_wanted: bool,
    /// When our EXP changed and by how much.
    exp_events: Vec<(u64, i64)>,
    last_exp: Option<u64>,
    path_fight: Option<(u64, i32)>,
    path_ignore_until: u64,
    /// The last HP potion press (time, HP then), checked once POT_CHECK_MS later; consecutive presses that did not heal.
    pot_check: Option<(u64, Option<u32>)>,
    pot_misses: u32,
    /// The current map spawns no mobs (a town, a shop): nothing to recover from there.
    town: bool,
    /// Set by the executor: the HP potion cell was found empty.
    pub pots_empty: bool,
    /// Walking to / resting in `cfg.safe_map` because we ran out of potions.
    town_retreat: bool,
    /// Since when the character has stood (grounded) where the map's data has no floor.
    off_floor_since: Option<u64>,
    revive_times: Vec<u64>,
    /// The rope chosen to recover on; kept until we are done so two ropes never trade places.
    refuge: Option<usize>,
    portals: std::collections::HashMap<u32, Vec<Portal>>,
    /// The highest level of any mob that spawns in each map (0 = none), for routing around dangerous maps.
    map_danger: std::collections::HashMap<u32, u32>,
    /// The last route warning given, (from, to), so it is said once per leg.
    route_warned: Option<(u32, u32)>,
    last_release: Option<u64>,
    footholds: Vec<neon_state::Foothold>,
    target: Option<Target>,
    peer_claims: std::collections::HashSet<u32>,
    blacklist: Vec<((i32, i32), u64)>,
    last_pos: Option<((i32, i32), u64)>,
    warned: HashSet<&'static str>,
}

#[derive(Debug, Clone)]
struct Target {
    x: i32,
    y: i32,
    since: u64,
    best_dist: i32,
    best_at: u64,
    attacked: bool,
    /// Platform we were last standing on, and jumps/drops tried from it.
    last_seg: Option<usize>,
    maneuvers: u32,
    id: Option<u32>,
    /// Attacks thrown at this target; a long run without a kill means it is not where we think.
    swings: u32,
    /// Server object id (network source): the target is the same mob exactly while this id is present.
    oid: Option<u32>,
}

impl Brain {
    pub fn new(cfg: BrainConfig) -> Self {
        Self {
            cfg,
            stats: BrainStats::default(),
            mode: Mode::Running,
            decision: String::new(),
            pause_reason: String::new(),
            last_ts: None,
            last_map: None,
            last_level: None,
            facing: None,
            moving: None,
            move_since: 0,
            move_start_x: 0,
            retreat: None,
            cur_x: 0,
            last_raw_x: i32::MIN,
            pos_changed_at: 0,
            last_hp_pot: None,
            last_mp_pot: None,
            last_attack: None,
            last_jump: None,
            jump_hold: None,
            last_loot: None,
            loot_tries: Default::default(),
            loot_blacklist: vec![],
            prev_drop_count: 0,
            fleeing: false,
            pos_ok: None,
            pos_pending: None,
            nav: None,
            nav_map: None,
            map_dir: None,
            held_vert: None,
            last_drop: None,
            last_nudge: None,
            last_portal: None,
            macro_last: Default::default(),
            skill_last: Default::default(),
            manual_pause: false,
            schedule_pause: None,
            dead_since: None,
            off_floor_since: None,
            pots_empty: false,
            town: false,
            pot_check: None,
            pot_misses: 0,
            phantoms: Default::default(),
            path_mob: None,
            path_mob_id: None,
            path_mob_map: None,
            pending_kills: vec![],
            alone: false,
            spawns: vec![],
            spawn_visited: Default::default(),
            safe_rest: None,
            safe_rest_banned_until: 0,
            seeking: false,
            attack_held: false,
            attack_wanted: false,
            exp_events: vec![],
            last_exp: None,
            path_fight: None,
            path_ignore_until: 0,
            town_retreat: false,
            revive_times: vec![],
            refuge: None,
            portals: Default::default(),
            map_danger: Default::default(),
            route_warned: None,
            last_release: None,
            footholds: vec![],
            target: None,
            peer_claims: Default::default(),
            blacklist: vec![],
            last_pos: None,
            warned: HashSet::new(),
        }
    }

    /// Use this geometry for navigation on `map` (instead of loading it from disk).
    pub fn with_geometry(mut self, map: u32, geom: &MapGeom) -> Self {
        self.nav = Some(Nav::new(geom, Tuning::default()));
        self.nav_map = Some(map);
        self.spawns = geom.mob_spawns.iter().map(|p| (p.id, p.x, p.y)).collect();
        self
    }

    /// Load map geometry (platforms, ladders) from `<dir>/<map id>.json` for navigation.
    pub fn with_maps(mut self, dir: std::path::PathBuf) -> Self {
        // Mob levels (neon-calib/mob-info.json next to the maps) rate each map's danger for routing.
        let levels: std::collections::HashMap<String, u32> = std::fs::read_to_string(dir.join("..").join("mob-info.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("mobs").and_then(|m| m.as_object()).map(|m| m.iter().filter_map(|(id, i)| Some((id.clone(), i.get("level")?.as_u64()? as u32))).collect()))
            .unwrap_or_default();
        // Index every map's usable portals (2 regular, 1 hidden: both are taken with Up) so routes between maps can be
        // planned; Tree Dungeon and the Southern Forest dungeons are entered through hidden ones.
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let Some(id) = e.path().file_stem().and_then(|n| n.to_str()).and_then(|n| n.parse::<u32>().ok()) else { continue };
                if let Some(g) = MapGeom::load(&dir, id) {
                    self.map_danger.insert(id, g.mobs.keys().filter_map(|m| levels.get(m)).copied().max().unwrap_or(0));
                    self.portals.insert(id, g.portals.into_iter().filter(|p| matches!(p.kind, Some(1 | 2)) && p.to_map.is_some_and(|m| m != 999_999_999)).collect());
                }
            }
        }
        self.map_dir = Some(dir);
        self
    }

    /// Rate a map's danger (its strongest mob's level) directly (tests).
    pub fn with_danger(mut self, map: u32, level: u32) -> Self {
        self.map_danger.insert(map, level);
        self
    }

    /// Register portals directly (tests).
    pub fn with_portals(mut self, map: u32, portals: Vec<Portal>) -> Self {
        self.portals.insert(map, portals);
        self
    }

    /// The strongest mob on the safest route from `from` to `to`, if there is a route.
    pub fn route_danger(&self, from: u32, to: u32) -> Option<u32> {
        self.route(from, to).map(|(_, d)| d)
    }

    /// The portal to take on `from` to get closer to `to`.
    fn next_portal(&self, from: u32, to: u32) -> Option<Portal> {
        self.route(from, to).map(|(p, _)| p)
    }

    /// The safest route from `from` to `to`: the one whose strongest mob on the way (maps passed through, not the two
    /// ends) is weakest, then the one through the fewest hidden portals, then the one with the fewest maps. Returns
    /// the first portal and that strongest mob level. A route by map count alone took a level-10 character from Perion
    /// to Henesys through Deep Valley (Fire Boars, level 32) and the Henesys Dungeon Entrance (Lupins, 37).
    fn route(&self, from: u32, to: u32) -> Option<(Portal, u32)> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        // (danger, hidden portals, maps, map, first portal index on `from`)
        let mut heap = BinaryHeap::from([Reverse((0u32, 0u32, 0u32, from, usize::MAX))]);
        let mut done: HashSet<u32> = HashSet::new();
        while let Some(Reverse((danger, hidden, hops, m, first))) = heap.pop() {
            if m == to {
                let p = self.portals.get(&from)?.get(first)?.clone();
                return Some((p, danger));
            }
            if !done.insert(m) {
                continue;
            }
            for (i, p) in self.portals.get(&m).into_iter().flatten().enumerate() {
                let Some(t) = p.to_map else { continue };
                if done.contains(&t) {
                    continue;
                }
                let d = if t == to { danger } else { danger.max(self.map_danger.get(&t).copied().unwrap_or(0)) };
                let h = hidden + u32::from(p.kind == Some(1));
                heap.push(Reverse((d, h, hops + 1, t, if m == from { i } else { first })));
            }
        }
        None
    }

    /// Walk back to the home map through portals. True when it took control this tick.
    fn travel(&mut self, s: &GameState, out: &mut Vec<Action>) -> bool {
        let target = self.cfg.home_map;
        self.travel_to(s, target, out)
    }

    fn travel_to(&mut self, s: &GameState, target: Option<u32>, out: &mut Vec<Action>) -> bool {
        let (Some(home), Some(map)) = (target, s.player.map_id) else { return false };
        if map == home {
            return false;
        }
        let now = s.ts_ms;
        let Some((portal, danger)) = self.route(map, home) else {
            self.warn_once(out, "route", &format!("no portal route from map {map} to {home}"));
            return false;
        };
        if self.route_warned != Some((map, home)) {
            self.route_warned = Some((map, home));
            if let Some(level) = s.player.level.filter(|l| danger > u32::from(*l) + ROUTE_DANGER_MARGIN) {
                out.push(Action::Notify(format!(
                    "the safest route from map {map} to {home} passes level-{danger} mobs (we are level {level}): no safer one exists"
                )));
            }
        }
        let Some(nav) = &self.nav else {
            self.warn_once(out, "travel-nav", &format!("map {map}: no geometry, cannot walk to its portal"));
            return false;
        };
        let (px, py) = (self.nav_x(s), s.player.y);
        let grounded = s.player.grounded.unwrap_or(true);
        let (tx, ty) = (portal.x.unwrap_or(px), portal.y.unwrap_or(py));
        let cmd = match nav.locate_within(tx, ty, 50) {
            Some(goal) => nav.step(px, py, grounded, goal, tx, 6),
            None => Cmd::NoRoute,
        };
        // Clear the way: a mob in reach ahead of where we are walking (or touching us) is fought, not walked into.
        if cmd == Cmd::NoRoute {
            self.warn_once(out, "travel-noroute", &format!("no route to the portal toward map {home} from ({}, {}): defending in place", s.player.x, s.player.y));
        }
        let heading = match cmd {
            Cmd::Walk(d) | Cmd::Align(d, _) | Cmd::Retreat(d, _) | Cmd::RunJump(d, _) | Cmd::Jump(Some(d)) => Some(d),
            _ => None,
        };
        if grounded && self.clear_path(s, heading, out) {
            return true;
        }
        if cmd == Cmd::Arrived {
            self.set_move(None, out);
            if ready(self.last_portal, now, 1500) {
                out.push(Action::Tap("Up".into()));
                self.last_portal = Some(now);
                out.push(Action::Notify(format!("taking portal {} to map {}", portal.name.clone().unwrap_or_default(), portal.to_map.unwrap_or(0))));
            }
            self.decision = "travel: at portal".into();
        } else {
            self.apply_cmd(cmd, now, grounded, false, out);
            self.decision = format!("travel {:?}", cmd);
        }
        true
    }

    /// While travelling: attack the nearest mob that is in reach ahead of us (or touching us from behind) instead of
    /// running through it. True while fighting.
    fn clear_path(&mut self, s: &GameState, heading: Option<Dir>, out: &mut Vec<Action>) -> bool {
        let now = s.ts_ms;
        self.track_facing(s);
        let (px, py) = (s.player.x, s.player.y);
        let (rx, ry) = (self.cfg.attack_range_x, self.cfg.attack_range_y);
        // The mob we were hitting on the way is gone from the server's list: a kill, counted like one in a fight.
        if let Some(((_, _, oid), n)) = self.path_mob {
            if oid != 0 && n > 0 && !s.mobs.iter().any(|m| m.oid == Some(oid)) {
                if s.player.map_id == self.path_mob_map {
                    self.note_kill(s, self.path_mob_id);
                }
                self.path_mob = None;
            }
        }
        if now < self.path_ignore_until && heading.is_some() {
            return false;
        }
        let blocker = s
            .mobs
            .iter()
            .filter(|m| (m.y - py).abs() <= ry)
            .filter(|m| !self.ignored_mob(m))
            .filter(|m| {
                let dx = m.x - px;
                let ahead = match heading {
                    Some(Dir::Right) => dx >= -8 && dx <= rx + PATH_LOOKAHEAD,
                    Some(Dir::Left) => dx <= 8 && dx >= -(rx + PATH_LOOKAHEAD),
                    None => false,
                };
                // Not walking anywhere (no route): defend against anything in reach rather than stand and take hits
                // (a character with no route out of L Forest II died standing still).
                ahead || dx.abs() <= if heading.is_none() { rx } else { rx / 2 }
            })
            .min_by_key(|m| (m.x - px).abs())
            .cloned();
        let Some(m) = blocker else {
            self.path_fight = None;
            return false;
        };
        // Mobs that keep respawning in the way (Maple Island's tutorial sentinels) would hold us forever: after
        // PATH_FIGHT_MS of fighting without getting anywhere, walk on past them for a while.
        let (since, at_x) = *self.path_fight.get_or_insert((now, px));
        if (px - at_x).abs() > 2 * rx {
            self.path_fight = Some((now, px));
        } else if heading.is_some() && now.saturating_sub(since) > PATH_FIGHT_MS {
            self.path_fight = None;
            self.path_ignore_until = now + PATH_IGNORE_MS;
            out.push(Action::Notify(format!("fought in the way for {} s without getting anywhere: walking on", PATH_FIGHT_MS / 1000)));
            return false;
        }
        let dx = m.x - px;
        // Not yet in reach (it is ahead): stop and let it come, swinging only once it is close.
        let want = if dx >= 0 { Facing::Right } else { Facing::Left };
        if self.facing != Some(want) && dx.abs() > 8 {
            self.set_move(Some(if dx >= 0 { Dir::Right } else { Dir::Left }), out);
            self.decision = "travel: turning to a mob in the way".into();
            return true;
        }
        if dx.abs() > rx {
            // Ahead but out of reach: step toward it.
            self.set_move(Some(if dx >= 0 { Dir::Right } else { Dir::Left }), out);
            self.decision = "travel: closing on a mob in the way".into();
            return true;
        }
        self.stop_moving(out);
        if ready(self.last_attack, now, self.cfg.attack_interval_ms) {
            // The same swing cap as a normal fight: a mob that takes this many without dying is not where we think.
            // (Checked before the swing that would be one too many.)
            let key = (m.x, m.y, m.oid.unwrap_or(0));
            let same = |(x, y, o): (i32, i32, u32)| if o != 0 { o == key.2 } else { dist(x, y, key.0, key.1) <= 20 };
            let n = match self.path_mob {
                Some((k, n)) if same(k) => n + 1,
                _ => 1,
            };
            self.path_mob = Some((key, n));
            self.path_mob_id = m.id;
            self.path_mob_map = s.player.map_id;
            if n > PHANTOM_SWINGS {
                if let Some(oid) = m.oid {
                    self.phantoms.insert(oid, (m.x, m.y));
                }
                self.path_mob = None;
                out.push(Action::Notify(format!("{PHANTOM_SWINGS} swings without a kill at ({}, {}) while travelling; skipping it", m.x, m.y)));
                return false;
            }
        }
        self.swing(s, out);
        self.decision = "travel: fighting a mob in the way".into();
        true
    }

    /// A mob we hit has vanished. Without an EXP reading it counts at once; with one it waits for our EXP to rise.
    fn note_kill(&mut self, s: &GameState, id: Option<u32>) {
        if s.player.exp.is_none() {
            self.count_kill(id.unwrap_or(0));
        } else {
            self.pending_kills.push((s.ts_ms, id.unwrap_or(0)));
        }
    }

    fn count_kill(&mut self, id: u32) {
        self.stats.mobs_killed += 1;
        *self.stats.kills_by_mob.entry(id).or_insert(0) += 1;
    }

    /// Pair vanished mobs with EXP changes (either may come first); unpaired ones expire after KILL_EXP_MS.
    fn settle_kills(&mut self, s: &GameState) {
        let now = s.ts_ms;
        if let Some(e) = s.player.exp {
            if let Some(l) = self.last_exp.filter(|l| *l != e) {
                self.exp_events.push((now, e as i64 - l as i64));
            }
            self.last_exp = Some(e);
        }
        while let (Some(&(tk, id)), Some(&(te, gain))) = (self.pending_kills.first(), self.exp_events.first()) {
            if tk.abs_diff(te) <= KILL_EXP_MS {
                self.pending_kills.remove(0);
                self.exp_events.remove(0);
                // A mob of unknown kind (0) is named by the EXP it paid: a kill step for Orange Mushrooms waited for
                // ever while every mushroom in the map was "mob 0".
                let id = if id == 0 && gain > 0 { self.cfg.mob_by_exp.get(&(gain as u64)).copied().unwrap_or(0) } else { id };
                self.count_kill(id);
            } else if tk < te {
                self.pending_kills.remove(0);
            } else {
                self.exp_events.remove(0);
            }
        }
        self.pending_kills.retain(|(t, _)| now.saturating_sub(*t) <= KILL_EXP_MS);
        self.exp_events.retain(|(t, _)| now.saturating_sub(*t) <= KILL_EXP_MS);
    }

    /// Out of potions and hurt: rest on the nearest platform of this map that no mob spawns on, instead of walking
    /// to town. True while it holds control. Gives up (town then) if the map has none or a mob comes close.
    fn rest_here(&mut self, s: &GameState, hp: f64, out: &mut Vec<Action>) -> bool {
        let now = s.ts_ms;
        if self.safe_rest.is_none() {
            if !(self.pots_empty && hp < RETREAT_HP_BELOW) || self.town || now < self.safe_rest_banned_until {
                return false;
            }
            let Some(nav) = &self.nav else { return false };
            let grounded = s.player.grounded.unwrap_or(true);
            let Some((seg, x0)) = nav.anchor(s.player.x, s.player.y, grounded) else { return false };
            let spawns: Vec<(i32, i32)> = self.spawns.iter().map(|&(_, x, y)| (x, y)).collect();
            let Some((goal, gx, gy)) = nav.safe_spot(seg, x0, &spawns, SAFE_REST_MIN_W) else { return false };
            self.safe_rest = Some((goal, gx, gy));
            self.target = None;
            out.push(Action::Notify(format!("out of potions at HP {hp:.0}%: resting on a safe platform at ({gx}, {gy})")));
        }
        let Some((goal, gx, gy)) = self.safe_rest else { return false };
        // A mob near the spot: this platform is not safe after all (a flying mob, or one knocked onto it).
        if s.mobs.iter().any(|m| !self.ignored_mob(m) && (m.x - gx).abs() < SAFE_REST_CLEAR_PX && (m.y - gy).abs() < 60) {
            self.safe_rest = None;
            self.safe_rest_banned_until = now + 60_000;
            out.push(Action::Notify(format!("a mob came to the safe platform at ({gx}, {gy}): resting in town instead")));
            return false;
        }
        if hp >= RETREAT_RESUME_HP {
            // Still out of potions: only buying some (the bot clears it after a shop trip) changes that. Clearing it
            // here made the next fight find out again at the potion threshold, and the retreat started at 20-33% HP.
            self.safe_rest = None;
            out.push(Action::Notify(format!("rested to HP {hp:.0}% on a safe platform: going back")));
            return false;
        }
        let Some(nav) = &self.nav else { return false };
        let grounded = s.player.grounded.unwrap_or(true);
        let (px, py) = (self.nav_x(s), s.player.y);
        let cmd = nav.step(px, py, grounded, goal, gx, 20);
        if cmd == Cmd::Arrived {
            self.stop_moving(out);
            self.decision = format!("resting on a safe platform at ({gx}, {gy})");
            return true;
        }
        if cmd == Cmd::NoRoute {
            self.safe_rest = None;
            self.safe_rest_banned_until = now + 60_000;
            return false;
        }
        let heading = match cmd {
            Cmd::Walk(d) | Cmd::Align(d, _) | Cmd::Retreat(d, _) | Cmd::RunJump(d, _) | Cmd::Jump(Some(d)) => Some(d),
            _ => None,
        };
        if grounded && self.clear_path(s, heading, out) {
            return true;
        }
        self.apply_cmd(cmd, now, grounded, false, out);
        self.decision = format!("to a safe platform at ({gx}, {gy}): {cmd:?}");
        true
    }

    /// A mob the current kill step wants. One of unknown kind (it was in the map before the stream started) counts if
    /// it is near a spawn point of a wanted kind: mobs stay near where they spawn.
    fn wanted(&self, m: &Mob) -> bool {
        let Some(ok) = self.cfg.allowed_mobs.as_ref() else { return true };
        match m.id.filter(|id| *id != 0) {
            Some(id) => ok.contains(&id),
            None => self.spawns.iter().any(|&(id, x, y)| ok.contains(&id) && (m.x - x).abs() <= SPAWN_NEAR_PX && (m.y - y).abs() <= 60),
        }
    }

    /// Walk to the nearest spawn point of a wanted mob not visited in the last SPAWN_REVISIT_MS. False when there is
    /// none (no spawn data, or all visited lately).
    fn seek_spawn(&mut self, s: &GameState, out: &mut Vec<Action>) -> bool {
        let now = s.ts_ms;
        let Some(ok) = self.cfg.allowed_mobs.clone() else { return false };
        let (px, py) = (self.nav_x(s), s.player.y);
        self.spawn_visited.retain(|_, t| now.saturating_sub(*t) < SPAWN_REVISIT_MS);
        let Some(&(_, sx, sy)) = self
            .spawns
            .iter()
            .filter(|(id, x, y)| ok.contains(id) && !self.spawn_visited.contains_key(&(*x, *y)))
            .min_by_key(|(_, x, y)| dist(px, py, *x, *y))
        else {
            return false;
        };
        let Some(nav) = &self.nav else { return false };
        let grounded = s.player.grounded.unwrap_or(true);
        let cmd = match nav.locate_within(sx, sy, 60) {
            Some(goal) => nav.step(px, py, grounded, goal, sx, 30),
            None => Cmd::NoRoute,
        };
        if matches!(cmd, Cmd::Arrived | Cmd::NoRoute) {
            self.spawn_visited.insert((sx, sy), now);
            return true;
        }
        let heading = match cmd {
            Cmd::Walk(d) | Cmd::Align(d, _) | Cmd::Retreat(d, _) | Cmd::RunJump(d, _) | Cmd::Jump(Some(d)) => Some(d),
            _ => None,
        };
        if grounded && self.clear_path(s, heading, out) {
            return true;
        }
        self.apply_cmd(cmd, now, grounded, false, out);
        self.decision = format!("to a spawn point of the wanted mobs at ({sx}, {sy}): {cmd:?}");
        true
    }

    /// A mob the network has not mentioned for a while: its removal was missed. Not when nobody else is in the map:
    /// then our own client moves every mob there (it "controls" them) and the server never sends us their moves, so
    /// every live mob looks old (jib01 stood in an empty map for minutes with 17 real mobs "stale"). Their last
    /// position is near where they spawned; the swing cap skips one that really is gone.
    fn stale(&self, m: &Mob) -> bool {
        !self.alone && m.age_ms.map_or(false, |a| a as u64 > STALE_MOB_MS)
    }

    /// A mob not to fight: the network has not mentioned it for a while (its removal was missed), or it took a full
    /// run of swings without dying and has not moved since.
    fn ignored_mob(&self, m: &Mob) -> bool {
        self.stale(m)
            || m.oid.and_then(|o| self.phantoms.get(&o)).map_or(false, |&(x, y)| dist(x, y, m.x, m.y) <= 20)
    }

    /// Walk to `cfg.walk_to` in the current map and stand still there. True while it holds control (not yet arrived).
    fn walk_to(&mut self, s: &GameState, out: &mut Vec<Action>) -> bool {
        let Some((tx, ty)) = self.cfg.walk_to else { return false };
        if self.cfg.kami {
            return false;
        }
        let Some(nav) = &self.nav else { return false };
        let (px, py) = (self.nav_x(s), s.player.y);
        let grounded = s.player.grounded.unwrap_or(true);
        let from = if grounded { nav.locate(px, py) } else { None };
        let goal = from.and_then(|f| nav.stand_near(f, px, tx, ty, 60)).or_else(|| nav.locate_within(tx, ty, 60));
        // An NPC can be clicked from anywhere on screen, so being close on its floor is enough; chasing the exact
        // pixel made the character shuffle left and right in front of it.
        let cmd = match goal {
            Some(goal) => nav.step(px, py, grounded, goal, tx, self.cfg.walk_tol),
            None => Cmd::NoRoute,
        };
        if cmd == Cmd::Arrived || cmd == Cmd::NoRoute {
            if cmd == Cmd::NoRoute {
                self.warn_once(out, "walk-route", &format!("no route to ({tx}, {ty}) in this map"));
            }
            self.set_move(None, out);
            self.decision = "standing at the target".into();
            return false;
        }
        self.apply_cmd(cmd, s.ts_ms, grounded, false, out);
        self.decision = format!("walk to ({tx},{ty}) {:?}", cmd);
        true
    }

    /// The player's x for navigation decisions: while walking on the ground the reading is up to one update interval
    /// old (readings come in ~15 px steps every ~100 ms), so advance it by how long ago it last changed.
    fn nav_x(&self, s: &GameState) -> i32 {
        match self.moving {
            Some(d) if s.player.grounded != Some(false) => {
                let dt = s.ts_ms.saturating_sub(self.pos_changed_at).min(DEAD_RECKON_MAX_MS) as f64 / 1000.0;
                let dx = (WALK_SPEED * dt).round() as i32;
                s.player.x + if d == Dir::Right { dx } else { -dx }
            }
            _ => s.player.x,
        }
    }

    /// True when pausing for an idle break cannot get the character hurt: standing on the ground,
    /// healthy, nothing nearby and not mid-climb.
    pub fn safe_to_idle(&self, s: &GameState) -> bool {
        if self.fleeing || self.held_vert.is_some() || s.player.grounded == Some(false) {
            return false;
        }
        if s.player.hp_percent().map_or(true, |hp| hp < self.cfg.resume_hp_above) {
            return false;
        }
        !s.mobs.iter().any(|m| dist(s.player.x, s.player.y, m.x, m.y) < 350)
    }

    /// Latest map geometry the stream has sent (empty until a sweep completes).
    pub fn footholds(&self) -> &[neon_state::Foothold] {
        &self.footholds
    }

    pub fn config(&self) -> &BrainConfig {
        &self.cfg
    }

    /// The server object id of the mob we are after, if known.
    pub fn target_oid(&self) -> Option<u32> {
        self.target.as_ref().and_then(|t| t.oid)
    }

    /// Mobs other bots in the party are after: never picked as a target.
    pub fn set_peer_claims(&mut self, oids: std::collections::HashSet<u32>) {
        self.peer_claims = oids;
    }

    /// The current target (x, y), if any.
    pub fn target_pos(&self) -> Option<(i32, i32)> {
        self.target.as_ref().map(|t| (t.x, t.y))
    }

    /// Pause/resume from outside (the web UI's button).
    pub fn set_manual_pause(&mut self, paused: bool) {
        self.manual_pause = paused;
    }

    /// Hold the bot still because the schedule says so (outside a window, plan finished).
    pub fn set_schedule_pause(&mut self, reason: Option<String>) {
        self.schedule_pause = reason;
    }

    pub fn config_mut(&mut self) -> &mut BrainConfig {
        &mut self.cfg
    }

    /// Frames whose position was rejected as physically impossible.
    pub fn rejected_positions(&self) -> u32 {
        self.stats.bad_positions
    }

    /// Reject position reads that cannot be real. Walking is ~150 px/s and falling ~800 px/s, so a
    /// jump far beyond that in the time between frames is a bad read, not movement. A bad frame
    /// gets the last good position; if the "new" place repeats three frames running (a real
    /// teleport, portal or knock-back) it is accepted. Returns a corrected copy only when needed.
    fn gate_position(&mut self, s: &GameState) -> Option<GameState> {
        let (x, y) = (s.player.x, s.player.y);
        let Some((lx, ly, lt)) = self.pos_ok else {
            self.pos_ok = Some((x, y, s.ts_ms));
            return None;
        };
        let dt = s.ts_ms.saturating_sub(lt) as i64;
        let max_dx = 300 * dt / 1000 + 90;
        let max_dy = 1000 * dt / 1000 + 90;
        let plausible = ((x - lx) as i64).abs() <= max_dx && ((y - ly) as i64).abs() <= max_dy;
        if plausible {
            self.pos_ok = Some((x, y, s.ts_ms));
            self.pos_pending = None;
            return None;
        }
        // Not plausible: is it the same new place as the last rejected frame?
        let repeats = match self.pos_pending {
            Some((px, py, n)) if (px - x).abs() <= 25 && (py - y).abs() <= 25 => n + 1,
            _ => 1,
        };
        if repeats >= 3 {
            self.pos_ok = Some((x, y, s.ts_ms));
            self.pos_pending = None;
            return None;
        }
        self.pos_pending = Some((x, y, repeats));
        self.stats.bad_positions += 1;
        let mut fixed = s.clone();
        fixed.player.x = lx;
        fixed.player.y = ly;
        Some(fixed)
    }

    /// Decide what to do for this state.
    pub fn tick(&mut self, s: &GameState) -> Vec<Action> {
        self.attack_wanted = false;
        self.alone = s.caps.players && s.others.is_empty();
        let mut out = self.tick_inner(s);
        if self.attack_held && !self.attack_wanted {
            self.attack_held = false;
            out.push(Action::Attack(false));
        }
        self.jump_at_edge(s, &mut out);
        out
    }

    /// Never walk off the end of a platform: about to, jump instead (a run off the edge of one of Maple Island's small
    /// stair platforms dropped the character back down the staircase, again and again).
    fn jump_at_edge(&mut self, s: &GameState, out: &mut Vec<Action>) {
        let (Some(d), Some(nav)) = (self.moving, self.nav.as_ref()) else { return };
        if s.player.grounded != Some(true) || self.mode == Mode::Paused {
            return;
        }
        if nav.edge_ahead(s.player.x, s.player.y, d).map_or(false, |left| left <= EDGE_JUMP_PX) {
            out.push(Action::Tap(self.cfg.keys.jump.clone()));
            self.last_jump = Some(s.ts_ms);
            self.jump_hold = Some((d, s.ts_ms));
            if self.decision.is_empty() {
                self.decision = "at the edge: jump".into();
            } else {
                self.decision.push_str(" + jump at the edge");
            }
        }
    }

    /// Attack the mob in reach: hold the basic attack (the client swings at the weapon's speed, faster than any
    /// tapping: taps every 400 ms against a 720 ms swing landed every other one), or tap a rotation skill that is due.
    /// Returns true when a new swing started (for swing counts).
    fn swing(&mut self, s: &GameState, out: &mut Vec<Action>) -> bool {
        let now = s.ts_ms;
        let due = ready(self.last_attack, now, self.cfg.attack_interval_ms);
        if due {
            let key = self.attack_key(s);
            self.last_attack = Some(now);
            self.stats.attacks += 1;
            if key != self.cfg.keys.attack {
                if self.attack_held {
                    self.attack_held = false;
                    out.push(Action::Attack(false));
                }
                out.push(Action::Tap(key));
                return true;
            }
        }
        self.attack_wanted = true;
        if !self.attack_held {
            self.attack_held = true;
            out.push(Action::Attack(true));
        }
        due
    }

    fn tick_inner(&mut self, s: &GameState) -> Vec<Action> {
        self.decision.clear();
        let corrected = if s.caps.position { self.gate_position(s) } else { None };
        let s = corrected.as_ref().unwrap_or(s);
        // The grounded flag in memory flickers to true at the top of a jump arc. For the length of a jump after take-off
        // the character is airborne whatever it says: believing it made the nav plan the next jump and press the opposite
        // direction in mid-air, which flipped the arc back off the platform it was landing on.
        let locked;
        let s = if self.last_jump.map_or(false, |t| s.ts_ms.saturating_sub(t) < AIR_LOCK_MS) && s.player.grounded != Some(false) {
            let mut c = s.clone();
            c.player.grounded = Some(false);
            locked = c;
            &locked
        } else {
            s
        };
        self.settle_kills(s);
        if s.player.x != self.last_raw_x {
            self.last_raw_x = s.player.x;
            self.pos_changed_at = s.ts_ms;
        }
        self.cur_x = s.player.x;
        let now = s.ts_ms;
        let mut out = vec![];

        // --- Input sanity: never act on data we can't trust -------------
        if !s.caps.position {
            // Without a position nothing can be navigated, but a character standing in a mob map still has to
            // drink: a level-8 character died idle here while the stream had lost its position.
            let mut out = self.pause(if s.alerts.connection_stalled {
                "paused: no data from the game server for 15 s (disconnected?)"
            } else {
                "paused: player position unavailable"
            });
            if s.player.dead != Some(true) && self.cfg.auto_pot && s.caps.hp_mp {
                self.handle_pots(s, &mut out);
            }
            return out;
        }
        if let Some(prev) = self.last_ts {
            if now.saturating_sub(prev) > self.cfg.max_state_gap_ms {
                // Stream stalled; forget transient tracking so we don't act on it.
                self.reset_tracking();
                self.pause_quiet(&mut out);
                self.last_ts = Some(now);
                return out; // resume next tick with fresh data
            }
        }
        self.last_ts = Some(now);

        if s.player.dead == Some(true) {
            return self.handle_dead(s);
        }
        self.dead_since = None;
        // A silent server alone is no reason to stop: a quiet map (a shop, an empty town corner) sends nothing for a long
        // time. A real disconnect also drops the client to the login screen, where the position disappears (above).
        if s.alerts.lie_detector {
            // An anti-macro check is up: never act on it, never touch the game; a human has to answer it.
            return self.pause("paused: lie detector active - answer it by hand");
        }
        if self.manual_pause {
            return self.pause("paused from the control panel");
        }
        if let Some(why) = self.schedule_pause.clone() {
            return self.pause(&format!("paused by schedule: {why}"));
        }

        // --- Map change: everything positional is invalid ---------------
        if let Some(map) = s.player.map_id {
            if self.last_map.is_some() && self.last_map != Some(map) {
                self.stats.map_changes += 1;
                self.footholds.clear();
                self.nav = None;
                self.nav_map = None;
                self.refuge = None;
                self.pos_ok = None;
                self.pos_pending = None;
                self.reset_tracking();
                self.safe_rest = None;
                self.path_mob = None;
                self.path_fight = None;
                out.push(Action::Move(None));
                out.push(Action::Notify(format!("map changed to {}", map)));
                self.moving = None;
            }
            self.last_map = Some(map);
        }

        if !s.footholds.is_empty() {
            self.footholds = s.footholds.clone();
        }
        self.ensure_nav(s, &mut out);

        // --- Data check: standing still on the ground means standing on a floor of this map. If the read position is
        // on none of the map file's floors for a while, either the position source or the map data is wrong (a
        // position chain froze after leaving a shop and the bot walked on a stale point). Never navigate on that.
        if let Some(nav) = &self.nav {
            let standing = s.player.grounded == Some(true);
            // The flag also reads "grounded" now and then on a ladder or rope: those are not floors but are fine.
            let (px, py) = (s.player.x, s.player.y);
            let on_rope = nav.ladders.iter().any(|l| (l.x - px).abs() <= 10 && py >= l.y1 - 10 && py <= l.y2 + 10);
            if standing && !on_rope && nav.locate_within(px, py, 6).is_none() {
                let since = *self.off_floor_since.get_or_insert(now);
                if now.saturating_sub(since) > OFF_FLOOR_MS {
                    return self.pause(&format!(
                        "paused: standing at ({}, {}) where map {} has no floor - position source or map data is wrong",
                        s.player.x,
                        s.player.y,
                        s.player.map_id.unwrap_or(0)
                    ));
                }
            } else {
                self.off_floor_since = None;
            }
        }

        // --- Only operate in the map(s) we were told to ---------------------
        if let (Some(ok), Some(map)) = (&self.cfg.allowed_maps, s.player.map_id) {
            if !ok.contains(&map) && self.cfg.home_map.is_none() {
                return self.pause(&format!("paused: map {map} is not an allowed map"));
            }
        }

        // --- Safety (evasion) -------------------------------------------
        if self.cfg.evasion {
            if !s.caps.players {
                self.warn_once(&mut out, "evasion", "player evasion unavailable: stream has no other-player data");
            } else if let Some(near) = self.nearest_stranger(s) {
                return self.pause(&format!("paused: stranger within {}px", near));
            }
        }
        self.mode = Mode::Running;
        self.pause_reason.clear();

        // --- Level up / AP ----------------------------------------------
        self.handle_level(s, &mut out);

        // --- Potions ----------------------------------------------------
        if self.cfg.auto_pot {
            self.handle_pots(s, &mut out);
        }

        // --- Timed macros (buffs, pet food, ...) -------------------------------
        self.handle_macros(s, &mut out);

        // --- Out of potions: rest in town, not in the hunting map -------------
        // With no potions HP only comes back at ~10 per 15 s, slower than mobs take it: twice a character died standing
        // still in a mob map, once it reached town with 8 HP. Leave early, rest where nothing attacks, come back.
        // First choice: a platform in this map where no mob spawns (walking mobs stay on their own platform).
        if let Some(hp) = s.player.hp_percent() {
            if self.rest_here(s, hp, &mut out) {
                return out;
            }
        }
        if let (Some(safe), Some(hp)) = (self.cfg.safe_map, s.player.hp_percent()) {
            if !self.town_retreat && self.pots_empty && hp < RETREAT_HP_BELOW && s.player.map_id != Some(safe) {
                self.town_retreat = true;
                self.target = None;
                out.push(Action::Notify(format!("out of potions at HP {hp:.0}%: going to map {safe} to rest")));
            }
            if self.town_retreat {
                if s.player.map_id == Some(safe) {
                    self.stop_moving(&mut out);
                    self.decision = "resting in town".into();
                    if hp >= RETREAT_RESUME_HP {
                        self.town_retreat = false; // still out of potions until a shop trip buys some
                        out.push(Action::Notify(format!("rested to HP {hp:.0}%: going back")));
                    }
                    return out;
                }
                if self.travel_to(s, Some(safe), &mut out) {
                    return out;
                }
            }
        }

        // --- Survival: don't keep fighting at low HP -----------------------
        // On the way to another map the portal is the escape: freezing on a rope in a pig-filled transit map
        // is what killed a level-7 character, so keep walking and let the potions do the healing.
        let in_transit = matches!((self.cfg.home_map, s.player.map_id), (Some(home), Some(here)) if home != here);
        if in_transit && self.fleeing {
            self.fleeing = false;
            self.refuge = None;
        }
        if let Some(hp) = s.player.hp_percent().filter(|_| !in_transit) {
            if !self.fleeing && hp < self.cfg.flee_hp_below {
                self.fleeing = true;
                out.push(Action::Notify(format!("HP {:.0}%: stopping to avoid dying", hp)));
            } else if self.fleeing && hp >= self.cfg.resume_hp_above {
                self.fleeing = false;
                self.refuge = None;
                out.push(Action::Notify(format!("HP recovered to {:.0}%: resuming", hp)));
            }
        }
        // Recovering only matters where something can hit us: in a town (no mobs in the map) keep walking.
        let threatened = !self.town;
        if self.fleeing && threatened {
            self.target = None;
            // Without potions HP barely comes back, so stepping away only means taking hits for free: fight whatever
            // is in reach (a level-3 character with no potions died backing away from snails).
            if self.pots_empty && self.clear_path(s, None, &mut out) {
                self.decision = "low HP, no potions: fighting back".into();
                return out;
            }
            if self.flee_to_refuge(s, &mut out) {
                return out; // pots (above) still fire
            }
            // No rope to hide on: at least step away from whatever is adjacent.
            let near = s
                .mobs
                .iter()
                .filter(|m| (m.y - s.player.y).abs() <= 60 && (m.x - s.player.x).abs() < self.cfg.retreat_distance)
                .min_by_key(|m| (m.x - s.player.x).abs());
            match near {
                Some(m) => {
                    let away = if m.x >= s.player.x { Dir::Left } else { Dir::Right };
                    self.set_move(Some(away), &mut out);
                }
                None => self.stop_moving(&mut out),
            }
            return out;
        }

        // --- Not in the grinding map: walk back ---------------------------------
        if self.travel(s, &mut out) {
            return out;
        }

        // --- Standing at a quest NPC ------------------------------------------------
        if self.walk_to(s, &mut out) {
            return out;
        }

        // --- Looting: only when nothing is in reach to fight -------------
        if self.cfg.loot && s.caps.drops && self.handle_loot(s, &mut out) {
            return out;
        }

        // --- Grinding ---------------------------------------------------
        if self.cfg.kami {
            if !s.caps.mobs {
                self.warn_once(&mut out, "mobs", "kami unavailable: stream has no mob data");
            } else {
                self.handle_grind(s, &mut out);
            }
        }
        out
    }

    // ---------------------------------------------------------------------

    /// Dead: stand still; with `auto_revive` click the revive button after a short pause (a death loop
    /// is capped, then it waits for a human).
    fn handle_dead(&mut self, s: &GameState) -> Vec<Action> {
        let now = s.ts_ms;
        let first = self.dead_since.is_none();
        if first {
            self.dead_since = Some(now);
            self.stats.deaths += 1;
        }
        let mut out = if first || self.mode != Mode::Paused { self.pause("paused: character is dead") } else { vec![] };
        self.revive_times.retain(|t| now.saturating_sub(*t) < 3_600_000);
        let waited = now.saturating_sub(self.dead_since.unwrap_or(now));
        if self.cfg.auto_revive && waited >= 3000 {
            // After a click the screen takes a moment to catch up and still reads dead: wait it out.
            if ready(self.revive_times.last().copied(), now, 8000) {
                if self.revive_times.len() as u32 >= self.cfg.max_revives_per_hour {
                    self.warn_once(&mut out, "revive-cap", "too many deaths this hour: not reviving automatically any more");
                } else {
                    self.revive_times.push(now);
                    self.stats.revives += 1;
                    out.push(Action::Revive);
                    out.push(Action::Notify("reviving in town".into()));
                }
            }
        }
        out
    }

    fn pause(&mut self, why: &str) -> Vec<Action> {
        self.pause_reason = why.trim_start_matches("paused: ").to_string();
        let mut v = vec![];
        if self.mode != Mode::Paused {
            v.push(Action::Notify(why.to_string()));
        }
        self.pause_quiet(&mut v);
        v
    }

    fn pause_quiet(&mut self, out: &mut Vec<Action>) {
        self.mode = Mode::Paused;
        self.set_vert(None, 0, out);
        if self.moving.take().is_some() {
            out.push(Action::Move(None));
        }
    }

    fn reset_tracking(&mut self) {
        self.target = None;
        self.blacklist.clear();
        self.last_pos = None;
    }

    fn warn_once(&mut self, out: &mut Vec<Action>, key: &'static str, msg: &str) {
        if self.warned.insert(key) {
            out.push(Action::Notify(msg.to_string()));
        }
    }

    fn nearest_stranger(&self, s: &GameState) -> Option<i32> {
        s.others
            .iter()
            .filter(|o| {
                !o.name
                    .as_ref()
                    .map(|n| self.cfg.trusted_igns.contains(&n.to_lowercase()) || s.party.iter().any(|p| p.eq_ignore_ascii_case(n)))
                    .unwrap_or(false)
            })
            .map(|o| dist(s.player.x, s.player.y, o.x, o.y))
            .filter(|d| *d < self.cfg.evasion_distance)
            .min()
    }

    fn handle_macros(&mut self, s: &GameState, out: &mut Vec<Action>) {
        let now = s.ts_ms;
        let mp = s.player.mp_percent();
        let fighting = s.mobs.iter().any(|m| {
            (m.x - s.player.x).abs() <= self.cfg.attack_range_x * 2 && (m.y - s.player.y).abs() <= self.cfg.attack_range_y * 2
        });
        for (i, m) in self.cfg.macros.clone().iter().enumerate() {
            if !m.enabled || m.key.is_empty() || m.every_ms == 0 {
                continue;
            }
            if m.only_when_fighting && !fighting {
                continue;
            }
            if m.min_mp_pct > 0.0 && mp.map_or(true, |p| p < m.min_mp_pct) {
                continue;
            }
            if ready(self.macro_last.get(&i).copied(), now, m.every_ms) {
                out.push(Action::Tap(m.key.clone()));
                self.macro_last.insert(i, now);
            }
        }
    }

    /// Mobs within `range` px on the side we are attacking (the target's side), roughly on our floor.
    fn mobs_ahead(&self, s: &GameState, range: i32) -> usize {
        let (px, py) = (s.player.x, s.player.y);
        let side = self.target_pos().map_or(0, |(tx, _)| (tx - px).signum());
        s.mobs
            .iter()
            .filter(|m| !self.ignored_mob(m) && (m.y - py).abs() <= self.cfg.attack_range_y * 2)
            .filter(|m| {
                let dx = m.x - px;
                dx.abs() <= range && (side == 0 || dx.signum() == side || dx.abs() <= self.cfg.attack_range_x / 2)
            })
            .count()
    }

    /// The key for the next attack: the first rotation skill that is off cooldown and affordable,
    /// otherwise the basic attack.
    fn attack_key(&mut self, s: &GameState) -> String {
        let now = s.ts_ms;
        let mp = s.player.mp_percent();
        for (i, k) in self.cfg.rotation.clone().iter().enumerate() {
            if !k.enabled || k.key.is_empty() {
                continue;
            }
            if k.min_mp_pct > 0.0 && mp.map_or(true, |p| p < k.min_mp_pct) {
                continue;
            }
            if k.min_hp_pct > 0.0 && s.player.hp_percent().map_or(true, |p| p < k.min_hp_pct) {
                continue;
            }
            if k.min_mobs > 0 && self.mobs_ahead(s, k.range_x) < k.min_mobs as usize {
                continue;
            }
            if ready(self.skill_last.get(&i).copied(), now, k.cooldown_ms) {
                self.skill_last.insert(i, now);
                return k.key.clone();
            }
        }
        self.cfg.keys.attack.clone()
    }

    fn handle_level(&mut self, s: &GameState, out: &mut Vec<Action>) {
        let Some(level) = s.player.level else {
            if self.cfg.auto_ap {
                self.warn_once(out, "level", "auto-AP unavailable: stream has no level");
            }
            return;
        };
        // First observed level is the baseline, not a level-up.
        let Some(prev) = self.last_level else {
            self.last_level = Some(level);
            return;
        };
        if level > prev && level - prev < 5 {
            let gained = (level - prev) as u32;
            self.stats.levels_gained += gained;
            if self.cfg.auto_ap {
                // Prefer the real unspent-AP count when the stream has it.
                let points = s.player.available_ap.unwrap_or(gained * self.cfg.ap_per_level);
                for _ in 0..points {
                    out.push(Action::Tap(self.cfg.keys.ap_key.clone()));
                }
                self.stats.ap_spent += points;
            }
        }
        // Accept downward moves too (re-roll, bad read) without counting them.
        self.last_level = Some(level);
    }

    fn handle_pots(&mut self, s: &GameState, out: &mut Vec<Action>) {
        let now = s.ts_ms;
        let Some(hp) = s.player.hp_percent() else {
            self.warn_once(out, "hp", "auto-pot unavailable: stream has no HP/MP");
            return;
        };
        // Did the last potion work? A key press gives no other sign of an empty quick slot: if HP has not risen
        // 1.5 s after two presses in a row, the potions are gone (that starts the trip to town with --safe-map).
        if let Some((at, hp_then)) = self.pot_check {
            if now.saturating_sub(at) >= POT_CHECK_MS {
                self.pot_check = None;
                let worked = s.player.hp.zip(hp_then).map_or(true, |(h, h0)| h >= h0 + 10);
                self.pot_misses = if worked { 0 } else { self.pot_misses + 1 };
                if self.pot_misses >= 2 && !self.cfg.keys.hp_pot.starts_with("inv:") && !self.pots_empty {
                    self.pots_empty = true;
                    out.push(Action::Notify("HP potions did not heal twice in a row: out of potions".into()));
                }
            }
        }
        if hp < self.cfg.hp_pot_below && ready(self.last_hp_pot, now, self.cfg.pot_cooldown_ms) {
            out.push(Action::Tap(self.cfg.keys.hp_pot.clone()));
            self.last_hp_pot = Some(now);
            self.stats.hp_pots += 1;
            if self.pot_check.is_none() {
                self.pot_check = Some((now, s.player.hp));
            }
        }
        if let Some(mp) = s.player.mp_percent() {
            if mp < self.cfg.mp_pot_below && ready(self.last_mp_pot, now, self.cfg.pot_cooldown_ms) {
                out.push(Action::Tap(self.cfg.keys.mp_pot.clone()));
                self.last_mp_pot = Some(now);
                self.stats.mp_pots += 1;
            }
        }
    }

    /// Returns true when it took over this tick (walking to or picking up a drop).
    fn handle_loot(&mut self, s: &GameState, out: &mut Vec<Action>) -> bool {
        let now = s.ts_ms;
        let (px, py) = (s.player.x, s.player.y);
        self.loot_blacklist.retain(|(_, until)| *until > now);
        // Count drops that vanished (picked up by us or expired).
        if s.drops.len() < self.prev_drop_count {
            self.stats.drops_gone += (self.prev_drop_count - s.drops.len()) as u32;
        }
        self.prev_drop_count = s.drops.len();
        let live: std::collections::HashSet<(i32, i32)> = s.drops.iter().map(|d| (d.x, d.y)).collect();
        self.loot_tries.retain(|k, _| live.contains(k));

        // Fighting comes first: never turn your back on a mob you can already hit.
        let mob_in_reach = s.mobs.iter().any(|m| {
            (m.x - px).abs() <= self.cfg.attack_range_x && (m.y - py).abs() <= self.cfg.attack_range_y
        });
        if mob_in_reach {
            return false;
        }
        // Walking to a drop only works on our own floor: one on the platform above (30 px up) was "not picked up"
        // over and over.
        let my_floor = self.nav.as_ref().and_then(|n| n.locate_within(px, py, 6));
        let same_floor = |d: &&neon_state::Drop| match (&self.nav, my_floor) {
            (Some(n), Some(f)) => n.locate_within(d.x, d.y, 6).map_or(false, |g| n.same_floor(f, g)),
            _ => (d.y - py).abs() <= self.cfg.loot_range_y,
        };
        let target = s
            .drops
            .iter()
            .filter(|d| {
                same_floor(d)
                    && (d.x - px).abs() <= self.cfg.loot_range_x
                    && !self.loot_blacklist.iter().any(|(k, _)| *k == (d.x, d.y))
            })
            .min_by_key(|d| (d.x - px).abs());
        let Some(d) = target else { return false };

        let dx = d.x - px;
        if dx.abs() > self.cfg.loot_pickup_dist {
            self.set_move(Some(if dx > 0 { Dir::Right } else { Dir::Left }), out);
            return true;
        }
        self.stop_moving(out);
        if ready(self.last_loot, now, self.cfg.loot_interval_ms) {
            let tries = self.loot_tries.entry((d.x, d.y)).or_insert(0);
            *tries += 1;
            if *tries > self.cfg.loot_max_tries {
                // Cannot be picked up (owned by someone else, inventory full...): leave it alone.
                self.loot_blacklist.push(((d.x, d.y), now + self.cfg.loot_blacklist_ms));
                self.loot_tries.remove(&(d.x, d.y));
                out.push(Action::Notify(format!("drop at ({}, {}) will not pick up; ignoring it for a while", d.x, d.y)));
            } else {
                out.push(Action::Tap(self.cfg.keys.loot.clone()));
                self.last_loot = Some(now);
                self.stats.loot_presses += 1;
            }
        }
        true
    }

    fn handle_grind(&mut self, s: &GameState, out: &mut Vec<Action>) {
        let now = s.ts_ms;
        let (px, py) = (s.player.x, s.player.y);

        self.blacklist.retain(|(_, until)| *until > now);
        self.track_facing(s);

        // Re-acquire the tracked target among current mobs. With a server object id it is exactly that mob (gone
        // means dead or despawned); without one, the nearest mob that moved a little, which mistakes a neighbour for
        // a mob that just died in a crowd.
        let tracked = self.target.as_ref().and_then(|t| match t.oid {
            Some(oid) => s.mobs.iter().find(|m| m.oid == Some(oid)).cloned(),
            None => s
                .mobs
                .iter()
                .filter(|m| dist(t.x, t.y, m.x, m.y) <= 120)
                .min_by_key(|m| dist(t.x, t.y, m.x, m.y))
                .cloned(),
        });

        match (&self.target, tracked) {
            (Some(t), None) => {
                if t.attacked {
                    let id = t.id;
                    self.note_kill(s, id);
                }
                self.target = None;
            }
            (Some(_), Some(m)) => {
                let d = self.progress_metric(s, m.x, m.y);
                let t = self.target.as_mut().unwrap();
                t.x = m.x;
                t.y = m.y;
                if d < t.best_dist - 5 {
                    t.best_dist = d;
                    t.best_at = now;
                }
            }
            _ => {}
        }

        // Always aim at the cheapest mob to reach. A mob that spawned closer, or wandered into
        // our path, replaces the one we were heading for; but never abandon a fight in progress.
        // A mob touching us hits us whatever we want to fight: kill it before anything else.
        let toucher = s
            .mobs
            .iter()
            .filter(|m| (m.x - px).abs() <= self.cfg.attack_range_x && (m.y - py).abs() <= self.cfg.attack_range_y)
            // Not one already found to be a phantom (14 swings, no kill): it would be picked again at once, forever
            // (jib01 swung at one for 5 minutes).
            .filter(|m| !self.ignored_mob(m) && !self.blacklist.iter().any(|((bx, by), _)| dist(*bx, *by, m.x, m.y) <= 40))
            .min_by_key(|m| (m.x - px).abs())
            .cloned();
        if let Some(m) = toucher {
            let on_it = self.target.as_ref().map_or(false, |t| {
                (t.x - px).abs() <= self.cfg.attack_range_x && (t.y - py).abs() <= self.cfg.attack_range_y
            });
            if !on_it {
                self.target = Some(Target {
                    x: m.x,
                    y: m.y,
                    since: now,
                    best_dist: 0,
                    best_at: now,
                    attacked: false,
                    last_seg: None,
                    maneuvers: 0,
                    swings: 0,
                    id: m.id,
                    oid: m.oid,
                });
            }
        } else if self.cfg.allowed_mobs.is_some()
            && self.target.is_none()
            && (s.player.grounded != Some(false) || self.seeking)
            && self.pick_target_from(s, true).is_none()
            && { self.seeking = self.seek_spawn(s, out); self.seeking }
        {
            // A kill step whose mobs are nowhere in sight: go where they spawn (fighting what is in the way) rather
            // than grinding whatever is near (Orange Mushrooms spawn only on the upper platforms of map 1000). Only
            // with no target and from the ground: target picking finds nothing in mid-air, and taking over then sent
            // the character toward another goal during a jump, off the platform it was landing on.
            return;
        } else if let Some(best) = self.pick_target(s) {
            let switch = match &self.target {
                None => true,
                Some(t) => {
                    let in_fight = t.attacked
                        && (t.x - px).abs() <= self.cfg.attack_range_x
                        && (t.y - py).abs() <= self.cfg.attack_range_y;
                    !in_fight
                        && dist(t.x, t.y, best.x, best.y) > 60
                        && self.progress_metric(s, best.x, best.y) + 60 < self.progress_metric(s, t.x, t.y)
                }
            };
            if switch {
                self.target = Some(Target {
                    x: best.x,
                    y: best.y,
                    since: now,
                    best_dist: self.progress_metric(s, best.x, best.y),
                    best_at: now,
                    attacked: false,
                    last_seg: None,
                    maneuvers: 0,
                    swings: 0,
                    id: best.id,
                    oid: best.oid,
                });
            }
        }

        self.seeking = false;
        let Some(t) = self.target.clone() else {
            self.stop_moving(out);
            return; // nothing to fight
        };

        // While airborne or on a rope we are not making ground progress by design.
        if s.player.grounded == Some(false) {
            if let Some(t) = self.target.as_mut() {
                t.best_at = now;
            }
        }
        // Not making progress => blacklist this spot and pick another next tick.
        if now.saturating_sub(t.best_at) > self.cfg.stuck_ms {
            self.blacklist.push(((t.x, t.y), now + self.cfg.blacklist_ms));
            // Hit without dying and without getting anywhere: not where we think (until it moves).
            if t.attacked {
                if let Some(oid) = t.oid {
                    self.phantoms.insert(oid, (t.x, t.y));
                }
            }
            self.target = None;
            self.stop_moving(out);
            return;
        }

        let dx = t.x - px;
        let dy = t.y - py;
        let in_x = dx.abs() <= self.cfg.attack_range_x;
        let in_y = dy.abs() <= self.cfg.attack_range_y;

        if in_x && in_y {
            // Face the mob first: attacks only hit the way we face.
            let want = if dx >= 0 { Facing::Right } else { Facing::Left };
            if self.facing != Some(want) && dx.abs() > 8 {
                self.set_move(Some(if dx >= 0 { Dir::Right } else { Dir::Left }), out);
                return;
            }
            // A hit knocks us away from the mob. With the platform's end close behind us that is a fall (jib01 was
            // knocked off a small stair platform and down the staircase while fighting): step in toward the mob first.
            let toward = if dx >= 0 { Dir::Right } else { Dir::Left };
            let behind = if dx >= 0 { Dir::Left } else { Dir::Right };
            let edge_behind = self.nav.as_ref().and_then(|n| n.edge_ahead(px, py, behind));
            if s.player.grounded == Some(true) && edge_behind.map_or(false, |e| e < KNOCKBACK_ROOM_PX) && dx.abs() > 20 {
                self.set_move(Some(toward), out);
                self.decision = "stepping in: the edge is close behind".into();
                return;
            }
            self.stop_moving(out);
            if t.swings >= PHANTOM_SWINGS {
                // Swinging at air: the tracked position is stale. Skip this spot and look again; a mob with a server id
                // is ignored until it moves (a "phantom" whose removal we missed never does).
                self.blacklist.push(((t.x, t.y), now + self.cfg.blacklist_ms));
                if let Some(oid) = t.oid {
                    self.phantoms.insert(oid, (t.x, t.y));
                }
                self.target = None;
                let age = t.oid.and_then(|o| s.mobs.iter().find(|m| m.oid == Some(o))).and_then(|m| m.age_ms);
                out.push(Action::Notify(format!(
                    "{} swings without a kill at ({}, {}); skipping it (mob {:?}, last network update {} ms ago)",
                    t.swings, t.x, t.y, t.id, age.map_or("?".to_string(), |a| a.to_string())
                )));
                return;
            }
            self.decision = "attack".into();
            if self.swing(s, out) {
                if let Some(t) = self.target.as_mut() {
                    t.attacked = true;
                    t.swings += 1;
                }
            }
            return;
        }

        if self.nav.is_some() {
            self.navigate(s, &t, out);
            return;
        }

        // No map geometry: close the horizontal gap and jump at anything above us.
        if dx.abs() > self.cfg.stand_off_x {
            self.set_move(Some(if dx > 0 { Dir::Right } else { Dir::Left }), out);
        } else {
            self.stop_moving(out);
        }
        if dy < -self.cfg.jump_if_above && dx.abs() <= self.cfg.attack_range_x * 2 {
            if ready(self.last_jump, now, 800) {
                out.push(Action::Tap(self.cfg.keys.jump.clone()));
                self.last_jump = Some(now);
            }
        }
    }

    /// How far (in travel cost) we are from a point; plain distance without map geometry.
    fn progress_metric(&self, s: &GameState, x: i32, y: i32) -> i32 {
        let (px, py) = (s.player.x, s.player.y);
        let Some(nav) = &self.nav else { return dist(px, py, x, y) };
        let grounded = s.player.grounded.unwrap_or(true);
        let (Some((from, fx)), Some(goal)) = (nav.anchor(px, py, grounded), nav.locate_within(x, y, 60)) else {
            return i32::MAX / 2;
        };
        nav.cost(from, fx, goal).map_or(i32::MAX / 2, |c| c + if from == goal { (x - px).abs() } else { 0 })
    }

    /// Follow the platform graph toward the target.
    fn navigate(&mut self, s: &GameState, t: &Target, out: &mut Vec<Action>) {
        let now = s.ts_ms;
        let (px, py) = (self.nav_x(s), s.player.y);
        let grounded = s.player.grounded.unwrap_or(true);
        let nav = self.nav.as_ref().expect("checked by caller");
        let cur = if grounded { nav.locate(px, py) } else { None };
        let cmd = match nav.locate_within(t.x, t.y, 60) {
            Some(goal) => nav.step(px, py, grounded, goal, t.x, self.cfg.stand_off_x),
            None => Cmd::NoRoute,
        };

        // Count maneuvers tried from one platform; a jump that never works must not loop forever.
        if let (Some(c), Some(tg)) = (cur, self.target.as_mut()) {
            if tg.last_seg != Some(c) {
                tg.last_seg = Some(c);
                tg.maneuvers = 0;
            }
        }

        self.apply_cmd(cmd, now, grounded, true, out);
    }

    /// Carry out a navigation command. `chasing` = the command serves a mob target (a failed route
    /// blacklists it and failed maneuvers are counted); otherwise it serves fleeing / travelling.
    fn apply_cmd(&mut self, cmd: Cmd, now: u64, grounded: bool, chasing: bool, out: &mut Vec<Action>) {
        self.decision = format!("nav {:?}", cmd);
        // Backing off for a run-up: keep going until there is room, whatever the nav says in the meantime.
        if let Some((d, until, since)) = self.retreat {
            let reached = match d { Dir::Left => self.cur_x <= until, Dir::Right => self.cur_x >= until };
            if reached || !grounded || now.saturating_sub(since) > RETREAT_MAX_MS {
                self.retreat = None;
            } else {
                self.set_vert(None, now, out);
                self.set_move(Some(d), out);
                self.decision = format!("run-up: backing off {d:?} to {until}");
                return;
            }
        }
        if let Cmd::Retreat(d, until) = cmd {
            self.retreat = Some((d, until, now));
            self.set_vert(None, now, out);
            self.set_move(Some(d), out);
            return;
        }
        if !matches!(cmd, Cmd::Climb { .. }) {
            self.set_vert(None, now, out);
        }
        match cmd {
            Cmd::Arrived | Cmd::Stop => {
                // Mid-air after a jump the nav has nothing to say (Stop). Letting go of the direction there drops the
                // character straight back down instead of over the block or onto the ledge, so keep it held until we land.
                let carry = if grounded { None } else { self.jump_hold.filter(|&(_, t)| now.saturating_sub(t) < JUMP_HOLD_MS).map(|(d, _)| d) };
                self.set_move(carry, out);
            }
            Cmd::Walk(d) => self.set_move(Some(d), out),
            Cmd::Align(d, px) => {
                self.set_move(None, out);
                // Let the character settle after a held walk or the last nudge before judging the gap.
                let settled = ready(self.last_release, now, 160) && ready(self.last_nudge, now, 320);
                if settled {
                    // ~150 px/s walk: 5 ms per px deliberately undershoots; the next nudge finishes it.
                    out.push(Action::Nudge(d, ((px as u64) * 5).clamp(35, 300)));
                    self.last_nudge = Some(now);
                }
            }
            Cmd::Jump(d) => {
                self.set_move(d, out);
                {
                    out.push(Action::Tap(self.cfg.keys.jump.clone()));
                    self.last_jump = Some(now);
                    self.jump_hold = d.map(|d| (d, now));
                    if chasing {
                        self.count_maneuver(now, out);
                    }
                }
            }
            Cmd::RunJump(d, staging) => {
                // A running start: the key has been down for a stretch in this direction. Without one, back off to the
                // staging point (if the platform has room) and run in again; with no room, jump from where we are.
                let ran = if self.moving == Some(d) { (self.cur_x - self.move_start_x).abs() } else { 0 };
                // Back off only if there is a full run of RUN_MIN_PX behind us. With less room than that, running in from the
                // staging point can never build more momentum than we have now: backing off again would loop forever (back off,
                // run in, reach the window, turn around) instead of jumping.
                let room = match d { Dir::Right => staging <= self.cur_x - RUN_MIN_PX - 4, Dir::Left => staging >= self.cur_x + RUN_MIN_PX + 4 };
                if ran < RUN_MIN_PX && room {
                    let back = if d == Dir::Right { Dir::Left } else { Dir::Right };
                    let staging = match d { Dir::Right => staging.max(self.cur_x - RUN_BACK_MAX_PX), Dir::Left => staging.min(self.cur_x + RUN_BACK_MAX_PX) };
                    self.retreat = Some((back, staging, now));
                    self.set_move(Some(back), out);
                    self.decision = format!("run-up: backing off {back:?} to {staging}");
                    return;
                }
                self.set_move(Some(d), out);
                // With a run behind us the key has long been down. Without room for one, the key still has to be down a
                // moment before the jump: pressed in the same tick it gives a hop straight up (measured: dx ~ 0).
                let lead_ok = ran >= RUN_MIN_PX || now.saturating_sub(self.move_since) >= PRESS_LEAD_MS;
                if lead_ok {
                    out.push(Action::Tap(self.cfg.keys.jump.clone()));
                    self.last_jump = Some(now);
                    self.jump_hold = Some((d, now));
                    if chasing {
                        self.count_maneuver(now, out);
                    }
                }
            }
            Cmd::DropThrough => {
                self.set_move(None, out);
                if ready(self.last_drop, now, 1200) {
                    out.push(Action::DropDown);
                    self.last_drop = Some(now);
                    if chasing {
                        self.count_maneuver(now, out);
                    }
                }
            }
            Cmd::Climb { dir, grab } => {
                self.set_move(None, out);
                self.set_vert(Some(dir), now, out);
                let held_for = self.held_vert.map_or(0, |(_, since)| now.saturating_sub(since));
                if grab && grounded && held_for > 250 && ready(self.last_jump, now, 1000) {
                    out.push(Action::Tap(self.cfg.keys.jump.clone()));
                    self.last_jump = Some(now);
                    if chasing {
                        self.count_maneuver(now, out);
                    }
                }
            }
            Cmd::Retreat(..) => {} // handled above
            Cmd::NoRoute => {
                if chasing {
                    self.blacklist_target(now, "no route to target");
                }
                self.stop_moving(out);
            }
        }
    }

    /// Recover on the platform at the top of a rope: ground mobs below cannot follow, and HP only
    /// regenerates while standing (a character hanging on a rope does not heal). Walk to the nearest
    /// rope, climb it and stand off to one side. True when it took control this tick.
    fn flee_to_refuge(&mut self, s: &GameState, out: &mut Vec<Action>) -> bool {
        let Some(nav) = &self.nav else { return false };
        let (px, py, now) = (s.player.x, s.player.y, s.ts_ms);
        let grounded = s.player.grounded.unwrap_or(true);
        // Commit to one rope: re-picking "the nearest" every tick makes two ropes trade places
        // and the character oscillates between them, taking hits the whole time.
        let committed = self.refuge.filter(|&li| li < nav.ladders.len());
        let li = match committed.or_else(|| nav.refuge(px, py, grounded, self.cfg.refuge_height + 30).map(|(_, _, li)| li)) {
            Some(li) => li,
            None => return false,
        };
        self.refuge = Some(li);
        let l = &nav.ladders[li];
        let Some(top) = nav.ladder_top(li) else { return false };
        // Stand a few steps in from the rope, toward the middle of that platform.
        let (_, cx) = nav.segment_span(top);
        let stand_x = l.x + if cx >= l.x { 40 } else { -40 };
        let cmd = nav.step(px, py, grounded, top, stand_x, 12);
        self.apply_cmd(cmd, now, grounded, false, out);
        self.decision = format!("flee {:?}", cmd);
        true
    }

    fn count_maneuver(&mut self, now: u64, out: &mut Vec<Action>) {
        let over = self.target.as_mut().map_or(false, |t| {
            t.maneuvers += 1;
            t.maneuvers > 10
        });
        if over {
            self.blacklist_target(now, "route keeps failing");
            self.stop_moving(out);
        }
    }

    fn blacklist_target(&mut self, now: u64, _why: &str) {
        if let Some(t) = self.target.take() {
            self.blacklist.push(((t.x, t.y), now + self.cfg.blacklist_ms));
        }
    }

    /// The mob to fight: an allowed one if any is known; otherwise (a kill step whose mobs are out of sight, e.g. idle
    /// mushrooms the network has not reported yet) the best of the rest, for the EXP, instead of standing idle.
    fn pick_target(&self, s: &GameState) -> Option<Mob> {
        self.pick_target_from(s, true).or_else(|| if self.cfg.prefer_allowed && self.cfg.allowed_mobs.is_some() { self.pick_target_from(s, false) } else { None })
    }

    fn pick_target_from(&self, s: &GameState, only_allowed: bool) -> Option<Mob> {
        let (px, py) = (s.player.x, s.player.y);
        let blacklisted = |m: &Mob| {
            // A mob the network has not mentioned for a while is gone (its removal was missed): every live mob is
            // updated about once a second.
            self.stale(m)
                || m.oid.and_then(|o| self.phantoms.get(&o)).map_or(false, |&(x, y)| dist(x, y, m.x, m.y) <= 20)
                || self.blacklist.iter().any(|((bx, by), _)| dist(*bx, *by, m.x, m.y) <= 40)
                || m.oid.map_or(false, |o| self.peer_claims.contains(&o))
                || (only_allowed && !self.wanted(m))
        };

        if let Some(nav) = &self.nav {
            let grounded = s.player.grounded.unwrap_or(true);
            let Some((seg, x0)) = nav.anchor(px, py, grounded) else { return None };
            let costs = nav.costs_from(seg, x0);
            return s
                .mobs
                .iter()
                .filter(|m| !blacklisted(m))
                .filter_map(|m| {
                    let ms = nav.locate_within(m.x, m.y, 60)?;
                    let c = costs[ms]? + if ms == seg { (m.x - px).abs() } else { 0 };
                    // Stay out of crowds: every other mob next to the target will hit us too.
                    let crowd = s
                        .mobs
                        .iter()
                        .filter(|o| !std::ptr::eq(*o, m) && (o.x - m.x).abs() <= 110 && (o.y - m.y).abs() <= 40)
                        .count() as i32;
                    let c = c + 400 * crowd;
                    Some((c, m))
                })
                .min_by_key(|(c, _)| *c)
                .map(|(_, m)| m.clone());
        }

        s.mobs
            .iter()
            .filter(|m| dist(px, py, m.x, m.y) <= self.cfg.max_target_distance && !blacklisted(m))
            .min_by_key(|m| {
                // Vertical separation costs more: it needs a jump/rope.
                let crowd = s
                    .mobs
                    .iter()
                    .filter(|o| !std::ptr::eq(*o, *m) && (o.x - m.x).abs() <= 110 && (o.y - m.y).abs() <= 40)
                    .count() as i32;
                (m.x - px).abs() + (m.y - py).abs() * 3 + 400 * crowd
            })
            .cloned()
    }

    fn track_facing(&mut self, s: &GameState) {
        if let Some(f) = s.player.facing {
            self.facing = Some(f);
        } else if let Some((prev, _)) = self.last_pos {
            // Infer from actual displacement, not from keys we merely sent.
            if s.player.x > prev.0 + 1 {
                self.facing = Some(Facing::Right);
            } else if s.player.x < prev.0 - 1 {
                self.facing = Some(Facing::Left);
            }
        }
        self.last_pos = Some(((s.player.x, s.player.y), s.ts_ms));
    }

    fn set_move(&mut self, d: Option<Dir>, out: &mut Vec<Action>) {
        if self.moving != d {
            if d.is_none() {
                self.last_release = self.last_ts;
            }
            self.moving = d;
            self.move_since = self.last_ts.unwrap_or(0);
            self.move_start_x = self.cur_x;
            out.push(Action::Move(d));
        }
    }

    fn stop_moving(&mut self, out: &mut Vec<Action>) {
        self.set_move(None, out);
        self.set_vert(None, 0, out);
    }

    fn set_vert(&mut self, v: Option<Vert>, now: u64, out: &mut Vec<Action>) {
        match (self.held_vert, v) {
            (Some((cur, _)), Some(new)) if cur == new => {}
            (None, None) => {}
            (_, Some(new)) => {
                self.held_vert = Some((new, now));
                out.push(Action::Vert(Some(new)));
            }
            (Some(_), None) => {
                self.held_vert = None;
                out.push(Action::Vert(None));
            }
        }
    }

    /// Load the platform/ladder graph for the current map (once per map).
    fn ensure_nav(&mut self, s: &GameState, out: &mut Vec<Action>) {
        let (Some(dir), Some(map)) = (self.map_dir.clone(), s.player.map_id) else { return };
        if self.nav_map == Some(map) {
            return;
        }
        self.nav_map = Some(map);
        let geom = MapGeom::load(&dir, map);
        self.town = geom.as_ref().map_or(false, |g| g.mobs.is_empty());
        self.spawns = geom.as_ref().map_or(vec![], |g| g.mob_spawns.iter().map(|p| (p.id, p.x, p.y)).collect());
        self.spawn_visited.clear();
        self.nav = geom.map(|g| Nav::new(&g, Tuning::default()));
        out.push(Action::Notify(match &self.nav {
            Some(n) => format!("map {}: {} platforms, {} ladders/ropes loaded", map, n.segment_count(), n.ladders.len()),
            None => format!("map {}: no geometry file; falling back to simple chasing", map),
        }));
    }
}

/// True if `gap_ms` has elapsed since `last` (or it never happened).
/// While travelling, a mob this far beyond attack reach ahead of us is dealt with before walking on (px).
const PATH_LOOKAHEAD: i32 = 30;

/// A mob without a network update for this long is treated as gone (live mobs are updated about every second).
const STALE_MOB_MS: u64 = 4000;
/// A mob of unknown kind this close (x) to a spawn point is taken for that spawn's kind.
const SPAWN_NEAR_PX: i32 = 150;
/// A spawn point found empty is not visited again for this long.
const SPAWN_REVISIT_MS: u64 = 60_000;
/// A vanished mob and a rise in our EXP this close together make a kill.
const KILL_EXP_MS: u64 = 1500;
/// Swings at one mob without a kill before it is treated as a phantom.
const PHANTOM_SWINGS: u32 = 14;
/// Fighting in the way this long from one spot means the mobs keep coming: walk past them for PATH_IGNORE_MS.
const PATH_FIGHT_MS: u64 = 20_000;
const PATH_IGNORE_MS: u64 = 8_000;

/// How long after a potion press HP must have risen.
const POT_CHECK_MS: u64 = 1500;

/// Say so when a route passes mobs this many levels above ours.
const ROUTE_DANGER_MARGIN: u32 = 5;

/// Out of potions: leave for `safe_map` below this HP percent, and come back once rested to RETREAT_RESUME_HP.
const RETREAT_HP_BELOW: f64 = 85.0;
const RETREAT_RESUME_HP: f64 = 95.0;

/// How long the character may stand where the map has no floor before the data is called wrong (the grounded flag
/// flickers and landings settle within a few frames).
const OFF_FLOOR_MS: u64 = 2000;

/// Walking speed measured 2026-10-25 (px/s) and the longest we extrapolate a reading.
const WALK_SPEED: f64 = 147.0;
const DEAD_RECKON_MAX_MS: u64 = 130;

/// After a jump take-off the character counts as airborne for this long (a jump lasts about 0.52 s).
const AIR_LOCK_MS: u64 = 480;

/// With no room to run, the direction key goes down this long before the jump.
const PRESS_LEAD_MS: u64 = 100;
/// A directional jump needs a run of at least this many px in its direction. Walking speed is reached in ~20 ms
/// (sim fit: accel 7537 px/s^2), so this is about the key having been down ~100 ms, not about building speed. It was
/// 28: a character landing 18 px before the window backed off for a run-up, reversed near the platform's edge and slid
/// off it (Maple Island's 60 px staircase of small platforms, jib01 again and again).
const RUN_MIN_PX: i32 = 14;
/// A platform to rest on must be at least this wide, and no mob within this distance of the spot.
const SAFE_REST_MIN_W: i32 = 60;
const SAFE_REST_CLEAR_PX: i32 = 100;
/// Fight with at least this much floor behind us (a hit knocks the character back, away from the mob).
const KNOCKBACK_ROOM_PX: i32 = 50;
/// Walking toward the end of a platform, jump this close to it (~70 ms at walking speed).
const EDGE_JUMP_PX: i32 = 10;
/// Back off at most this far for a run-up (never all the way to the far end of the platform).
const RUN_BACK_MAX_PX: i32 = 24;
/// Give up backing off for a run-up after this long.
const RETREAT_MAX_MS: u64 = 2500;

/// A jump's direction stays held this long while airborne (a jump lasts about 0.8 s).
const JUMP_HOLD_MS: u64 = 900;

/// How close (px, horizontally) walking to a spot has to get before the character stands still.
pub const WALK_TO_TOL: i32 = 100;

fn ready(last: Option<u64>, now: u64, gap_ms: u64) -> bool {
    last.map_or(true, |t| now.saturating_sub(t) >= gap_ms)
}

fn dist(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    let (dx, dy) = ((ax - bx) as f64, (ay - by) as f64);
    (dx * dx + dy * dy).sqrt() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use neon_state::{Capabilities, OtherPlayer, Player};

    fn caps() -> Capabilities {
        Capabilities { position: true, map_id: true, level: true, hp_mp: true, mobs: true, ..Default::default() }
    }

    fn state(ts: u64, px: i32, mobs: &[(i32, i32)]) -> GameState {
        GameState {
            seq: ts,
            ts_ms: ts,
            caps: caps(),
            player: Player {
                x: px,
                y: 0,
                map_id: Some(1),
                level: Some(10),
                hp: Some(100),
                max_hp: Some(100),
                mp: Some(100),
                max_mp: Some(100),
                ..Default::default()
            },
            mobs: mobs.iter().map(|&(x, y)| Mob { x, y, id: None, hp_percent: None, oid: None, age_ms: None }).collect(),
            ..Default::default()
        }
    }

    fn brain() -> Brain {
        Brain::new(BrainConfig::default())
    }

    /// Presses of `key`; for the basic attack ("ctrl") starting to hold it counts as one.
    fn taps(a: &[Action], key: &str) -> usize {
        a.iter().filter(|x| matches!(x, Action::Tap(k) if k == key) || (key == "ctrl" && matches!(x, Action::Attack(true)))).count()
    }

    #[test]
    fn walks_toward_distant_mob_then_stops_and_attacks() {
        let mut b = brain();
        // Establish facing right via displacement.
        b.tick(&state(0, 0, &[(300, 0)]));
        let a = b.tick(&state(100, 10, &[(300, 0)]));
        assert!(a.iter().all(|x| !matches!(x, Action::Tap(_))), "no attack while far: {a:?}");
        // Close in.
        let a = b.tick(&state(1000, 250, &[(300, 0)]));
        assert!(a.contains(&Action::Move(None)), "stops to attack: {a:?}");
        assert_eq!(taps(&a, "ctrl"), 1);
    }

    #[test]
    fn faces_mob_before_attacking() {
        let mut b = brain();
        // We last moved left (facing left); mob is to the right and close.
        b.tick(&state(0, 100, &[(150, 0)]));
        b.tick(&state(100, 90, &[(150, 0)]));
        let a = b.tick(&state(200, 90, &[(150, 0)]));
        assert_eq!(taps(&a, "ctrl"), 0, "must turn first: {a:?}");
        assert_eq!(b.moving, Some(Dir::Right), "holding right to turn");
    }

    #[test]
    fn attack_is_held_while_a_mob_is_in_reach_and_counted_per_weapon_cycle() {
        let mut b = brain();
        b.tick(&state(0, 0, &[(30, 0)])); // turns to face the mob
        let first = b.tick(&state(50, 5, &[(30, 0)]));
        assert!(first.contains(&Action::Attack(true)), "{first:?}");
        let soon = b.tick(&state(200, 5, &[(30, 0)]));
        assert!(!soon.contains(&Action::Attack(false)) && !soon.contains(&Action::Attack(true)), "kept down: {soon:?}");
        assert_eq!(b.stats.attacks, 1);
        b.tick(&state(50 + 720, 5, &[(30, 0)]));
        assert_eq!(b.stats.attacks, 2, "one swing per 720 ms cycle");
        let gone = b.tick(&state(900, 5, &[]));
        assert!(gone.contains(&Action::Attack(false)), "let go when nothing is in reach: {gone:?}");
    }

    #[test]
    fn counts_kill_only_after_attacking_and_target_vanishes() {
        let mut b = brain();
        b.tick(&state(0, 0, &[(30, 0)]));
        b.tick(&state(50, 5, &[(30, 0)]));
        b.tick(&state(100, 5, &[(30, 0)]));
        assert_eq!(b.stats.mobs_killed, 0);
        b.tick(&state(900, 5, &[]));
        assert_eq!(b.stats.mobs_killed, 1);
    }

    #[test]
    fn mob_vanishing_without_attack_is_not_a_kill() {
        let mut b = brain();
        b.tick(&state(0, 0, &[(400, 0)]));
        b.tick(&state(100, 0, &[]));
        assert_eq!(b.stats.mobs_killed, 0);
    }

    #[test]
    fn jumps_when_mob_is_above() {
        let mut b = brain();
        let a = b.tick(&state(0, 0, &[(20, -120)]));
        assert_eq!(taps(&a, "alt"), 1, "{a:?}");
    }

    #[test]
    fn stuck_target_is_blacklisted_and_next_chosen() {
        let mut b = brain();
        // Mob unreachably far away vertically; we never get closer.
        let mobs = [(400, -300), (-200, 0)];
        let mut ts = 0;
        b.tick(&state(ts, 0, &mobs));
        let first = b.target.clone().unwrap();
        while ts < 3000 {
            ts += 100;
            b.tick(&state(ts, 0, &mobs));
        }
        let now = b.target.clone().map(|t| (t.x, t.y));
        assert_ne!(now, Some((first.x, first.y)), "should have moved on");
    }

    #[test]
    fn pots_respect_threshold_and_cooldown() {
        let mut b = brain();
        let mut s = state(0, 0, &[]);
        s.player.hp = Some(30);
        let a = b.tick(&s);
        assert_eq!(taps(&a, "F2"), 1);
        s.ts_ms = 500;
        assert_eq!(taps(&b.tick(&s), "F2"), 0, "cooldown");
        s.ts_ms = 1500;
        assert_eq!(taps(&b.tick(&s), "F2"), 1);
    }

    #[test]
    fn hp_and_mp_cooldowns_are_independent() {
        let mut b = brain();
        let mut s = state(0, 0, &[]);
        s.player.hp = Some(30);
        s.player.mp = Some(10);
        let a = b.tick(&s);
        assert_eq!((taps(&a, "F2"), taps(&a, "F3")), (1, 1));
    }

    #[test]
    fn no_pot_when_hp_unknown() {
        let mut b = brain();
        let mut s = state(0, 0, &[]);
        s.player.hp = None;
        s.player.max_hp = None;
        let a = b.tick(&s);
        assert_eq!(taps(&a, "F2"), 0);
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("auto-pot"))));
    }

    #[test]
    fn first_level_reading_is_not_a_level_up() {
        let mut b = brain();
        let a = b.tick(&state(0, 0, &[]));
        assert_eq!(taps(&a, "F1"), 0);
        assert_eq!(b.stats.levels_gained, 0);
    }

    #[test]
    fn level_up_spends_ap() {
        let mut b = brain();
        b.tick(&state(0, 0, &[]));
        let mut s = state(100, 0, &[]);
        s.player.level = Some(11);
        let a = b.tick(&s);
        assert_eq!(taps(&a, "F1"), 5);
        assert_eq!(b.stats.levels_gained, 1);
    }

    #[test]
    fn level_up_uses_real_available_ap_when_known() {
        let mut b = brain();
        b.tick(&state(0, 0, &[]));
        let mut s = state(100, 0, &[]);
        s.player.level = Some(11);
        s.player.available_ap = Some(3);
        assert_eq!(taps(&b.tick(&s), "F1"), 3);
    }

    #[test]
    fn garbage_level_jump_is_ignored() {
        let mut b = brain();
        b.tick(&state(0, 0, &[]));
        let mut s = state(100, 0, &[]);
        s.player.level = Some(150);
        assert_eq!(taps(&b.tick(&s), "F1"), 0);
    }

    #[test]
    fn pauses_without_position_and_releases_keys() {
        let mut b = brain();
        b.tick(&state(0, 0, &[(400, 0)])); // starts moving right
        let mut s = state(100, 0, &[(400, 0)]);
        s.caps.position = false;
        let a = b.tick(&s);
        assert!(a.contains(&Action::Move(None)));
        assert_eq!(b.mode, Mode::Paused);
    }

    #[test]
    fn stalled_stream_drops_tracking_and_releases_keys() {
        let mut b = brain();
        b.tick(&state(0, 0, &[(400, 0)]));
        let a = b.tick(&state(10_000, 0, &[(400, 0)]));
        assert!(a.contains(&Action::Move(None)), "{a:?}");
        assert!(b.target.is_none());
    }

    #[test]
    fn map_change_resets_and_stops() {
        let mut b = brain();
        b.tick(&state(0, 0, &[(400, 0)]));
        let mut s = state(100, 0, &[(400, 0)]);
        s.player.map_id = Some(2);
        let a = b.tick(&s);
        assert!(a.contains(&Action::Move(None)));
        assert_eq!(b.stats.map_changes, 1);
    }

    #[test]
    fn stranger_pauses_bot_trusted_does_not() {
        let mut b = Brain::new(BrainConfig { evasion: true, ..BrainConfig::default() });
        let mut s = state(0, 0, &[(30, 0)]);
        s.caps.players = true;
        s.others.push(OtherPlayer { name: Some("Bob".into()), x: 50, y: 0 });
        let a = b.tick(&s);
        assert_eq!(b.mode, Mode::Paused);
        assert_eq!(taps(&a, "ctrl"), 0);

        let mut cfg = BrainConfig { evasion: true, ..BrainConfig::default() };
        cfg.trusted_igns.insert("bob".into());
        let mut b = Brain::new(cfg);
        b.tick(&s);
        assert_eq!(b.mode, Mode::Running);
    }

    #[test]
    fn a_party_member_nearby_is_not_a_stranger() {
        let mut b = Brain::new(BrainConfig { evasion: true, ..BrainConfig::default() });
        let mut s = state(0, 0, &[(30, 0)]);
        s.caps.players = true;
        s.others.push(OtherPlayer { name: Some("Bob".into()), x: 50, y: 0 });
        s.party = vec!["bob".into()];
        b.tick(&s);
        assert_eq!(b.mode, Mode::Running);
    }

    #[test]
    fn stops_fighting_at_low_hp_and_resumes_with_hysteresis() {
        let mut b = brain();
        let mut s = state(0, 0, &[(30, 0)]);
        s.player.hp = Some(100);
        b.tick(&s); // turn to face
        s.ts_ms = 50; s.player.x = 5;
        assert_eq!(taps(&b.tick(&s), "ctrl"), 1, "healthy: attacks");

        s.ts_ms = 800; s.player.hp = Some(20); // 20%
        let a = b.tick(&s);
        assert_eq!(taps(&a, "ctrl"), 0, "must not attack at 20% HP: {a:?}");
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("avoid dying"))));
        assert_eq!(taps(&a, "F2"), 1, "but still drinks a potion");

        s.ts_ms = 3000; s.player.hp = Some(45); // recovering but below resume threshold
        assert_eq!(taps(&b.tick(&s), "ctrl"), 0, "45% is not enough to resume");
        s.ts_ms = 4000; s.player.hp = Some(90);
        let a = b.tick(&s);
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("resuming"))));
    }

    #[test]
    fn low_hp_in_transit_keeps_travelling_and_still_drinks() {
        let mut b = brain();
        b.cfg.home_map = Some(99);
        let mut s = state(0, 0, &[(400, 0)]);
        s.player.map_id = Some(5); // not the home map: on the way
        s.player.hp = Some(20);
        let a = b.tick(&s);
        assert!(!a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("avoid dying"))), "no freeze while in transit: {a:?}");
        assert_eq!(a.iter().filter(|x| matches!(x, Action::Tap(k) if k == "F2")).count(), 1, "potion still drunk");
    }

    #[test]
    fn flee_stops_a_held_movement_key() {
        let mut b = brain();
        let mut s = state(0, 0, &[(400, 0)]);
        b.tick(&s); // starts walking right
        s.ts_ms = 100; s.player.hp = Some(10);
        assert!(b.tick(&s).contains(&Action::Move(None)));
    }

    #[test]
    fn airborne_time_does_not_count_as_stuck() {
        let mut b = brain();
        let mut s = state(0, 0, &[(400, -300)]);
        s.player.grounded = Some(false);
        b.tick(&s);
        let first = b.target.clone().unwrap();
        let mut ts = 0;
        while ts < 6000 { ts += 100; s.ts_ms = ts; b.tick(&s); }
        let still = b.target.clone().map(|t| (t.x, t.y));
        assert_eq!(still, Some((first.x, first.y)), "no blacklisting while off the ground");
    }

    #[test]
    fn keeps_latest_footholds_and_drops_them_on_map_change() {
        let mut b = brain();
        let mut s = state(0, 0, &[]);
        s.footholds.push(neon_state::Foothold { x1: 0, y1: 35, x2: 500, y2: 35 });
        b.tick(&s);
        assert_eq!(b.footholds().len(), 1);
        s.footholds.clear(); s.ts_ms = 100;
        b.tick(&s);
        assert_eq!(b.footholds().len(), 1, "empty field means 'unchanged', not 'cleared'");
        s.ts_ms = 200; s.player.map_id = Some(2);
        b.tick(&s);
        assert!(b.footholds().is_empty(), "new map invalidates old geometry");
    }

    fn with_drops(mut s: GameState, drops: &[(i32, i32)]) -> GameState {
        s.caps.drops = true;
        s.drops = drops.iter().map(|&(x, y)| neon_state::Drop { x, y, is_meso: None }).collect();
        s
    }

    #[test]
    fn walks_to_a_drop_then_presses_the_pickup_key_with_an_interval() {
        let mut b = brain();
        let s = with_drops(state(0, 0, &[]), &[(200, 0)]);
        let a = b.tick(&s);
        assert!(a.contains(&Action::Move(Some(Dir::Right))), "{a:?}");
        let mut near = with_drops(state(1000, 190, &[]), &[(200, 0)]);
        let a = b.tick(&near);
        assert!(a.contains(&Action::Move(None)));
        assert_eq!(taps(&a, "z"), 1, "close enough: pick up");
        near.ts_ms = 1100;
        assert_eq!(taps(&b.tick(&near), "z"), 0, "interval respected");
        near.ts_ms = 1600;
        assert_eq!(taps(&b.tick(&near), "z"), 1);
    }

    #[test]
    fn fights_a_mob_in_reach_before_looting() {
        let mut b = brain();
        let s = with_drops(state(0, 0, &[(30, 0)]), &[(100, 0)]);
        b.tick(&s);
        let s2 = with_drops(state(50, 5, &[(35, 0)]), &[(100, 0)]);
        let a = b.tick(&s2);
        assert_eq!(taps(&a, "ctrl"), 1, "mob in reach takes priority: {a:?}");
        assert_eq!(taps(&a, "z"), 0);
    }

    #[test]
    fn ignores_drops_on_another_floor() {
        let mut b = brain();
        let s = with_drops(state(0, 0, &[]), &[(100, -240)]);
        let a = b.tick(&s);
        assert!(!a.iter().any(|x| matches!(x, Action::Move(Some(_)))), "must not chase a drop 240px up: {a:?}");
    }

    #[test]
    fn gives_up_on_a_drop_that_will_not_pick_up() {
        let mut b = brain();
        let mut s = with_drops(state(0, 190, &[]), &[(200, 0)]);
        let mut presses = 0;
        let mut ts = 0;
        let mut gave_up = false;
        for _ in 0..30 {
            s.ts_ms = ts;
            let a = b.tick(&s);
            presses += taps(&a, "z");
            if a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("will not pick up"))) {
                gave_up = true;
            }
            ts += 500;
        }
        assert!(gave_up, "expected a give-up notice");
        assert_eq!(presses, 6, "exactly loot_max_tries presses, then it stops");
    }

    #[test]
    fn counts_drops_that_disappear() {
        let mut b = brain();
        b.tick(&with_drops(state(0, 190, &[]), &[(200, 0), (260, 0)]));
        let mut s = with_drops(state(100, 190, &[]), &[(260, 0)]);
        s.ts_ms = 100;
        b.tick(&s);
        assert_eq!(b.stats.drops_gone, 1);
    }

    #[test]
    fn unavailable_features_warn_once() {
        let mut b = brain();
        let mut s = state(0, 0, &[]);
        s.caps.mobs = false;
        let n = |a: &[Action]| a.iter().filter(|x| matches!(x, Action::Notify(m) if m.contains("kami"))).count();
        assert_eq!(n(&b.tick(&s)), 1);
        s.ts_ms = 100;
        assert_eq!(n(&b.tick(&s)), 0);
    }

    // --- navigation -------------------------------------------------------

    use crate::nav::{Ladder, RawFoothold};

    fn nav_geom() -> MapGeom {
        let fh = |x1, y1, x2, y2| RawFoothold { x1, y1, x2, y2, no_drop: false };
        // Ground, a high platform (300 up, far too high to jump), and a ladder at x=600.
        MapGeom {
            footholds: vec![fh(-500, 0, 1000, 0), fh(200, -300, 600, -300)],
            ladders: vec![Ladder { x: 600, y1: -300, y2: 0, ladder: true }],
            portals: vec![],
            mobs: Default::default(), mob_spawns: vec![],
        }
    }

    fn nav_brain() -> Brain {
        Brain::new(BrainConfig::default()).with_geometry(1, &nav_geom())
    }

    fn nav_state(ts: u64, px: i32, py: i32, grounded: bool, mobs: &[(i32, i32)]) -> GameState {
        let mut s = state(ts, px, mobs);
        s.player.y = py;
        s.player.grounded = Some(grounded);
        s
    }

    #[test]
    fn walking_to_a_spot_stops_within_range_instead_of_shuffling_on_the_exact_pixel() {
        let mut b = Brain::new(BrainConfig { walk_to: Some((700, 0)), kami: false, ..BrainConfig::default() }).with_geometry(1, &nav_geom());
        let mut moved_right = false;
        let mut x = 0;
        for i in 0..8 {
            let a = b.tick(&nav_state(1000 + i * 20, x, 0, true, &[]));
            moved_right |= a.iter().any(|a| matches!(a, Action::Move(Some(Dir::Right))));
            x += 90; // the character walks toward the spot
        }
        assert!(moved_right, "walks toward the spot");
        // 650 px: 50 px from the spot. It must stand still, not nudge left and right to get exact.
        let near: Vec<Action> = (0..10).flat_map(|i| b.tick(&nav_state(2000 + i * 20, 650, 0, true, &[]))).collect();
        assert!(!near.iter().any(|a| matches!(a, Action::Move(Some(_)))), "{near:?}");
    }

    #[test]
    fn a_jump_keeps_its_direction_held_in_the_air_and_lets_go_on_landing() {
        let mut b = nav_brain();
        let mut out = vec![];
        b.apply_cmd(Cmd::Jump(Some(Dir::Left)), 1000, true, false, &mut out);
        assert!(out.iter().any(|a| matches!(a, Action::Tap(_))), "jumps: {out:?}");
        // Airborne, the nav says Stop: the direction must stay down.
        let mut air = vec![];
        b.apply_cmd(Cmd::Stop, 1200, false, false, &mut air);
        assert!(!air.iter().any(|a| matches!(a, Action::Move(None))), "released mid-air: {air:?}");
        // Landed: let go.
        let mut ground = vec![];
        b.apply_cmd(Cmd::Stop, 1400, true, false, &mut ground);
        assert!(ground.iter().any(|a| matches!(a, Action::Move(None))), "{ground:?}");
        // A jump that was long ago no longer holds anything.
        let mut late = vec![];
        b.apply_cmd(Cmd::Jump(Some(Dir::Right)), 5000, true, false, &mut late);
        let mut stale = vec![];
        b.apply_cmd(Cmd::Stop, 7000, false, false, &mut stale);
        assert!(stale.iter().any(|a| matches!(a, Action::Move(None))), "{stale:?}");
    }

    #[test]
    fn walks_to_the_ladder_instead_of_jumping_at_a_high_platform() {
        let mut b = nav_brain();
        let mut all = vec![];
        // The mob stands on the platform far above; we are on the ground to the left of the ladder.
        for i in 0..5 {
            all.extend(b.tick(&nav_state(i * 100, 100, 0, true, &[(400, -300)])));
        }
        assert_eq!(taps(&all, "alt"), 0, "must not jump at an unreachable platform: {all:?}");
        assert!(all.iter().any(|a| matches!(a, Action::Move(Some(Dir::Right)))), "{all:?}");
    }

    #[test]
    fn grabs_the_ladder_when_aligned_then_keeps_climbing() {
        let mut b = nav_brain();
        let a = b.tick(&nav_state(0, 600, 0, true, &[(400, -300)]));
        assert!(a.iter().any(|x| matches!(x, Action::Vert(Some(Vert::Up)))), "{a:?}");
        // Mid-ladder, airborne flag false: still climbing, no horizontal movement, no stuck blacklist.
        let a = b.tick(&nav_state(200, 600, -150, false, &[(400, -300)]));
        assert!(!a.iter().any(|x| matches!(x, Action::Vert(None))), "{a:?}");
        assert!(b.target.is_some());
    }

    #[test]
    fn releases_the_ladder_key_once_on_the_platform() {
        let mut b = nav_brain();
        b.tick(&nav_state(0, 600, 0, true, &[(400, -300)]));
        b.tick(&nav_state(200, 600, -150, false, &[(400, -300)]));
        let a = b.tick(&nav_state(400, 600, -300, true, &[(400, -300)]));
        assert!(a.iter().any(|x| matches!(x, Action::Vert(None))), "{a:?}");
        assert!(a.iter().any(|x| matches!(x, Action::Move(Some(Dir::Left)))), "{a:?}");
    }

    #[test]
    fn rope_needs_a_jump_to_grab() {
        let mut g = nav_geom();
        g.ladders[0].ladder = false;
        let mut b = Brain::new(BrainConfig::default()).with_geometry(1, &g);
        b.tick(&nav_state(0, 600, 0, true, &[(400, -300)]));
        let a = b.tick(&nav_state(400, 600, 0, true, &[(400, -300)]));
        assert_eq!(taps(&a, "alt"), 1, "{a:?}");
    }

    #[test]
    fn unreachable_mob_is_not_chased() {
        // A mob on a platform with no way up: it must not become the target.
        let g = MapGeom {
            footholds: vec![RawFoothold { x1: -500, y1: 0, x2: 1000, y2: 0, no_drop: false }, RawFoothold { x1: 200, y1: -400, x2: 600, y2: -400, no_drop: false }],
            ladders: vec![],
            portals: vec![],
            mobs: Default::default(), mob_spawns: vec![],
        };
        let mut b = Brain::new(BrainConfig::default()).with_geometry(1, &g);
        let a = b.tick(&nav_state(0, 100, 0, true, &[(400, -400)]));
        assert!(b.target.is_none(), "{a:?}");
        assert_eq!(taps(&a, "alt"), 0);
    }

    #[test]
    fn prefers_the_reachable_mob_over_a_nearer_unreachable_one() {
        let mut b = nav_brain();
        // One mob right above us on the unreachable-by-jump platform (path via the ladder is long),
        // one on our own floor further away: the walkable one is cheaper.
        b.tick(&nav_state(0, 100, 0, true, &[(120, -300), (500, 0)]));
        let t = b.target.clone().unwrap();
        assert_eq!((t.x, t.y), (500, 0));
    }

    #[test]
    fn drops_through_a_platform_to_reach_a_mob_below() {
        let g = MapGeom {
            footholds: vec![RawFoothold { x1: 0, y1: -100, x2: 400, y2: -100, no_drop: false }, RawFoothold { x1: -500, y1: 0, x2: 1000, y2: 0, no_drop: false }],
            ladders: vec![],
            portals: vec![],
            mobs: Default::default(), mob_spawns: vec![],
        };
        let mut b = Brain::new(BrainConfig::default()).with_geometry(1, &g);
        let a = b.tick(&nav_state(0, 200, -100, true, &[(200, 0)]));
        assert!(a.iter().any(|x| matches!(x, Action::DropDown)), "{a:?}");
    }

    #[test]
    fn avoids_crowds_and_kills_whatever_touches_it() {
        // Nearer mob has two neighbours; the lone one further away is preferred.
        let mut b = brain();
        b.tick(&state(0, 0, &[(150, 0), (190, 0), (230, 0), (500, 0)]));
        assert_eq!(b.target.clone().map(|t| t.x), Some(500));
        // A mob that is not wanted but touches us is fought first.
        let mut cfg = BrainConfig::default();
        cfg.allowed_mobs = Some([2u32].into_iter().collect());
        let mut b = Brain::new(cfg);
        let mut s = state(0, 0, &[(40, 0), (300, 0)]);
        s.mobs[0].id = Some(8);
        s.mobs[1].id = Some(2);
        b.tick(&s);
        assert_eq!(b.target.clone().map(|t| t.x), Some(40));
    }

    #[test]
    fn only_fights_allowed_mob_types() {
        let mut cfg = BrainConfig::default();
        cfg.allowed_mobs = Some([2u32].into_iter().collect());
        let mut b = Brain::new(cfg);
        let mut s = state(0, 0, &[(150, 0), (400, 0)]);
        s.mobs[0].id = Some(8); // a pig, nearer
        s.mobs[1].id = Some(2); // a snail
        b.tick(&s);
        assert_eq!(b.target.clone().map(|t| t.x), Some(400));
        // Unidentified mobs are not fought either.
        let mut b2 = Brain::new(BrainConfig { allowed_mobs: Some([2u32].into_iter().collect()), ..BrainConfig::default() });
        b2.tick(&state(0, 0, &[(150, 0)]));
        assert!(b2.target.is_none());
    }

    #[test]
    fn idle_breaks_wait_until_it_is_safe() {
        let b = brain();
        assert!(b.safe_to_idle(&state(0, 0, &[])));
        assert!(!b.safe_to_idle(&state(0, 0, &[(100, 0)])), "mob nearby");
        let mut hurt = state(0, 0, &[]);
        hurt.player.hp = Some(40);
        assert!(!b.safe_to_idle(&hurt), "low HP");
        let mut climbing = state(0, 0, &[]);
        climbing.player.grounded = Some(false);
        assert!(!b.safe_to_idle(&climbing), "airborne or on a rope");
    }

    #[test]
    fn steps_away_from_an_adjacent_mob_while_recovering() {
        let mut b = brain();
        let mut s = state(0, 100, &[(150, 0)]);
        s.player.hp = Some(20);
        let a = b.tick(&s);
        assert!(a.iter().any(|x| matches!(x, Action::Move(Some(Dir::Left)))), "{a:?}");
        assert_eq!(taps(&a, "ctrl"), 0);
    }

    #[test]
    fn rejects_an_impossible_position_jump_but_accepts_a_real_teleport() {
        let mut b = brain();
        b.tick(&state(0, 100, &[]));
        // 5000 px in 100 ms is a garbage read: the brain must act on the old position.
        let a = b.tick(&state(100, 5100, &[(160, 0)]));
        assert_eq!(b.rejected_positions(), 1);
        assert!(a.iter().any(|x| matches!(x, Action::Move(Some(Dir::Right)))), "chased from x=100, not 5100: {a:?}");
        // The same new place three frames running is real (portal / knock-back).
        b.tick(&state(200, 5100, &[]));
        b.tick(&state(300, 5100, &[]));
        assert_eq!(b.rejected_positions(), 2);
        assert_eq!(b.pos_ok.map(|p| p.0), Some(5100));
    }

    #[test]
    fn normal_walking_is_never_rejected() {
        let mut b = brain();
        for i in 0..50 {
            b.tick(&state(i * 100, (i * 15) as i32, &[]));
        }
        assert_eq!(b.rejected_positions(), 0);
    }

    #[test]
    fn a_mob_that_appears_closer_replaces_the_current_target() {
        let mut b = brain();
        b.tick(&state(0, 0, &[(500, 0)]));
        assert_eq!(b.target.clone().map(|t| t.x), Some(500));
        // A new mob spawns right in front of us.
        b.tick(&state(100, 10, &[(500, 0), (120, 0)]));
        assert_eq!(b.target.clone().map(|t| t.x), Some(120));
    }

    #[test]
    fn does_not_abandon_a_fight_in_progress_for_a_slightly_closer_mob() {
        let mut b = brain();
        // We hit it (first swing happens on the first tick)...
        let a = b.tick(&state(0, 0, &[(6, 0)]));
        assert_eq!(taps(&a, "ctrl"), 1, "{a:?}");
        // ...another mob appears just behind us; keep fighting the first.
        b.tick(&state(100, 0, &[(6, 0), (-3, 0)]));
        assert_eq!(b.target.clone().map(|t| t.x), Some(6));
    }

    #[test]
    fn nudges_instead_of_holding_a_key_when_aligning_under_a_rope() {
        let mut b = nav_brain();
        // 40 px left of the ladder at x=600 on the ground, mob on the high platform.
        let a = b.tick(&nav_state(0, 560, 0, true, &[(400, -300)]));
        assert!(a.iter().any(|x| matches!(x, Action::Nudge(Dir::Right, ms) if (35..=300).contains(ms))), "{a:?}");
        assert!(!a.iter().any(|x| matches!(x, Action::Move(Some(_)))), "no held walk key: {a:?}");
        // Settling: no second nudge within the settle time, even though still misaligned.
        let a = b.tick(&nav_state(100, 568, 0, true, &[(400, -300)]));
        assert!(!a.iter().any(|x| matches!(x, Action::Nudge(..))), "{a:?}");
        let a = b.tick(&nav_state(400, 580, 0, true, &[(400, -300)]));
        assert!(a.iter().any(|x| matches!(x, Action::Nudge(Dir::Right, _))), "{a:?}");
    }

    #[test]
    fn never_reverses_direction_while_aligning() {
        let mut b = nav_brain();
        // Overshooting past the rope must produce a nudge back, never a held walk key either way.
        for (i, x) in [560, 590, 612, 604, 598, 601].into_iter().enumerate() {
            let a = b.tick(&nav_state(i as u64 * 400, x, 0, true, &[(400, -300)]));
            assert!(!a.iter().any(|x| matches!(x, Action::Move(Some(_)))), "x={x}: {a:?}");
        }
    }

    #[test]
    fn only_acts_in_the_allowed_map() {
        let mut cfg = BrainConfig::default();
        cfg.allowed_maps = Some([1u32].into_iter().collect());
        let mut b = Brain::new(cfg);
        let a = b.tick(&state(0, 0, &[(10, 0)]));
        assert!(taps(&a, "ctrl") >= 1 || b.target.is_some(), "state() is map 1: acts");
        let mut elsewhere = state(100, 0, &[(10, 0)]);
        elsewhere.player.map_id = Some(2);
        let a = b.tick(&elsewhere);
        assert_eq!(taps(&a, "ctrl"), 0, "{a:?}");
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("not an allowed map"))), "{a:?}");
        assert_eq!(b.mode, Mode::Paused);
    }

    #[test]
    fn swings_inside_punch_reach_but_not_beyond_it() {
        // Barehanded reach is 64 px; we attack from 52 and walk up to a 38 px stand-off.
        let mut b = brain();
        let mut near = state(0, 0, &[(45, 0)]);
        near.player.facing = Some(Facing::Right);
        let a = b.tick(&near);
        assert_eq!(taps(&a, "ctrl"), 1, "45 px is inside reach: {a:?}");
        let mut b = brain();
        let a = b.tick(&state(0, 0, &[(90, 0)]));
        assert_eq!(taps(&a, "ctrl"), 0, "90 px is out of reach: {a:?}");
        assert!(a.iter().any(|x| matches!(x, Action::Move(Some(Dir::Right)))));
    }

    #[test]
    fn does_not_swing_at_a_mob_on_another_floor_just_above() {
        let mut b = brain();
        // 45 px away horizontally but 60 px up: outside the 33 px punch height.
        let a = b.tick(&state(0, 0, &[(45, -60)]));
        assert_eq!(taps(&a, "ctrl"), 0, "{a:?}");
    }

    // --- travel and refuge ------------------------------------------------

    fn portal(name: &str, x: i32, y: i32, to: u32) -> Portal {
        Portal { name: Some(name.into()), kind: Some(2), x: Some(x), y: Some(y), to_map: Some(to) }
    }

    fn ground_geom() -> MapGeom {
        MapGeom {
            footholds: vec![RawFoothold { x1: -1000, y1: 0, x2: 1000, y2: 0, no_drop: false }],
            ladders: vec![],
            portals: vec![],
            mobs: Default::default(), mob_spawns: vec![],
        }
    }

    fn travel_brain() -> Brain {
        let cfg = BrainConfig { home_map: Some(3), ..BrainConfig::default() };
        Brain::new(cfg)
            .with_geometry(1, &ground_geom())
            .with_portals(1, vec![portal("a", 300, 0, 2)])
            .with_portals(2, vec![portal("b", -50, 0, 3), portal("back", 10, 0, 1)])
    }

    #[test]
    fn plans_a_multi_map_route_home() {
        let b = travel_brain();
        assert_eq!(b.next_portal(1, 3).and_then(|p| p.name), Some("a".into()), "first hop from map 1");
        assert_eq!(b.next_portal(2, 3).and_then(|p| p.name), Some("b".into()));
        assert!(b.next_portal(3, 1).is_none(), "no portals known on map 3");
    }

    #[test]
    fn routes_around_dangerous_maps_even_when_longer() {
        // 1 -> 2 -> 9 is short but map 2 holds level-32 mobs; 1 -> 3 -> 4 -> 9 stays at level 10.
        let b = Brain::new(BrainConfig::default())
            .with_portals(1, vec![portal("short", 0, 0, 2), portal("long", 0, 0, 3)])
            .with_portals(2, vec![portal("x", 0, 0, 9)])
            .with_portals(3, vec![portal("y", 0, 0, 4)])
            .with_portals(4, vec![portal("z", 0, 0, 9)])
            .with_danger(2, 32)
            .with_danger(3, 10)
            .with_danger(4, 8)
            .with_danger(9, 40);
        let (p, danger) = b.route(1, 9).unwrap();
        assert_eq!((p.name.as_deref(), danger), (Some("long"), 10), "the target's own mobs do not count");
        // With equal danger the shorter route wins.
        let b = b.with_danger(2, 10);
        assert_eq!(b.next_portal(1, 9).and_then(|p| p.name), Some("short".into()));
        // A regular portal beats a hidden one to the same place.
        let mut hidden = portal("hidden", 0, 0, 9);
        hidden.kind = Some(1);
        let b = Brain::new(BrainConfig::default()).with_portals(1, vec![hidden, portal("door", 0, 0, 9)]);
        assert_eq!(b.next_portal(1, 9).and_then(|p| p.name), Some("door".into()));
    }

    #[test]
    fn walks_to_the_portal_and_takes_it_when_not_in_the_home_map() {
        let mut b = travel_brain();
        let a = b.tick(&nav_state(0, 0, 0, true, &[(50, 0)])); // state() is map 1, a mob is near: ignored
        assert!(a.iter().any(|x| matches!(x, Action::Move(Some(Dir::Right)))), "{a:?}");
        assert_eq!(taps(&a, "ctrl"), 0, "does not fight on the way");
        let a = b.tick(&nav_state(1500, 300, 0, true, &[]));
        assert_eq!(taps(&a, "Up"), 1, "presses Up on the portal: {a:?}");
        // No second press within the portal cooldown.
        let a = b.tick(&nav_state(1600, 300, 0, true, &[]));
        assert_eq!(taps(&a, "Up"), 0, "{a:?}");
    }

    #[test]
    fn stays_put_and_says_so_when_there_is_no_route_home() {
        let cfg = BrainConfig { home_map: Some(99), ..BrainConfig::default() };
        let mut b = Brain::new(cfg).with_geometry(1, &ground_geom());
        let a = b.tick(&nav_state(0, 0, 0, true, &[]));
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("no portal route"))), "{a:?}");
    }

    fn rope_geom() -> MapGeom {
        let fh = |x1, y1, x2, y2| RawFoothold { x1, y1, x2, y2, no_drop: false };
        MapGeom {
            footholds: vec![fh(-500, 0, 1000, 0), fh(200, -400, 600, -400)],
            ladders: vec![Ladder { x: 600, y1: -400, y2: 0, ladder: false }],
            portals: vec![],
            mobs: Default::default(), mob_spawns: vec![],
        }
    }

    #[test]
    fn low_hp_heads_for_a_rope_instead_of_standing_in_the_fight() {
        let mut b = Brain::new(BrainConfig::default()).with_geometry(1, &rope_geom());
        let mut s = nav_state(0, 100, 0, true, &[(130, 0)]);
        s.player.hp = Some(30);
        let a = b.tick(&s);
        assert!(a.iter().any(|x| matches!(x, Action::Move(Some(Dir::Right)))), "walks toward the rope: {a:?}");
        assert_eq!(taps(&a, "ctrl"), 0);
    }

    #[test]
    fn recovers_standing_on_the_platform_above_not_hanging_on_the_rope() {
        // HP does not regenerate on a rope, so the refuge is the platform at its top.
        let mut b = Brain::new(BrainConfig::default()).with_geometry(1, &rope_geom());
        let mut s = nav_state(0, 600, 0, true, &[]);
        s.player.hp = Some(20);
        let a = b.tick(&s);
        assert!(a.iter().any(|x| matches!(x, Action::Vert(Some(Vert::Up)))), "starts climbing: {a:?}");
        // Mid-rope: keeps climbing, never lets go.
        let mut s2 = nav_state(300, 600, -150, false, &[]);
        s2.player.hp = Some(21);
        let a = b.tick(&s2);
        assert!(!a.iter().any(|x| matches!(x, Action::Vert(None))), "{a:?}");
        // On the top platform: walks in from the rope, then stands still without swinging.
        let mut s3 = nav_state(600, 600, -400, true, &[]);
        s3.player.hp = Some(22);
        let a = b.tick(&s3);
        assert!(a.iter().any(|x| matches!(x, Action::Move(Some(Dir::Left)))), "steps off the rope: {a:?}");
        assert_eq!(taps(&a, "ctrl"), 0);
        let mut s4 = nav_state(900, 560, -400, true, &[]);
        s4.player.hp = Some(23);
        let a = b.tick(&s4);
        assert!(a.iter().any(|x| matches!(x, Action::Move(None))) || !a.iter().any(|x| matches!(x, Action::Move(Some(_)))), "settles: {a:?}");
    }

    #[test]
    fn keeps_the_same_rope_while_fleeing_even_when_another_becomes_nearer() {
        let fh = |x1, y1, x2, y2| RawFoothold { x1, y1, x2, y2, no_drop: false };
        let g = MapGeom {
            footholds: vec![fh(-500, 0, 1500, 0), fh(100, -400, 300, -400), fh(900, -400, 1100, -400)],
            ladders: vec![
                Ladder { x: 200, y1: -400, y2: 0, ladder: false },
                Ladder { x: 1000, y1: -400, y2: 0, ladder: false },
            ],
            portals: vec![],
            mobs: Default::default(), mob_spawns: vec![],
        };
        let mut b = Brain::new(BrainConfig::default()).with_geometry(1, &g);
        let mut s = nav_state(0, 560, 0, true, &[]);
        s.player.hp = Some(20);
        let first = b.tick(&s);
        let dir = |a: &[Action]| a.iter().find_map(|x| if let Action::Move(Some(d)) = x { Some(*d) } else { None });
        let d0 = dir(&first).expect("heads for a rope");
        // Walk a little toward it, past the midpoint where the other rope would be nearer.
        let mut t = 100;
        let mut x = 560;
        for _ in 0..8 {
            x += if d0 == Dir::Left { -40 } else { 40 };
            t += 300;
            let mut s = nav_state(t, x, 0, true, &[]);
            s.player.hp = Some(20);
            let a = b.tick(&s);
            if let Some(d) = dir(&a) {
                assert_eq!(d, d0, "reversed toward the other rope at x={x}: {a:?}");
            }
        }
    }

    // --- macros, rotation, manual pause --------------------------------------

    #[test]
    fn macros_fire_on_their_timer_and_respect_mp_and_enabled() {
        let mut cfg = BrainConfig::default();
        cfg.macros = vec![
            MacroCfg { label: "buff".into(), key: "F5".into(), every_ms: 1000, enabled: true, ..Default::default() },
            MacroCfg { label: "off".into(), key: "F6".into(), every_ms: 1000, enabled: false, ..Default::default() },
            MacroCfg { label: "costly".into(), key: "F7".into(), every_ms: 1000, min_mp_pct: 90.0, enabled: true, ..Default::default() },
        ];
        let mut b = Brain::new(cfg);
        let mut s = state(0, 0, &[]);
        s.player.mp = Some(50); // 50% MP: "costly" must wait
        let a = b.tick(&s);
        assert_eq!((taps(&a, "F5"), taps(&a, "F6"), taps(&a, "F7")), (1, 0, 0), "{a:?}");
        let mut s = state(500, 0, &[]);
        s.player.mp = Some(50);
        assert_eq!(taps(&b.tick(&s), "F5"), 0, "not before its interval");
        let mut s = state(1100, 0, &[]);
        s.player.mp = Some(100);
        let a = b.tick(&s);
        assert_eq!((taps(&a, "F5"), taps(&a, "F7")), (1, 1), "{a:?}");
    }

    #[test]
    fn an_attack_macro_only_runs_when_a_mob_is_close() {
        let mut cfg = BrainConfig::default();
        cfg.macros = vec![MacroCfg { key: "F5".into(), every_ms: 500, only_when_fighting: true, enabled: true, ..Default::default() }];
        let mut b = Brain::new(cfg);
        assert_eq!(taps(&b.tick(&state(0, 0, &[(900, 0)])), "F5"), 0);
        assert_eq!(taps(&b.tick(&state(100, 0, &[(60, 0)])), "F5"), 1);
    }

    #[test]
    fn rotation_skill_replaces_the_basic_attack_while_off_cooldown() {
        let mut cfg = BrainConfig::default();
        cfg.rotation = vec![SkillCfg { label: "snails".into(), key: "s".into(), cooldown_ms: 2000, enabled: true, ..Default::default() }];
        let mut b = Brain::new(cfg);
        let mut near = state(0, 0, &[(6, 0)]);
        near.player.facing = Some(Facing::Right);
        let a = b.tick(&near);
        assert_eq!((taps(&a, "s"), taps(&a, "ctrl")), (1, 0), "skill first: {a:?}");
        let mut near = state(700, 0, &[(6, 0)]);
        near.player.facing = Some(Facing::Right);
        let a = b.tick(&near);
        assert_eq!((taps(&a, "s"), taps(&a, "ctrl")), (0, 1), "on cooldown: basic attack: {a:?}");
        let mut near = state(2100, 0, &[(6, 0)]);
        near.player.facing = Some(Facing::Right);
        assert_eq!(taps(&b.tick(&near), "s"), 1, "ready again");
    }

    #[test]
    fn an_area_attack_waits_for_enough_mobs_in_range() {
        let mut cfg = BrainConfig::default();
        cfg.rotation = vec![
            SkillCfg { label: "slash".into(), key: "Delete".into(), min_mobs: 3, range_x: 130, enabled: true, ..Default::default() },
            SkillCfg { label: "strike".into(), key: "Shift_L".into(), enabled: true, ..Default::default() },
        ];
        let mut b = Brain::new(cfg);
        let mut one = state(0, 0, &[(6, 0), (400, 0)]);
        one.player.facing = Some(Facing::Right);
        let a = b.tick(&one);
        assert_eq!((taps(&a, "Delete"), taps(&a, "Shift_L")), (0, 1), "one mob in range: single-target skill: {a:?}");
        let mut three = state(800, 0, &[(6, 0), (60, 0), (120, 0)]);
        three.player.facing = Some(Facing::Right);
        let a = b.tick(&three);
        assert_eq!((taps(&a, "Delete"), taps(&a, "Shift_L")), (1, 0), "three in range: area attack: {a:?}");
    }

    #[test]
    fn manual_pause_stops_everything_and_resume_restarts_it() {
        let mut b = brain();
        let mut near = state(0, 0, &[(6, 0)]);
        near.player.facing = Some(Facing::Right);
        b.set_manual_pause(true);
        let a = b.tick(&near);
        assert_eq!(taps(&a, "ctrl"), 0, "{a:?}");
        assert_eq!(b.mode, Mode::Paused);
        assert!(b.pause_reason.contains("control panel"), "{}", b.pause_reason);
        b.set_manual_pause(false);
        let mut near = state(100, 0, &[(6, 0)]);
        near.player.facing = Some(Facing::Right);
        assert_eq!(taps(&b.tick(&near), "ctrl"), 1);
        assert_eq!(b.mode, Mode::Running);
        assert!(b.pause_reason.is_empty());
    }

    #[test]
    fn a_lie_detector_alert_stops_all_input_and_says_so_once() {
        let mut b = brain();
        let mut near = state(0, 0, &[(6, 0)]);
        near.player.facing = Some(Facing::Right);
        near.alerts.lie_detector = true;
        let a = b.tick(&near);
        assert_eq!(taps(&a, "ctrl"), 0, "{a:?}");
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("lie detector"))), "{a:?}");
        let mut again = state(100, 0, &[(6, 0)]);
        again.alerts.lie_detector = true;
        let a = b.tick(&again);
        assert!(!a.iter().any(|x| matches!(x, Action::Notify(_))), "no repeated notice: {a:?}");
        // Cleared: back to work.
        let mut clear = state(200, 0, &[(6, 0)]);
        clear.player.facing = Some(Facing::Right);
        assert_eq!(taps(&b.tick(&clear), "ctrl"), 1);
    }

    // --- death, revive, per-mob stats ----------------------------------------

    fn dead_state(ts: u64) -> GameState {
        let mut s = state(ts, 0, &[]);
        s.player.hp = Some(0);
        s.player.dead = Some(true);
        s
    }

    /// Tick every 1.5 s (a stalled stream would reset tracking) from `from` to `to`; returns every action.
    fn run_dead(b: &mut Brain, from: u64, to: u64, dead: bool) -> Vec<Action> {
        let mut all = vec![];
        let mut t = from;
        while t <= to {
            all.extend(b.tick(&if dead { dead_state(t) } else { state(t, 0, &[]) }));
            t += 1500;
        }
        all
    }

    #[test]
    fn a_death_is_counted_once_and_nothing_is_pressed() {
        let mut b = brain();
        let a = run_dead(&mut b, 0, 9000, true);
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("dead"))), "{a:?}");
        assert!(!a.iter().any(|x| matches!(x, Action::Revive | Action::Tap(_))), "auto_revive is off: {a:?}");
        assert_eq!(b.stats.deaths, 1);
        run_dead(&mut b, 10_500, 12_000, false);
        run_dead(&mut b, 13_500, 16_500, true);
        assert_eq!(b.stats.deaths, 2);
    }

    #[test]
    fn auto_revive_waits_clicks_once_and_is_capped_per_hour() {
        let cfg = BrainConfig { auto_revive: true, max_revives_per_hour: 2, ..BrainConfig::default() };
        let mut b = Brain::new(cfg);
        let revives = |a: &[Action]| a.iter().filter(|x| matches!(x, Action::Revive)).count();
        assert_eq!(revives(&run_dead(&mut b, 0, 1500, true)), 0, "gives the dialog a moment");
        assert_eq!(revives(&run_dead(&mut b, 3000, 6000, true)), 1, "one click, no double click");
        run_dead(&mut b, 7500, 9000, false);
        assert_eq!(revives(&run_dead(&mut b, 10_500, 21_000, true)), 1, "second death, second revive");
        run_dead(&mut b, 22_500, 24_000, false);
        let a = run_dead(&mut b, 25_500, 36_000, true);
        assert_eq!(revives(&a), 0, "{a:?}");
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("too many deaths"))), "{a:?}");
        // An hour later the allowance is back.
        run_dead(&mut b, 3_700_000, 3_701_500, false);
        assert_eq!(revives(&run_dead(&mut b, 3_703_000, 3_712_000, true)), 1);
    }

    #[test]
    fn kills_are_counted_per_mob_type() {
        let mut b = brain();
        let mut near = state(0, 0, &[(6, 0)]);
        near.player.facing = Some(Facing::Right);
        near.mobs[0].id = Some(3);
        b.tick(&near); // swings
        b.tick(&state(700, 0, &[])); // it is gone: a kill
        assert_eq!(b.stats.mobs_killed, 1);
        assert_eq!(b.stats.kills_by_mob.get(&3), Some(&1), "{:?}", b.stats.kills_by_mob);
    }

    #[test]
    fn travelling_fights_a_mob_in_the_way_instead_of_walking_into_it() {
        let g = MapGeom {
            footholds: vec![RawFoothold { x1: -500, y1: 0, x2: 500, y2: 0, no_drop: false }],
            ladders: vec![],
            portals: vec![],
            mobs: Default::default(), mob_spawns: vec![],
        };
        let mk = || {
            let mut b = Brain::new(BrainConfig { home_map: Some(2), ..BrainConfig::default() }).with_geometry(1, &g);
            b.portals.insert(1, vec![crate::nav::Portal { name: Some("east".into()), kind: Some(2), x: Some(400), y: Some(0), to_map: Some(2) }]);
            b
        };
        // A snail 50 px ahead on the way to the portal: stop and hit it.
        let mut b = mk();
        let mut s = nav_state(0, 0, 0, true, &[(50, 0)]);
        s.player.facing = Some(Facing::Right);
        let a = b.tick(&s);
        assert_eq!(taps(&a, "ctrl"), 1, "{a:?}");
        assert!(!a.contains(&Action::Move(Some(Dir::Right))), "{a:?}");
        // One behind us, out of reach: keep walking to the portal.
        let mut b = mk();
        let mut s = nav_state(0, 0, 0, true, &[(-200, 0)]);
        s.player.facing = Some(Facing::Right);
        let a = b.tick(&s);
        assert_eq!(taps(&a, "ctrl"), 0, "{a:?}");
        assert!(a.contains(&Action::Move(Some(Dir::Right))), "{a:?}");
        // A mob in the way that never dies (always a fresh one there, or not really there): after a while, walk on.
        let mut b = mk();
        let mut s = nav_state(0, 0, 0, true, &[(50, 0)]);
        s.player.facing = Some(Facing::Right);
        let (mut walked_on, mut said) = (false, false);
        for t in (0..30_000).step_by(100) {
            s.ts_ms = t;
            let a = b.tick(&s);
            said |= a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("walking on") || m.contains("skipping it")));
            walked_on |= a.contains(&Action::Move(Some(Dir::Right)));
        }
        assert!(said && walked_on, "a blocker that never goes away does not hold the character forever");
        // One that dies (leaves the server's list) after being hit is counted as a kill.
        let mut b = mk();
        let mut s = nav_state(0, 0, 0, true, &[(50, 0)]);
        s.player.facing = Some(Facing::Right);
        s.mobs[0].oid = Some(77);
        s.mobs[0].id = Some(2);
        b.tick(&s);
        s.ts_ms = 1000;
        s.mobs.clear();
        b.tick(&s);
        assert_eq!((b.stats.mobs_killed, b.stats.kills_by_mob.get(&2).copied()), (1, Some(1)));
        // Leaving the map empties the mob list: that is not a kill.
        let mut b = mk();
        let mut s = nav_state(0, 0, 0, true, &[(50, 0)]);
        s.player.facing = Some(Facing::Right);
        s.mobs[0].oid = Some(78);
        b.tick(&s);
        s.ts_ms = 1000;
        s.mobs.clear();
        s.player.map_id = Some(3);
        b.tick(&s);
        assert_eq!(b.stats.mobs_killed, 0);
        // With EXP readable: a mob that vanishes without our EXP rising was someone else's kill.
        let mut b = mk();
        let mut s = nav_state(0, 0, 0, true, &[(50, 0)]);
        s.player.facing = Some(Facing::Right);
        s.player.exp = Some(10);
        s.mobs[0].oid = Some(79);
        b.tick(&s);
        s.mobs.clear();
        for t in (1000..4000).step_by(100) {
            s.ts_ms = t;
            b.tick(&s);
        }
        assert_eq!(b.stats.mobs_killed, 0, "vanished, no EXP: not ours");
        let mut s = nav_state(0, 0, 0, true, &[(50, 0)]);
        s.ts_ms = 5000;
        s.player.facing = Some(Facing::Right);
        s.player.exp = Some(10);
        s.mobs[0].oid = Some(80);
        b.tick(&s);
        s.ts_ms = 5400;
        s.mobs.clear();
        b.tick(&s);
        s.ts_ms = 5600;
        s.player.exp = Some(14);
        b.tick(&s);
        assert_eq!(b.stats.mobs_killed, 1, "vanished and our EXP rose: ours");
        // A mob of unknown kind (template 0) is named by the EXP it paid: 15 is an Orange Mushroom.
        let mut b = mk();
        b.cfg.mob_by_exp.insert(15, 9);
        let mut s = nav_state(0, 0, 0, true, &[(50, 0)]);
        s.player.facing = Some(Facing::Right);
        s.player.exp = Some(14);
        s.mobs[0].oid = Some(81);
        s.mobs[0].id = None;
        b.tick(&s);
        s.ts_ms = 400;
        s.mobs.clear();
        b.tick(&s);
        s.ts_ms = 600;
        s.player.exp = Some(29);
        b.tick(&s);
        assert_eq!(b.stats.kills_by_mob.get(&9).copied(), Some(1), "{:?}", b.stats.kills_by_mob);
    }

    #[test]
    fn low_hp_without_potions_fights_back_instead_of_backing_away() {
        let mut b = Brain::new(BrainConfig { kami: true, ..BrainConfig::default() });
        let mut s = state(0, 0, &[(30, 0)]);
        s.player.facing = Some(Facing::Right);
        s.player.hp = Some(30);
        b.pots_empty = true;
        let a = b.tick(&s);
        assert!(taps(&a, "ctrl") > 0, "{a:?} {}", b.decision);
        // With potions it still steps away and lets them work.
        let mut b = Brain::new(BrainConfig { kami: true, ..BrainConfig::default() });
        let a = b.tick(&s);
        assert_eq!(taps(&a, "ctrl"), 0, "{a:?}");
    }

    #[test]
    fn walking_toward_the_end_of_a_platform_jumps_before_running_off() {
        // Two joined floor pieces (0..100 and 100..200), then nothing.
        let g = MapGeom {
            footholds: vec![RawFoothold { x1: 0, y1: 0, x2: 100, y2: 0, no_drop: false }, RawFoothold { x1: 100, y1: 0, x2: 200, y2: 0, no_drop: false }],
            ladders: vec![],
            portals: vec![],
            mobs: Default::default(),
            mob_spawns: vec![],
        };
        let n = crate::nav::Nav::new(&g, Tuning::default());
        assert_eq!(n.edge_ahead(95, 0, Dir::Right), Some(105), "the joint at 100 is not an edge");
        assert_eq!(n.edge_ahead(95, 0, Dir::Left), Some(95));
        let mut b = Brain::new(BrainConfig::default()).with_geometry(1, &g);
        b.moving = Some(Dir::Right);
        let mut s = nav_state(5000, 194, 0, true, &[]);
        let mut out = vec![];
        b.jump_at_edge(&s, &mut out);
        assert_eq!(taps(&out, &b.cfg.keys.jump.clone()), 1, "6 px from the end: jump");
        // Mid-platform, or across the joint: no jump.
        let mut b = Brain::new(BrainConfig::default()).with_geometry(1, &g);
        b.moving = Some(Dir::Right);
        s.player.x = 95;
        let mut out = vec![];
        b.jump_at_edge(&s, &mut out);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn steps_in_toward_the_mob_when_the_edge_is_close_behind() {
        let g = MapGeom { footholds: vec![RawFoothold { x1: 0, y1: 0, x2: 300, y2: 0, no_drop: false }], ladders: vec![], portals: vec![], mobs: Default::default(), mob_spawns: vec![] };
        let mut b = Brain::new(BrainConfig { kami: true, ..BrainConfig::default() }).with_geometry(1, &g);
        let mut s = nav_state(0, 20, 0, true, &[(60, 0)]);
        s.player.facing = Some(Facing::Right);
        let a = b.tick(&s);
        assert!(a.contains(&Action::Move(Some(Dir::Right))) && !a.contains(&Action::Attack(true)), "{a:?} {}", b.decision);
        // With room behind, it just attacks.
        let mut b = Brain::new(BrainConfig { kami: true, ..BrainConfig::default() }).with_geometry(1, &g);
        let mut s = nav_state(0, 150, 0, true, &[(190, 0)]);
        s.player.facing = Some(Facing::Right);
        let a = b.tick(&s);
        assert!(a.contains(&Action::Attack(true)), "{a:?} {}", b.decision);
    }

    #[test]
    fn out_of_potions_rests_on_a_platform_no_mob_spawns_on() {
        // Ground 0..400 with a snail spawn; a separate ledge 500..700 (a gap between) with none.
        let g = MapGeom {
            footholds: vec![RawFoothold { x1: 0, y1: 0, x2: 400, y2: 0, no_drop: false }, RawFoothold { x1: 450, y1: 0, x2: 700, y2: 0, no_drop: false }],
            ladders: vec![],
            portals: vec![],
            mobs: [("2".to_string(), 1)].into_iter().collect(),
            mob_spawns: vec![crate::nav::SpawnPoint { id: 2, x: 100, y: 0 }],
        };
        let mut b = Brain::new(BrainConfig { kami: true, ..BrainConfig::default() }).with_geometry(1, &g);
        b.pots_empty = true;
        let mut s = nav_state(0, 300, 0, true, &[]);
        s.player.hp = Some(40);
        let a = b.tick(&s);
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("safe platform"))), "{a:?}");
        let (_, gx, _) = b.safe_rest.expect("a spot");
        assert!(gx >= 450, "the ledge, not the snails' ground: {gx}");
        assert!(a.contains(&Action::Move(Some(Dir::Right))) || a.iter().any(|x| matches!(x, Action::Tap(_))), "{a:?} {}", b.decision);
    }

    #[test]
    fn alone_in_a_map_old_mobs_are_still_there() {
        // Nobody else in the map: our client moves the mobs, the server sends no moves, every mob looks old.
        let mut s = state(0, 0, &[(300, 0)]);
        s.mobs[0].age_ms = Some(60_000);
        s.caps.players = true;
        s.player.facing = Some(Facing::Right);
        let mut b = brain();
        b.tick(&s);
        assert!(b.target.is_some(), "alone: the old mob is fought");
        // With another player around, an old mob is a missed removal.
        s.others.push(neon_state::OtherPlayer { name: Some("x".into()), x: 500, y: 0 });
        let mut b = brain();
        b.tick(&s);
        assert!(b.target.is_none(), "not alone: skipped");
    }

    #[test]
    fn a_phantom_in_reach_is_not_fought_again() {
        // Alone in the map (old mobs count), one "mob" in reach that never dies.
        let mut s = state(0, 0, &[(30, 0)]);
        s.caps.players = true;
        s.mobs[0].oid = Some(5);
        s.mobs[0].age_ms = Some(60_000);
        s.player.facing = Some(Facing::Right);
        let mut b = brain();
        for t in (0..30_000).step_by(100) {
            s.ts_ms = t;
            b.tick(&s);
        }
        assert!(b.target.is_none(), "left alone (blacklisted or a phantom), not swung at forever");
        let before = b.stats.attacks;
        for t in (30_000..40_000).step_by(100) {
            s.ts_ms = t;
            b.tick(&s);
        }
        assert_eq!(b.stats.attacks, before, "no more swings at it");
    }

    #[test]
    fn a_kill_step_goes_to_where_its_mobs_spawn() {
        let g = MapGeom {
            footholds: vec![RawFoothold { x1: -500, y1: 0, x2: 900, y2: 0, no_drop: false }],
            ladders: vec![],
            portals: vec![],
            mobs: [("9".to_string(), 1)].into_iter().collect(),
            mob_spawns: vec![crate::nav::SpawnPoint { id: 9, x: 700, y: 0 }],
        };
        let cfg = BrainConfig { kami: true, allowed_mobs: Some([9].into_iter().collect()), prefer_allowed: true, ..BrainConfig::default() };
        // Nothing wanted in sight (a snail behind us): head for the Orange Mushroom spawn, not the snail.
        let mut b = Brain::new(cfg.clone()).with_geometry(1, &g);
        let mut s = nav_state(0, 0, 0, true, &[(-300, 0)]);
        s.mobs[0].id = Some(2);
        let a = b.tick(&s);
        assert!(a.contains(&Action::Move(Some(Dir::Right))), "{a:?} {}", b.decision);
        // A mob of unknown kind next to that spawn is taken for one of them.
        let mut b = Brain::new(cfg).with_geometry(1, &g);
        let mut s = nav_state(0, 0, 0, true, &[(-300, 0), (680, 0)]);
        s.mobs[0].id = Some(2);
        s.mobs[1].id = None;
        b.tick(&s);
        assert_eq!(b.target.as_ref().map(|t| t.x), Some(680));
    }

    #[test]
    fn a_potion_hotkey_that_does_not_heal_means_out_of_potions() {
        let mut cfg = BrainConfig::default();
        cfg.keys.hp_pot = "Insert".into();
        let mut b = Brain::new(cfg);
        let mut s = state(0, 0, &[]);
        s.player.hp = Some(40);
        let mut said = false;
        for t in (0..8000).step_by(100) {
            s.ts_ms = t;
            let a = b.tick(&s);
            said |= a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("out of potions")));
        }
        assert!(b.pots_empty && said, "presses that never heal end in 'out of potions'");
        // Potions that work never trip it.
        let mut cfg = BrainConfig::default();
        cfg.keys.hp_pot = "Insert".into();
        let mut b = Brain::new(cfg);
        let mut s = state(0, 0, &[]);
        let mut hp = 40;
        for t in (0..8000).step_by(100) {
            s.ts_ms = t;
            s.player.hp = Some(hp);
            let a = b.tick(&s);
            if taps(&a, "Insert") > 0 {
                hp = 40; // heals, then gets hit back down before the next press
            }
            if t % 1000 == 500 {
                hp = 90;
            }
        }
        assert!(!b.pots_empty);
    }

    #[test]
    fn out_of_potions_it_rests_in_town_and_comes_back() {
        let mut b = Brain::new(BrainConfig { safe_map: Some(1), home_map: Some(2), ..BrainConfig::default() });
        b.pots_empty = true;
        // In the hunting map (2) at 70 % HP: head for town. (No portal data here, so it cannot walk there and
        // defends itself instead of standing still.)
        let mut s = state(0, 0, &[(20, 0)]);
        s.player.map_id = Some(2);
        s.player.hp = Some(70);
        s.player.facing = Some(Facing::Right);
        let a = b.tick(&s);
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("out of potions"))), "{a:?}");
        assert!(b.town_retreat);
        // In town (1): stand and rest until 95 %.
        s.player.map_id = Some(1);
        s.ts_ms = 1000;
        assert_eq!(taps(&b.tick(&s), "ctrl"), 0);
        s.player.hp = Some(96);
        s.ts_ms = 2000;
        let a = b.tick(&s);
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("going back"))), "{a:?}");
        assert!(b.pots_empty, "still out of potions: only a shop trip changes that");
    }

    #[test]
    fn a_kill_in_a_crowd_is_counted_by_object_id() {
        let mut b = brain();
        // Two slimes side by side; we fight the near one (oid 1).
        let mut s = state(0, 0, &[(30, 0), (70, 0)]);
        s.player.facing = Some(Facing::Right);
        for (i, m) in s.mobs.iter_mut().enumerate() {
            m.oid = Some(i as u32 + 1);
            m.id = Some(7);
        }
        for t in (0..2000).step_by(100) {
            s.ts_ms = t;
            b.tick(&s);
        }
        assert_eq!(b.target.as_ref().and_then(|t| t.oid), Some(1));
        // It dies: its id leaves the list; the neighbour 40 px away must not be taken for it.
        s.mobs.remove(0);
        s.ts_ms = 2100;
        b.tick(&s);
        assert_eq!(b.stats.kills_by_mob.get(&7), Some(&1), "the kill is counted");
    }

    #[test]
    fn standing_where_the_map_has_no_floor_pauses_the_bot() {
        let g = MapGeom {
            footholds: vec![RawFoothold { x1: -500, y1: 0, x2: 500, y2: 0, no_drop: false }],
            ladders: vec![],
            portals: vec![],
            mobs: Default::default(), mob_spawns: vec![],
        };
        let mut b = Brain::new(BrainConfig::default()).with_geometry(1, &g);
        // "Standing" 450 px below any floor (a frozen or foreign position read), frame after frame.
        let mut paused_at = None;
        let mut said = false;
        for t in (0..3000).step_by(100) {
            let a = b.tick(&nav_state(t, 100, 450, true, &[]));
            said |= a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("no floor")));
            if b.mode == Mode::Paused && paused_at.is_none() {
                paused_at = Some(t);
            }
        }
        let at = paused_at.expect("pauses");
        assert!(at > 1500, "a moment off the floor is a landing settling (paused at {at})");
        assert!(said);
        // Back on the floor: resumes (a jump of 450 px is first treated as a glitch, then believed).
        for t in (3100..4000).step_by(100) {
            b.tick(&nav_state(t, 100, 0, true, &[]));
        }
        assert_eq!(b.mode, Mode::Running);
    }

    #[test]
    fn a_silent_server_stops_all_input_until_it_talks_again() {
        let mut b = brain();
        let mut near = state(0, 0, &[(6, 0)]);
        near.player.facing = Some(Facing::Right);
        near.alerts.connection_stalled = true;
        near.caps.position = false; // back at the login screen
        let a = b.tick(&near);
        assert_eq!(taps(&a, "ctrl"), 0, "{a:?}");
        assert!(a.iter().any(|x| matches!(x, Action::Notify(m) if m.contains("game server"))), "{a:?}");
        let mut ok = state(100, 0, &[(6, 0)]);
        ok.player.facing = Some(Facing::Right);
        assert_eq!(taps(&b.tick(&ok), "ctrl"), 1);
        // A quiet map with the character still in game: keep going.
        let mut quiet = state(200, 0, &[(6, 0)]);
        quiet.player.facing = Some(Facing::Right);
        quiet.alerts.connection_stalled = true;
        let mut b = brain();
        let first = taps(&b.tick(&quiet), "ctrl");
        quiet.ts_ms += 1000;
        assert!(first + taps(&b.tick(&quiet), "ctrl") >= 1, "a quiet server alone does not stop the bot");
    }
}

#[cfg(test)]
mod perf {
    use super::*;
    use neon_state::{Capabilities, Mob, Player};

    /// Planning runs every frame (up to 50 Hz) on maps with ~160 platforms and dozens of mobs.
    #[test]
    fn a_tick_on_a_real_map_with_many_mobs_is_fast() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../neon-calib/maps");
        let Some(geom) = MapGeom::load(&dir, 10001030) else { return };
        let mut b = Brain::new(BrainConfig::default()).with_geometry(10001030, &geom);
        let mobs: Vec<Mob> = (0..45).map(|i| Mob { x: -900 + i * 45, y: 35, id: Some(2), hp_percent: None, oid: None, age_ms: None }).collect();
        let mk = |ts: u64, x: i32| GameState {
            seq: ts,
            ts_ms: ts,
            caps: Capabilities { position: true, map_id: true, level: true, hp_mp: true, mobs: true, ..Default::default() },
            player: Player { x, y: 35, map_id: Some(10001030), level: Some(7), hp: Some(100), max_hp: Some(100), grounded: Some(true), ..Default::default() },
            mobs: mobs.clone(),
            ..Default::default()
        };
        let t0 = std::time::Instant::now();
        let n = 200;
        for i in 0..n {
            b.tick(&mk(i * 20, 900 - (i as i32 % 40)));
        }
        let per = t0.elapsed().as_micros() as f64 / n as f64;
        eprintln!("tick = {per:.0} us");
        assert!(per < 3000.0, "tick too slow: {per} us");
    }

}

#[cfg(test)]
mod party_claim_tests {
    use super::*;

    #[test]
    fn a_mob_a_peer_is_after_is_not_picked() {
        let mut cfg = BrainConfig::default();
        let mut b = Brain::new(cfg);
        let mut s = GameState::default();
        s.player.x = 0;
        s.player.y = 35;
        s.mobs = vec![
            Mob { x: 50, y: 35, id: None, hp_percent: None, oid: Some(1), age_ms: None },
            Mob { x: 120, y: 35, id: None, hp_percent: None, oid: Some(2), age_ms: None },
        ];
        assert_eq!(b.pick_target(&s).and_then(|m| m.oid), Some(1));
        b.set_peer_claims([1].into_iter().collect());
        assert_eq!(b.pick_target(&s).and_then(|m| m.oid), Some(2));
    }
}
