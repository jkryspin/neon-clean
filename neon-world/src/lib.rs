//! Event-sourced world model.
//!
//! Packets (decoded through the `neon-protocol` registry) become typed [`Event`]s; [`World::apply`] folds them
//! into state; [`World::snapshot`] is what the view API serves. A packet log can be replayed offline with
//! [`replay`], so the model is testable without a live client.

use neon_protocol::{Dir, Registry};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    MobSpawn { oid: u32, template: u32, x: i32, y: i32 },
    MobMove { oid: u32, x: i32, y: i32 },
    MobKill { oid: u32 },
    PlayerSpawn { char_id: u32, name: String },
    PlayerMove { char_id: u32, x: i32, y: i32 },
    DropSpawn { oid: u32, owner: u32, x: i32, y: i32 },
    DropRemove { oid: u32 },
    /// We entered a map (set-field).
    MapEnter { map: u32 },
    /// Own stat update; only the mask is read by the registry (values depend on the mask).
    StatMask { mask: u32 },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Mob {
    pub template: Option<u32>,
    pub x: i32,
    pub y: i32,
    pub last_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Player {
    pub name: Option<String>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub last_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Drop {
    pub owner: u32,
    pub x: i32,
    pub y: i32,
    pub last_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Snapshot {
    pub map: Option<u32>,
    pub events: u64,
    pub now_ms: u64,
    pub mobs: BTreeMap<u32, Mob>,
    pub players: BTreeMap<u32, Player>,
    pub drops: BTreeMap<u32, Drop>,
}

#[derive(Debug, Clone, Default)]
pub struct World {
    s: Snapshot,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, e: &Event, now_ms: u64) {
        let s = &mut self.s;
        s.events += 1;
        s.now_ms = now_ms;
        match e {
            Event::MobSpawn { oid, template, x, y } => {
                s.mobs.insert(*oid, Mob { template: Some(*template), x: *x, y: *y, last_ms: now_ms });
            }
            Event::MobMove { oid, x, y } => {
                // A move can arrive for a mob that spawned before we joined: template unknown.
                let m = s.mobs.entry(*oid).or_insert(Mob { template: None, x: *x, y: *y, last_ms: now_ms });
                m.x = *x;
                m.y = *y;
                m.last_ms = now_ms;
            }
            Event::MobKill { oid } => {
                s.mobs.remove(oid);
            }
            Event::PlayerSpawn { char_id, name } => {
                let p = s.players.entry(*char_id).or_insert(Player { name: None, x: None, y: None, last_ms: now_ms });
                p.name = Some(name.clone());
                p.last_ms = now_ms;
            }
            Event::PlayerMove { char_id, x, y } => {
                let p = s.players.entry(*char_id).or_insert(Player { name: None, x: None, y: None, last_ms: now_ms });
                p.x = Some(*x);
                p.y = Some(*y);
                p.last_ms = now_ms;
            }
            Event::DropSpawn { oid, owner, x, y } => {
                s.drops.insert(*oid, Drop { owner: *owner, x: *x, y: *y, last_ms: now_ms });
            }
            Event::DropRemove { oid } => {
                s.drops.remove(oid);
            }
            Event::MapEnter { map } => {
                // Everything of the map we left is gone; the new map's spawns follow.
                s.mobs.clear();
                s.players.clear();
                s.drops.clear();
                s.map = Some(*map);
            }
            Event::StatMask { .. } => {}
        }
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.s
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.s).expect("snapshot serializes")
    }
}

fn int(d: &neon_protocol::Decoded, name: &str) -> Option<i64> {
    d.get(name)?.as_i64()
}

/// Turn a plaintext server->client packet into an event, using the registry for every verified layout.
/// Returns None for opcodes the world model does not use.
pub fn event_from_packet(reg: &Registry, plain: &[u8]) -> Option<Event> {
    // 0x01a9 is two packets: a very long login inventory, and (41..1000 bytes) set-field with the map id at @36.
    if plain.len() >= 41 && plain.len() <= 1000 && plain[0] == 0xa9 && plain[1] == 0x01 {
        let map = u32::from_le_bytes(plain[36..40].try_into().ok()?);
        return (1..=999_999_999).contains(&map).then_some(Event::MapEnter { map });
    }
    let d = reg.decode(Dir::In, plain)?;
    let name = d.name.as_deref()?;
    Some(match name {
        "MobSpawn" => Event::MobSpawn { oid: int(&d, "oid")? as u32, template: int(&d, "template")? as u32, x: int(&d, "x")? as i32, y: int(&d, "y")? as i32 },
        "MobMove" => Event::MobMove { oid: int(&d, "oid")? as u32, x: int(&d, "x")? as i32, y: int(&d, "y")? as i32 },
        "MobKill" => Event::MobKill { oid: int(&d, "oid")? as u32 },
        "PlayerSpawn" => {
            let neon_protocol::Value::Str(n) = d.get("name")? else { return None };
            Event::PlayerSpawn { char_id: int(&d, "char_id")? as u32, name: n.clone() }
        }
        "PlayerMove" => Event::PlayerMove { char_id: int(&d, "char_id")? as u32, x: int(&d, "x")? as i32, y: int(&d, "y")? as i32 },
        "DropSpawn" => Event::DropSpawn { oid: int(&d, "oid")? as u32, owner: int(&d, "owner")? as u32, x: int(&d, "x")? as i32, y: int(&d, "y")? as i32 },
        "DropRemove" => Event::DropRemove { oid: int(&d, "oid")? as u32 },
        "StatUpdate" => Event::StatMask { mask: int(&d, "mask")? as u32 },
        _ => return None,
    })
}

/// One line of a stamped packet log: `<unix seconds> op <hex4> len <n>: <hex bytes separated by spaces>`
/// (what `NEON_DUMP_OPS=all neon-stream` prints, with a timestamp prefixed by the capture helper).
pub fn parse_log_line(line: &str) -> Option<(f64, Vec<u8>)> {
    let (ts, rest) = line.split_once(' ')?;
    let rest = rest.strip_prefix("op ")?;
    let (_, bytes) = rest.split_once(": ").or_else(|| rest.split_once(':'))?;
    let plain: Option<Vec<u8>> = bytes.split_whitespace().map(|h| u8::from_str_radix(h, 16).ok()).collect();
    Some((ts.parse().ok()?, plain?))
}

/// Replay a packet log into a fresh world. Returns the world and how many lines were not understood.
pub fn replay<I: IntoIterator<Item = String>>(reg: &Registry, lines: I) -> (World, ReplayStats) {
    let mut w = World::new();
    let mut st = ReplayStats::default();
    let mut t0: Option<f64> = None;
    for line in lines {
        let Some((t, plain)) = parse_log_line(&line) else {
            st.bad_lines += 1;
            continue;
        };
        st.packets += 1;
        let t0 = *t0.get_or_insert(t);
        match event_from_packet(reg, &plain) {
            Some(e) => {
                st.events += 1;
                w.apply(&e, ((t - t0) * 1000.0) as u64);
            }
            None => st.unused += 1,
        }
    }
    (w, st)
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ReplayStats {
    pub packets: u64,
    pub events: u64,
    pub unused: u64,
    pub bad_lines: u64,
}
