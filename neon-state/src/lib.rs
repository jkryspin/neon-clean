//! Shared game-state schema.
//!
//! `neon-stream` produces one [`GameState`] per tick as a single JSON line;
//! `neon-loader` consumes them. Anything the producer could not read reliably
//! is `None` rather than `0`, so consumers never act on fabricated values.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facing {
    Left,
    Right,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub x: i32,
    pub y: i32,
    pub map_id: Option<u32>,
    pub level: Option<u16>,
    pub hp: Option<u32>,
    pub max_hp: Option<u32>,
    pub mp: Option<u32>,
    pub max_mp: Option<u32>,
    pub exp: Option<u64>,
    pub exp_needed: Option<u64>,
    pub meso: Option<u64>,
    pub available_ap: Option<u32>,
    pub facing: Option<Facing>,
    pub dead: Option<bool>,
    /// True when standing on a foothold; false while airborne. (Rope state untested.)
    #[serde(default)]
    pub grounded: Option<bool>,
    /// Job id from the server's stat updates (0 Beginner, 100 Warrior, ...), remembered per character.
    #[serde(default)]
    pub job: Option<u16>,
    /// Unspent skill points, from the server's stat updates (only known once one has carried them).
    #[serde(default)]
    pub sp: Option<u32>,
}

impl Player {
    pub fn hp_percent(&self) -> Option<f64> {
        pct(self.hp?, self.max_hp?)
    }

    pub fn mp_percent(&self) -> Option<f64> {
        pct(self.mp?, self.max_mp?)
    }
}

fn pct(cur: u32, max: u32) -> Option<f64> {
    (max > 0).then(|| (cur as f64 / max as f64 * 100.0).min(100.0))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mob {
    pub x: i32,
    pub y: i32,
    pub id: Option<u32>,
    pub hp_percent: Option<u8>,
    /// The server's object id of this mob (network source): stable for its whole life, gone when it dies.
    #[serde(default)]
    pub oid: Option<u32>,
    /// Milliseconds since the network last said where this mob is (spawn or move).
    #[serde(default)]
    pub age_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OtherPlayer {
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drop {
    pub x: i32,
    pub y: i32,
    pub is_meso: Option<bool>,
}

/// A platform segment (or wall, when x1 == x2). `x1 <= x2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Foothold {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl Foothold {
    /// Y of the segment at `x`, or None when x is outside its span / it is a wall.
    pub fn y_at(&self, x: i32) -> Option<f64> {
        if self.x1 == self.x2 || x < self.x1 || x > self.x2 {
            return None;
        }
        let t = (x - self.x1) as f64 / (self.x2 - self.x1) as f64;
        Some(self.y1 as f64 + t * (self.y2 - self.y1) as f64)
    }
}

/// True if (x, y) stands on one of `fh` within `tol` pixels.
pub fn on_foothold(fh: &[Foothold], x: i32, y: i32, tol: f64) -> bool {
    fh.iter().any(|f| f.y_at(x).map_or(false, |fy| (fy - y as f64).abs() <= tol))
}

/// Which parts of the state the producer could actually populate this tick.
/// Lets the bot refuse to run features whose inputs are missing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub position: bool,
    pub map_id: bool,
    pub level: bool,
    pub hp_mp: bool,
    pub mobs: bool,
    pub players: bool,
    pub drops: bool,
    pub exp: bool,
    pub ap: bool,
    /// True when max HP/MP are inferred from the highest value seen this run
    /// (no static root found yet). Percentages are optimistic until HP has
    /// been full once.
    #[serde(default)]
    pub max_estimated: bool,
    #[serde(default)]
    pub grounded: bool,
}

/// EXP needed to go from level N to N+1 (index 0 = level 1), MapleStory Classic World
/// (https://meowdb.com/msclassic/formulas/leveling). Level 7 -> 8 is 560, checked against the live screen.
pub const EXP_TO_NEXT: [u64; 40] = [
    15, 34, 57, 92, 135, 372, 560, 840, 1242, 1716, 2360, 3216, 4200, 5460, 7050, 8840, 11040, 13716, 16680, 20216,
    24402, 28980, 34320, 40512, 47216, 54900, 63666, 73080, 83720, 95700, 108480, 122760, 138666, 155540, 174216,
    194832, 216600, 240500, 266682, 294216,
];

/// EXP needed to leave `level`, if the table covers it.
pub fn exp_to_next(level: u16) -> Option<u64> {
    EXP_TO_NEXT.get((level as usize).checked_sub(1)?).copied()
}

/// EXP still missing to reach `target` from `level` with `exp` already earned in it.
pub fn exp_remaining(level: u16, exp: u64, target: u16) -> Option<u64> {
    if level >= target {
        return Some(0);
    }
    let mut total = exp_to_next(level)?.saturating_sub(exp);
    for l in level + 1..target {
        total += exp_to_next(l)?;
    }
    Some(total)
}

/// Conditions the stream noticed that the bot must react to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Alerts {
    /// An anti-macro check (lie detector) is active: stop acting and tell a human.
    #[serde(default)]
    pub lie_detector: bool,
    /// No packets from the game server for a long time: the connection is probably gone.
    #[serde(default)]
    pub connection_stalled: bool,
}

impl Alerts {
    pub fn is_default(&self) -> bool {
        *self == Alerts::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameState {
    /// Monotonic per-run sequence number; gaps mean dropped frames.
    pub seq: u64,
    /// Producer wall clock, ms since epoch.
    pub ts_ms: u64,
    pub caps: Capabilities,
    pub player: Player,
    pub mobs: Vec<Mob>,
    pub others: Vec<OtherPlayer>,
    /// Names of the members of our party (network source), so they are not treated as strangers.
    #[serde(default)]
    pub party: Vec<String>,
    pub drops: Vec<Drop>,
    #[serde(default, skip_serializing_if = "Alerts::is_default")]
    pub alerts: Alerts,
    /// Map geometry. Sent once when a map's scan completes, empty otherwise;
    /// the consumer keeps the latest non-empty list until the map changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footholds: Vec<Foothold>,
    /// The game camera: world coordinates of the view's top-left corner (x, y). World -> screen is
    /// `screen = world - camera` inside the 1366x768 game area; None when the camera chains do not read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<(i32, i32)>,
    /// Our quests as the server last reported them (status messages; remembered per character across restarts).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quests: Vec<QuestRecord>,
    /// Inventory slots the server has reported (remembered per character across restarts).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inventory: Vec<InvSlot>,
    /// Items picked up since the stream started (item id -> quantity), from the server's pickup messages.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub items_gained: std::collections::BTreeMap<u32, u32>,
}

/// One inventory slot as the server reported it: `tab` 1 equip, 2 use, 3 set-up, 4 etc; `slot` counts from 1, five per
/// row; `item` unknown for a stack that has only had its quantity reported.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InvSlot {
    pub tab: u8,
    pub slot: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<u32>,
    pub qty: u32,
}

/// One quest's state from the server: status 1 = in progress (with `progress`: three digits per mob the quest asks
/// for, in the order of its requirements, e.g. "001" = one Orange Mushroom), 2 = completed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestRecord {
    pub id: u32,
    pub status: u8,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub progress: String,
}

impl QuestRecord {
    /// Kills so far of the quest's n-th required mob.
    pub fn kills(&self, n: usize) -> Option<u32> {
        self.progress.get(n * 3..n * 3 + 3)?.parse().ok()
    }
}

impl GameState {
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("GameState serializes")
    }

    pub fn from_line(line: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(line)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn exp_table_matches_the_live_client() {
        assert_eq!(exp_to_next(7), Some(560), "level 7 needs 560 (78 exp = 13.93% on screen)");
        assert_eq!(exp_to_next(1), Some(15));
        assert_eq!(exp_to_next(0), None);
        assert_eq!(exp_to_next(41), None);
    }

    #[test]
    fn remaining_exp_to_a_target_level() {
        // Level 7 with 461 earned: 99 to finish 7, then all of 8 and 9.
        assert_eq!(exp_remaining(7, 461, 10), Some(99 + 840 + 1242));
        assert_eq!(exp_remaining(7, 100, 8), Some(460));
        assert_eq!(exp_remaining(10, 5, 10), Some(0));
        assert_eq!(exp_remaining(39, 0, 45), None, "beyond the table");
    }

    use super::*;

    #[test]
    fn roundtrip_is_single_line() {
        let mut s = GameState::default();
        s.player.hp = Some(10);
        s.mobs.push(Mob { x: 1, y: 2, id: None, hp_percent: None, oid: None, age_ms: None });
        let line = s.to_line();
        assert!(!line.contains('\n'));
        assert_eq!(GameState::from_line(&line).unwrap(), s);
    }

    #[test]
    fn foothold_height_and_membership() {
        let slope = Foothold { x1: 0, y1: 0, x2: 100, y2: -50 };
        assert_eq!(slope.y_at(50), Some(-25.0));
        assert_eq!(slope.y_at(101), None);
        let wall = Foothold { x1: 10, y1: 0, x2: 10, y2: 80 };
        assert_eq!(wall.y_at(10), None, "walls are not floors");
        let fh = [slope, Foothold { x1: 200, y1: 274, x2: 400, y2: 274 }];
        assert!(on_foothold(&fh, 300, 276, 4.0));
        assert!(!on_foothold(&fh, 300, 100, 4.0), "floating in the air");
        assert!(!on_foothold(&fh, 150, 274, 4.0), "no segment under x=150");
    }

    #[test]
    fn footholds_omitted_from_json_when_empty() {
        let s = GameState::default();
        assert!(!s.to_line().contains("footholds"));
        let mut s2 = GameState::default();
        s2.footholds.push(Foothold { x1: 0, y1: 1, x2: 2, y2: 3 });
        assert!(GameState::from_line(&s2.to_line()).unwrap().footholds.len() == 1);
    }

    #[test]
    fn percent_none_when_unknown() {
        let p = Player { hp: Some(5), ..Default::default() };
        assert_eq!(p.hp_percent(), None);
        let p = Player { hp: Some(5), max_hp: Some(10), ..Default::default() };
        assert_eq!(p.hp_percent(), Some(50.0));
    }
}
