//! Event-sourced world model.
//!
//! Packets (decoded through the `neon-protocol` registry) become typed [`Event`]s; [`World::apply`] folds them
//! into state; [`World::snapshot`] is what the view API serves. A packet log can be replayed offline with
//! [`replay`], so the model is testable without a live client.

pub mod decode;
pub mod record;
use neon_protocol::{Dir, Registry};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    MobSpawn { oid: u32, template: u32, x: i32, y: i32, hp: Option<u32> },
    MobMove { oid: u32, x: i32, y: i32 },
    MobKill { oid: u32 },
    PlayerSpawn { char_id: u32, name: String, level: u32, equips: Vec<(u8, u32)> },
    PlayerMove { char_id: u32, x: i32, y: i32 },
    /// Our own movement, read from the packet we send (the server never echoes it to us).
    SelfMove { x: i32, y: i32, tick: u32 },
    /// An NPC walked (object id, end of its last fragment).
    NpcMove { oid: u32, x: i32, y: i32 },
    /// A drop of ours appeared: an item (`item`) or mesos (`mesos`).
    DropSpawn { oid: u32, owner: u32, x: i32, y: i32, item: Option<u32>, mesos: Option<u32> },
    DropRemove { oid: u32 },
    /// We entered a map (set-field).
    MapEnter { map: u32 },
    /// Own stat update; only the mask is read by the registry (values depend on the mask).
    StatMask { mask: u32 },
    /// Own stat update with decoded values (what changed).
    Stats(decode::StatUpdate),
    Party(decode::PartyMsg),
    Pickup { item: u32, qty: u32 },
    /// A line in the chat window (kind byte as sent; the megaphone kinds carry `Name : message`).
    Chat { kind: u8, text: String },
    /// An EXP gain message (our own kill, or a party share).
    ExpGain { amount: u32 },
    /// One inventory slot: `item` known for a new stack, `qty` None when removed.
    Inventory { tab: u8, slot: u16, item: Option<u32>, qty: Option<u32> },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Mob {
    pub template: Option<u32>,
    /// HP when it spawned (the spawn packet carries it).
    pub hp: Option<u32>,
    pub x: i32,
    pub y: i32,
    pub last_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Player {
    pub name: Option<String>,
    pub level: Option<u32>,
    /// (slot, item id) of what they wear, from their spawn packet.
    pub equips: Vec<(u8, u32)>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub last_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Npc {
    pub x: i32,
    pub y: i32,
    pub last_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Drop {
    pub item: Option<u32>,
    pub mesos: Option<u32>,
    pub owner: u32,
    pub x: i32,
    pub y: i32,
    pub last_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChatLine {
    pub t_ms: u64,
    pub kind: u8,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FeedItem {
    pub t_ms: u64,
    pub text: String,
}

/// Our own character, as far as stat updates have told us.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Me {
    /// Position and state read from the client's memory (`World::apply_mem`), not from packets.
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub grounded: Option<bool>,
    pub dead: Option<bool>,
    pub hp: Option<u32>,
    pub max_hp: Option<u32>,
    pub mp: Option<u32>,
    pub max_mp: Option<u32>,
    pub exp: Option<u32>,
    pub level: Option<u32>,
    pub ap: Option<u32>,
    pub sp: Option<u32>,
    pub job: Option<u32>,
    pub meso: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Snapshot {
    pub me: Me,
    pub party_id: Option<u32>,
    pub party: Vec<decode::PartyMember>,
    /// Most recent pickups, newest last (item id, quantity).
    pub pickups: Vec<(u32, u32)>,
    /// Inventory slots we have heard about: "tab:slot" -> (item if known, quantity).
    pub inventory: BTreeMap<String, (Option<u32>, Option<u32>)>,
    pub map: Option<u32>,
    /// EXP messages seen and their sum since the view started (one message per kill of ours).
    pub exp_messages: u64,
    pub exp_gained: u64,
    /// Chat lines, newest last (at most 60).
    pub chat: Vec<ChatLine>,
    /// Notable things that happened to us, newest last (at most 40).
    pub feed: Vec<FeedItem>,
    pub events: u64,
    pub now_ms: u64,
    pub mobs: BTreeMap<u32, Mob>,
    pub players: BTreeMap<u32, Player>,
    pub drops: BTreeMap<u32, Drop>,
    /// NPCs that have walked since we entered the map (standing NPCs are not announced by anything we decode yet).
    pub npcs: BTreeMap<u32, Npc>,
}

#[derive(Debug, Clone, Default)]
pub struct World {
    s: Snapshot,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    fn note(&mut self, now_ms: u64, text: String) {
        self.s.feed.push(FeedItem { t_ms: now_ms, text });
        if self.s.feed.len() > 40 {
            self.s.feed.remove(0);
        }
    }

    pub fn apply(&mut self, e: &Event, now_ms: u64) {
        match e {
            Event::ExpGain { amount } => self.note(now_ms, format!("+{amount} EXP")),
            Event::Pickup { item, qty } => self.note(now_ms, format!("picked up item {item} x{qty}")),
            Event::MapEnter { map } => self.note(now_ms, format!("entered map {map}")),
            Event::Stats(u) => {
                if let (Some(new), Some(old)) = (u.level, self.s.me.level) {
                    if new > old {
                        self.note(now_ms, format!("level up: {old} -> {new}"));
                    }
                }
                if let Some(m) = u.meso {
                    if let Some(old) = self.s.me.meso {
                        if m != old {
                            self.note(now_ms, format!("mesos {old} -> {m}"));
                        }
                    }
                }
            }
            Event::Party(decode::PartyMsg::List { members, .. }) => self.note(now_ms, format!("party: {} member(s)", members.len())),
            Event::Party(decode::PartyMsg::Left { .. }) => self.note(now_ms, "party: someone left".to_string()),
            _ => {}
        }
        let s = &mut self.s;
        s.events += 1;
        s.now_ms = now_ms;
        match e {
            Event::MobSpawn { oid, template, x, y, hp } => {
                s.mobs.insert(*oid, Mob { template: Some(*template), hp: *hp, x: *x, y: *y, last_ms: now_ms });
            }
            Event::MobMove { oid, x, y } => {
                // A move can arrive for a mob that spawned before we joined: template unknown.
                let m = s.mobs.entry(*oid).or_insert(Mob { template: None, hp: None, x: *x, y: *y, last_ms: now_ms });
                m.x = *x;
                m.y = *y;
                m.last_ms = now_ms;
            }
            Event::MobKill { oid } => {
                s.mobs.remove(oid);
            }
            Event::PlayerSpawn { char_id, name, level, equips } => {
                let p = s.players.entry(*char_id).or_insert(Player { name: None, level: None, equips: vec![], x: None, y: None, last_ms: now_ms });
                p.name = Some(name.clone());
                p.equips = equips.clone();
                p.level = Some(*level).filter(|l| *l > 0);
                p.last_ms = now_ms;
            }
            Event::PlayerMove { char_id, x, y } => {
                let p = s.players.entry(*char_id).or_insert(Player { name: None, level: None, equips: vec![], x: None, y: None, last_ms: now_ms });
                p.x = Some(*x);
                p.y = Some(*y);
                p.last_ms = now_ms;
            }
            Event::SelfMove { x, y, .. } => {
                s.me.x = Some(*x);
                s.me.y = Some(*y);
            }
            Event::NpcMove { oid, x, y } => {
                s.npcs.insert(*oid, Npc { x: *x, y: *y, last_ms: now_ms });
            }
            Event::DropSpawn { oid, owner, x, y, item, mesos } => {
                s.drops.insert(*oid, Drop { item: *item, mesos: *mesos, owner: *owner, x: *x, y: *y, last_ms: now_ms });
            }
            Event::DropRemove { oid } => {
                s.drops.remove(oid);
            }
            Event::MapEnter { map } => {
                // Everything of the map we left is gone; the new map's spawns follow.
                s.mobs.clear();
                s.players.clear();
                s.drops.clear();
                s.npcs.clear();
                s.map = Some(*map);
            }
            Event::StatMask { .. } => {}
            Event::Stats(u) => {
                macro_rules! set { ($f:ident) => { if u.$f.is_some() { s.me.$f = u.$f; } }; }
                set!(hp); set!(max_hp); set!(mp); set!(max_mp); set!(exp); set!(level); set!(ap); set!(sp); set!(job); set!(meso);
            }
            Event::Party(decode::PartyMsg::List { id, members }) => {
                s.party_id = Some(*id);
                s.party = members.clone();
            }
            Event::Party(decode::PartyMsg::Left { char_id }) => {
                // Someone left; if it was one of the members drop them, and an emptied party is gone. (When *we* leave,
                // the server sends our own id: the list shrinks the same way, and a lone member's departure disbands it.)
                s.party.retain(|m| m.char_id != *char_id);
                if s.party.len() <= 1 {
                    s.party.clear();
                    s.party_id = None;
                }
            }
            Event::Chat { kind, text } => {
                s.chat.push(ChatLine { t_ms: now_ms, kind: *kind, text: text.clone() });
                if s.chat.len() > 60 {
                    s.chat.remove(0);
                }
            }
            Event::ExpGain { amount } => {
                s.exp_messages += 1;
                s.exp_gained += u64::from(*amount);
            }
            Event::Pickup { item, qty } => {
                s.pickups.push((*item, *qty));
                if s.pickups.len() > 50 {
                    s.pickups.remove(0);
                }
            }
            Event::Inventory { tab, slot, item, qty } => {
                let key = format!("{tab}:{slot}");
                match qty {
                    None => {
                        s.inventory.remove(&key);
                    }
                    Some(_) => {
                        let known = item.or_else(|| s.inventory.get(&key).and_then(|e| e.0));
                        s.inventory.insert(key, (known, *qty));
                    }
                }
            }
        }
    }

    /// Merge one `neon-stream` state line (JSON with a `player` object): position, HP/MP, level, grounded, dead.
    /// Null fields are ignored, so a source that cannot read something does not erase what packets told us; max HP/MP from
    /// memory are estimates and never override a packet-derived value.
    pub fn apply_mem(&mut self, v: &serde_json::Value, now_ms: u64) {
        let Some(p) = v.get("player") else { return };
        let me = &mut self.s.me;
        let num = |k: &str| p.get(k).and_then(|x| x.as_i64());
        let set_i32 = |dst: &mut Option<i32>, v: Option<i64>| if let Some(n) = v { *dst = Some(n as i32) };
        let set_u32 = |dst: &mut Option<u32>, v: Option<i64>| if let Some(n) = v { if n >= 0 { *dst = Some(n as u32) } };
        set_i32(&mut me.x, num("x"));
        set_i32(&mut me.y, num("y"));
        set_u32(&mut me.hp, num("hp"));
        // Max HP/MP from memory are estimates (remembered per level): a packet stat update gives the exact value, so
        // memory only fills them in when no packet has.
        if me.max_hp.is_none() { set_u32(&mut me.max_hp, num("max_hp")); }
        set_u32(&mut me.mp, num("mp"));
        if me.max_mp.is_none() { set_u32(&mut me.max_mp, num("max_mp")); }
        set_u32(&mut me.level, num("level"));
        if let Some(b) = p.get("grounded").and_then(|x| x.as_bool()) { me.grounded = Some(b) }
        if let Some(b) = p.get("dead").and_then(|x| x.as_bool()) { me.dead = Some(b) }
        if let Some(m) = num("map_id").filter(|m| *m > 0) { self.s.map = Some(m as u32) }
        self.s.now_ms = self.s.now_ms.max(now_ms);
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

/// Where a movement packet leaves the entity: the end of its last 21-byte fragment, else the packet's start position.
fn end_position(d: &neon_protocol::Decoded) -> Option<(i32, i32)> {
    let n = d.fields.iter().filter(|(k, _)| k.starts_with("move[") && k.ends_with("].x")).count();
    if n > 0 {
        let x = int(d, &format!("move[{}].x", n - 1))?;
        let y = int(d, &format!("move[{}].y", n - 1))?;
        return Some((x as i32, y as i32));
    }
    Some((int(d, "x")? as i32, int(d, "y")? as i32))
}

/// Turn a plaintext server->client packet into an event, using the registry for every verified layout.
/// Returns None for opcodes the world model does not use.
pub fn event_from_packet(reg: &Registry, plain: &[u8]) -> Option<Event> {
    // 0x01a9 is two packets: a very long login inventory, and (41..1000 bytes) set-field with the map id at @36.
    if plain.len() >= 41 && plain.len() <= 1000 && plain[0] == 0xa9 && plain[1] == 0x01 {
        let map = u32::from_le_bytes(plain[36..40].try_into().ok()?);
        return (1..=999_999_999).contains(&map).then_some(Event::MapEnter { map });
    }
    if plain.len() >= 2 {
        match u16::from_le_bytes([plain[0], plain[1]]) {
            0x007e => return decode::parse_stats(plain).map(Event::Stats).or_else(|| event_from_registry(reg, plain)),
            0x00a7 => return decode::parse_party(plain).map(Event::Party),
            0x008b => {
                return decode::parse_pickup(plain)
                    .map(|(item, qty)| Event::Pickup { item, qty })
                    .or_else(|| decode::parse_exp_gain(plain).map(|amount| Event::ExpGain { amount }))
            }
            0x0072 => return decode::parse_inventory(plain).map(|(tab, slot, item, qty)| Event::Inventory { tab, slot, item, qty }),
            _ => {}
        }
    }
    event_from_registry(reg, plain)
}

/// The world-relevant meaning of a packet *we sent*. Today only our own movement: it is the only way to know where we are
/// without reading the client's memory.
pub fn event_from_out_packet(reg: &Registry, plain: &[u8]) -> Option<Event> {
    let d = reg.decode(Dir::Out, plain)?;
    match d.name.as_deref()? {
        "MoveSelf" => Some(Event::SelfMove { x: int(&d, "x")? as i32, y: int(&d, "y")? as i32, tick: int(&d, "tick")? as u32 }),
        _ => None,
    }
}

fn event_from_registry(reg: &Registry, plain: &[u8]) -> Option<Event> {
    let d = reg.decode(Dir::In, plain)?;
    let name = d.name.as_deref()?;
    Some(match name {
        "MobSpawn" | "MobControl" => Event::MobSpawn {
            oid: int(&d, "oid")? as u32,
            template: int(&d, "template")? as u32,
            x: int(&d, "x")? as i32,
            y: int(&d, "y")? as i32,
            hp: int(&d, "hp").map(|v| v as u32),
        },
        "MobMove" => {
            let (x, y) = end_position(&d)?;
            Event::MobMove { oid: int(&d, "oid")? as u32, x, y }
        }
        "MobKill" => Event::MobKill { oid: int(&d, "oid")? as u32 },
        "PlayerSpawn" => {
            let neon_protocol::Value::Str(n) = d.get("name")? else { return None };
            Event::PlayerSpawn { char_id: int(&d, "char_id")? as u32, name: n.clone(), level: int(&d, "level")? as u32, equips: decode::parse_equips(plain) }
        }
        "PlayerMove" => {
            let (x, y) = end_position(&d)?;
            Event::PlayerMove { char_id: int(&d, "char_id")? as u32, x, y }
        }
        "Chat" => {
            let neon_protocol::Value::Str(t) = d.get("text")? else { return None };
            Event::Chat { kind: int(&d, "kind")? as u8, text: t.clone() }
        }
        "NpcMove" => {
            let (x, y) = end_position(&d)?;
            Event::NpcMove { oid: int(&d, "oid")? as u32, x, y }
        }
        "DropSpawnItem" | "DropSpawnMeso" => Event::DropSpawn {
            oid: int(&d, "oid")? as u32,
            owner: int(&d, "owner")? as u32,
            x: int(&d, "x")? as i32,
            y: int(&d, "y")? as i32,
            item: int(&d, "item").map(|v| v as u32),
            mesos: int(&d, "amount").map(|v| v as u32),
        },
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
