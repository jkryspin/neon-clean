//! Passive decode of the MapleStory Classic World game connection.
//!
//! Source: `tcpdump -w -` (pcap) of the client's TCP connection. Nothing touches
//! the game process. What was verified against a live client:
//!
//! * Framing: every message is `[4-byte header][payload]`; `len = a ^ b` where `a`
//!   and `b` are the little-endian u16 halves of the header (valid for any length).
//! * Payload "cipher": every byte is `plain + K` (mod 256) for one constant `K` per packet, `K` being the first byte of a
//!   32-bit receive IV that advances by the classic shuffle after each packet (exact decryption in `recv.rs`, found
//!   2026-10-26). Until that IV has been learned (12 frames after the start of a capture, or after lost bytes) the older
//!   heuristic is used: per block (1456 bytes first, then 1460) the most common byte is K for zero. That heuristic
//!   fails when a block is not zero-dominated (23 of 373 real frames).
//! * Opcodes (offsets into the decoded payload, little endian):
//!   * `0x03c9` mob spawn:   oid u32@3, template u32@8, x i16@206, y i16@208
//!   * `0x03dc` mob update:  oid u32@2, x i16@30,  y i16@32
//!   * `0x03d4` mob removed: oid u32@2
//!   * `0x022d` player spawn: char id u32@6, name len u16@18, name@20
//!   * `0x029d` player move:  char id u32@2, x i16@10, y i16@12
//!   * `0x0471` drop spawn:   oid u32@4, owner char id u32@25, x i16@30, y i16@32 (sent twice)
//!   * `0x0472` drop removed: oid u32@2 (picked up or expired)
//!   * `0x0072` inventory update, one entry @9: mode u8 (0 add: tab u8@10, slot u16@11, item u32@14, quantity u16@32;
//!     1 quantity: tab@10, slot@11, quantity u16@13; 3 remove: tab@10, slot@11); slots count from 1, five per row
//!   * `0x01a9` set field (we entered a map, 103 bytes): map id u32@36, spawn point u8@40. At login it is the whole
//!     character (~3 KB): every Use/Set-up/Etc stack is a 43-byte record around the "never expires" time
//!     `00 80 05 bb 46 e6 17 02` at e: slot u16@e-8, type u8@e-6 (2), item u32@e-5, quantity u16@e+13
//!   * `0x008b` status message; type u8@2: 1 = quest record: quest id u32@3, status u8@7, then for status 1 the kill
//!     counts as a string (u16 length@8, three digits per required mob), e.g. 1010 in progress "001"
//!   * `0x007e` own stat update: mask u32@5 (HP 0x400, EXP 0x10000, ...), u32 values from @9
//!
//! Mob template IDs are the small decimals documented at meowdb.com/msclassic
//! (2 Snail, 3 Blue Snail, 5 Red Snail, 9 Orange Mushroom, ...).

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Read};

/// Assumed length of one mob move, ms (fragment durations are not decoded).
const MOVE_MS: f32 = 800.0;
pub const PORT_MIN: u16 = 8500;
pub const PORT_MAX: u16 = 8700;
const MAX_SYNC_LEN: usize = 8000;
const MAX_PACKET_LEN: usize = 16_000;

#[derive(Debug, Clone, PartialEq)]
pub enum NetEvent {
    MobSpawn { oid: u32, template: u32, x: i32, y: i32 },
    /// `x, y` is where the move starts; `to_x` is where its last fragment ends (equal when unknown).
    MobMove { oid: u32, x: i32, y: i32, to_x: i32 },
    MobKill { oid: u32 },
    PlayerSpawn { char_id: u32, name: String },
    PlayerMove { char_id: u32, x: i32, y: i32 },
    DropSpawn { oid: u32, owner: u32, x: i32, y: i32 },
    DropRemove { oid: u32 },
    /// We entered a map (the server's set-field on every portal, revive or warp).
    MapEnter { map: u32 },
    /// A quest of ours changed (status message type 1): status 1 in progress (+ kill counts), 2 completed.
    Quest { id: u32, status: u8, progress: String },
    /// Own stat update. Only the bits whose size was verified are decoded.
    Stats(StatUpdate),
    /// We picked up an item (status message type 0, mode 0): its id and how many.
    Pickup { item: u32, qty: u32 },
    /// The whole Use/Set-up/Etc inventory at login (the long 0x01a9): (tab, slot, item, quantity).
    InventoryAll(Vec<(u8, u16, u32, u32)>),
    /// One inventory slot changed (0x0072): `item` is known when a new stack was added, `qty` None means removed.
    Inventory { tab: u8, slot: u16, item: Option<u32>, qty: Option<u32> },
    /// A packet whose opcode was registered as an alert (e.g. the lie detector).
    Alert { kind: AlertKind },
    /// Our party as the server just described it (0x00a7 modes 0x0e create, 0x13 member joined).
    Party { id: u32, members: Vec<PartyMember> },
    /// A member left or the party was disbanded (0x00a7 mode 0x10): the character id.
    PartyLeft { char_id: u32 },
}

/// One party member as listed in a party update.
#[derive(Debug, Clone, PartialEq)]
pub struct PartyMember {
    pub char_id: u32,
    pub name: String,
    pub job: u16,
    pub level: u32,
    /// Zero-based channel (the 35th channel is 34).
    pub channel: u32,
}

/// Members in a 0x00a7 party list. Each is `char id u32, name length u16, name, job u16`, with the level `8` bytes
/// after the end of the name and the channel `12` (verified live with two characters, 2026-10-26); a large
/// per-member block follows that is not decoded. The list is found by scanning for that shape, since the packet's
/// own header differs between modes and the decoder's per-block key is only an estimate.
pub fn parse_party_members(p: &[u8]) -> Vec<PartyMember> {
    let mut out: Vec<PartyMember> = vec![];
    let mut i = 12;
    while i + 24 < p.len() {
        let (Some(id), Some(n)) = (u32le(p, i), u16le(p, i + 4)) else { break };
        let n = n as usize;
        let ok_id = (1..=0x00ff_ffff).contains(&id);
        if ok_id && (3..=13).contains(&n) {
            let end = i + 6 + n;
            if let Some(name) = p.get(i + 6..end).filter(|s| s.iter().all(|c| c.is_ascii_alphanumeric())) {
                if let (Some(job), Some(level), Some(channel)) = (u16le(p, end), u32le(p, end + 8), u32le(p, end + 12)) {
                    if (1..=250).contains(&level) && channel < 64 && job < 4000 && !out.iter().any(|m| m.char_id == id) {
                        out.push(PartyMember { char_id: id, name: String::from_utf8_lossy(name).into_owned(), job, level, channel });
                        i = end + 16;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// The fields of one own stat update (each packet carries only what changed).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatUpdate {
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

impl StatUpdate {
    fn is_empty(&self) -> bool {
        *self == StatUpdate::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    LieDetector,
}

/// What we have seen of one server->client opcode (for finding the ones we do not know yet).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct OpStat {
    pub count: u64,
    pub min_len: usize,
    pub max_len: usize,
    pub total_len: u64,
}

fn u16le(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]))
}
fn i16le(d: &[u8], o: usize) -> Option<i32> {
    Some(i16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]) as i32)
}
fn u32le(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?]))
}

fn mode_byte(blk: &[u8]) -> u8 {
    let mut counts = [0u32; 256];
    for &b in blk {
        counts[b as usize] += 1;
    }
    counts.iter().enumerate().max_by_key(|(_, c)| **c).map(|(i, _)| i as u8).unwrap_or(0)
}

/// The constant for a packet's first block. Plain zeros usually dominate, so the most common
/// byte is the constant, but not for small or number-heavy packets (e.g. a block full of
/// 0xFD = -3). Every opcode seen has a high byte of 0..=4, which gives five candidates
/// (`raw[1] - h`); of the candidates satisfying that, pick the one producing the most zeros.
fn first_block_constant(blk: &[u8]) -> u8 {
    let mode = mode_byte(blk);
    if blk.len() < 2 {
        return mode;
    }
    let mut best: Option<(usize, u8)> = None;
    let cands = std::iter::once(mode).chain((0u8..=4).map(|h| blk[1].wrapping_sub(h)));
    for k in cands {
        if blk[1].wrapping_sub(k) > 4 {
            continue;
        }
        let zeros = blk.iter().filter(|b| b.wrapping_sub(k) == 0).count();
        if best.map_or(true, |(z, _)| zeros > z) {
            best = Some((zeros, k));
        }
    }
    best.map_or(mode, |(_, k)| k)
}

/// Undo the per-block additive constant. See the module docs for the caveat.
pub fn decode_payload(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut pos = 0;
    let mut first = true;
    while pos < body.len() {
        let n = (body.len() - pos).min(if first { 1456 } else { 1460 });
        let blk = &body[pos..pos + n];
        let k = if first { first_block_constant(blk) } else { mode_byte(blk) };
        out.extend(blk.iter().map(|b| b.wrapping_sub(k)));
        pos += n;
        first = false;
    }
    out
}

/// Interpret one decoded payload (starting with its 2-byte opcode).
/// Where a mob update's move ends. The packet lists movement fragments after the start point;
/// each carries an (x, y) pair. The last pair that stays on the mob's floor (same y, within a
/// plausible distance) is the destination. Falls back to the start.
fn move_end_x(p: &[u8], x: i32, y: i32) -> i32 {
    let mut end = x;
    let mut i = 38;
    while i + 4 <= p.len() {
        if let (Some(fx), Some(fy)) = (i16le(p, i), i16le(p, i + 2)) {
            if (fy - y).abs() <= 30 && (fx - x).abs() <= 500 && fx != 0 {
                end = fx;
            }
        }
        i += 1;
    }
    end
}

pub fn parse_event(p: &[u8]) -> Option<NetEvent> {
    let op = u16le(p, 0)?;
    match op {
        0x03c9 if p.len() >= 210 => {
            let (oid, template) = (u32le(p, 3)?, u32le(p, 8)?);
            let (x, y) = (i16le(p, 206)?, i16le(p, 208)?);
            // Reject garbage from a mis-estimated constant: ids are small, coords are bounded.
            if !(1..=1_000_000).contains(&template) || x.abs() > 20_000 || y.abs() > 20_000 {
                return None;
            }
            Some(NetEvent::MobSpawn { oid, template, x, y })
        }
        0x03dc if p.len() >= 34 => {
            let (x, y) = (i16le(p, 30)?, i16le(p, 32)?);
            if x.abs() > 20_000 || y.abs() > 20_000 {
                return None;
            }
            Some(NetEvent::MobMove { oid: u32le(p, 2)?, x, y, to_x: move_end_x(p, x, y) })
        }
        0x03d4 if p.len() >= 6 => Some(NetEvent::MobKill { oid: u32le(p, 2)? }),
        0x00a7 if p.len() == 8 && p[2] == 0x10 => Some(NetEvent::PartyLeft { char_id: u32le(p, 3)? }),
        0x00a7 if p.len() > 100 && (p[2] == 0x0e || p[2] == 0x13) => {
            let members = parse_party_members(p);
            // The party id sits at 3 in a create and after the joiner's name in a join.
            let id = if p[2] == 0x0e { u32le(p, 3)? } else { u32le(p, 5 + u16le(p, 3)? as usize)? };
            (!members.is_empty()).then_some(NetEvent::Party { id, members })
        }
        0x022d if p.len() >= 24 => {
            let n = u16le(p, 18)? as usize;
            let name = p.get(20..20 + n)?;
            if !(1..=16).contains(&n) || !name.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                return None;
            }
            Some(NetEvent::PlayerSpawn { char_id: u32le(p, 6)?, name: String::from_utf8_lossy(name).into_owned() })
        }
        0x029d if p.len() >= 14 => {
            let (x, y) = (i16le(p, 10)?, i16le(p, 12)?);
            if x.abs() > 20_000 || y.abs() > 20_000 {
                return None;
            }
            Some(NetEvent::PlayerMove { char_id: u32le(p, 2)?, x, y })
        }
        0x0471 if p.len() >= 40 => {
            let (x, y) = (i16le(p, 30)?, i16le(p, 32)?);
            if x.abs() > 20_000 || y.abs() > 20_000 {
                return None;
            }
            Some(NetEvent::DropSpawn { oid: u32le(p, 4)?, owner: u32le(p, 25)?, x, y })
        }
        0x0472 if p.len() >= 6 => Some(NetEvent::DropRemove { oid: u32le(p, 2)? }),
        // Status message type 0, mode 0 (@4): an item picked up, id u32@5, quantity u32@9 (mode 1 is mesos, amount u32@6).
        0x008b if p.len() >= 13 && p[2] == 0 && p[4] == 0 => {
            let (item, qty) = (u32le(p, 5)?, u32le(p, 9)?);
            ((1_000_000..10_000_000).contains(&item) && (1..=10_000).contains(&qty)).then_some(NetEvent::Pickup { item, qty })
        }
        0x008b if p.len() >= 8 && p[2] == 1 => {
            let (id, status) = (u32le(p, 3)?, p[7]);
            let progress = if status == 1 && p.len() >= 10 {
                let n = u16le(p, 8)? as usize;
                String::from_utf8(p.get(10..10 + n)?.to_vec()).ok()?
            } else {
                String::new()
            };
            (id > 0 && id < 1_000_000 && status <= 2).then_some(NetEvent::Quest { id, status, progress })
        }
        0x0072 if p.len() >= 13 => {
            let (tab, slot) = (p[10], u16le(p, 11)?);
            if !(1..=5).contains(&tab) || slot == 0 || slot > 128 {
                return None;
            }
            match p[9] {
                0 if p.len() >= 34 => {
                    let item = u32le(p, 14)?;
                    ((1_000_000..10_000_000).contains(&item)).then_some(NetEvent::Inventory { tab, slot, item: Some(item), qty: Some(u16le(p, 32)? as u32) })
                }
                1 if p.len() >= 15 => Some(NetEvent::Inventory { tab, slot, item: None, qty: Some(u16le(p, 13)? as u32) }),
                3 => Some(NetEvent::Inventory { tab, slot, item: None, qty: None }),
                _ => None,
            }
        }
        0x01a9 if p.len() > 1000 => Some(NetEvent::InventoryAll(login_inventory(p))),
        0x01a9 if p.len() >= 41 => Some(u32le(p, 36)?).filter(|m| (1..=999_999_999).contains(m)).map(|map| NetEvent::MapEnter { map }),
        // Byte 2 is 1 on the replies to our own requests (spending AP), 0 otherwise.
        0x007e if p.len() >= 13 && p[2] <= 1 => parse_stats(p),
        _ => None,
    }
}

/// Every Use/Set-up/Etc stack in the login character data (see the 0x01a9 note at the top).
fn login_inventory(p: &[u8]) -> Vec<(u8, u16, u32, u32)> {
    const NEVER: [u8; 8] = [0x00, 0x80, 0x05, 0xbb, 0x46, 0xe6, 0x17, 0x02];
    let mut out = vec![];
    for e in 8..p.len().saturating_sub(15) {
        if p[e..e + 8] != NEVER || p[e - 6] != 2 {
            continue;
        }
        let (Some(slot), Some(item), Some(qty)) = (u16le(p, e - 8), u32le(p, e - 5), u16le(p, e + 13)) else { continue };
        if (2_000_000..5_000_000).contains(&item) && (1..=128).contains(&slot) {
            out.push(((item / 1_000_000) as u8, slot, item, qty as u32));
        }
    }
    out
}

/// Stat update: `[op 2][3 bytes][mask u32 @5][values @9...]`, values in ascending bit order.
/// Verified: HP 0x400, EXP 0x10000 (EXP total matched the on-screen value after three kills; HP
/// matched memory). Stops at the first bit whose size is unknown, since everything after it would be misaligned.
fn parse_stats(p: &[u8]) -> Option<NetEvent> {
    // Field sizes in this client, from captured packets (a level-up: LEVEL, HP..MAXMP, AP, SP, EXP; spending AP:
    // STR, AP; picking up mesos: MESO). Fields come in mask-bit order; an unknown bit stops the walk.
    // EXP and MESO are 8 bytes: an HP-only update ends 2 bytes after its 4-byte value, an EXP-only or MESO-only one
    // 6 bytes after the low 4 (2026-10-25 capture: mesos 555 -> 566 with an 11-meso pickup message between).
    // JOB's size is not known (2 in the classic client); its value is read but the walk stops after it.
    const LEVEL: u32 = 0x10;
    const JOB: u32 = 0x20;
    const HP: u32 = 0x400;
    const MAXHP: u32 = 0x800;
    const MP: u32 = 0x1000;
    const MAXMP: u32 = 0x2000;
    const AP: u32 = 0x4000;
    const SP: u32 = 0x8000;
    const EXP: u32 = 0x10000;
    const MESO: u32 = 0x40000;
    let size = |bit: u32| -> Option<usize> {
        Some(match bit {
            LEVEL | HP | MAXHP | MP | MAXMP => 4,
            EXP | MESO => 8,
            0x40 | 0x80 | 0x100 | 0x200 | AP | SP => 2, // STR, DEX, INT, LUK, AP, SP
            _ => return None,
        })
    };
    let mask = u32le(p, 5)?;
    let mut u = StatUpdate::default();
    let mut off = 9;
    for bit in (0..32).map(|b| 1u32 << b) {
        if mask & bit == 0 {
            continue;
        }
        if bit == JOB {
            u.job = u16le(p, off).map(u32::from).filter(|j| is_job(*j));
            break;
        }
        let Some(n) = size(bit) else { break };
        // A packet cut short keeps the fields read so far.
        let v = match n {
            8 => u32le(p, off).zip(u32le(p, off + 4)).map(|(lo, hi)| u64::from(lo) | (u64::from(hi) << 32)),
            4 => u32le(p, off).map(u64::from),
            _ => u16le(p, off).map(u64::from),
        };
        let Some(v) = v else { break };
        off += n;
        let v32 = u32::try_from(v).ok();
        match bit {
            HP => u.hp = v32,
            MAXHP => u.max_hp = v32,
            MP => u.mp = v32,
            MAXMP => u.max_mp = v32,
            EXP => u.exp = v32,
            LEVEL => u.level = v32.filter(|l| (1..=250).contains(l)),
            AP => u.ap = v32,
            SP => u.sp = v32,
            MESO => u.meso = Some(v).filter(|m| *m < 10_000_000_000),
            _ => {}
        }
    }
    (!u.is_empty()).then_some(NetEvent::Stats(u))
}

/// A job id the game has: 0 Beginner, then x00 first job and x10..x32 for the later ones (100 Warrior, 110 Fighter ...).
fn is_job(j: u32) -> bool {
    j == 0 || ((1..=5).contains(&(j / 100)) && (j % 100 == 0 || ((1..=3).contains(&(j % 100 / 10)) && j % 10 <= 2)))
}

// ---------------------------------------------------------------- framing

fn header_len(buf: &[u8], i: usize) -> Option<usize> {
    let h = buf.get(i..i + 4)?;
    let a = u16::from_le_bytes([h[0], h[1]]);
    let b = u16::from_le_bytes([h[2], h[3]]);
    Some((a ^ b) as usize)
}

/// How many packets chain cleanly starting at `start` (bounded by `limit`).
fn chain_len(buf: &[u8], start: usize, limit: usize) -> usize {
    let (mut i, mut n) = (start, 0);
    while n < limit {
        match header_len(buf, i) {
            // A real packet carries at least a 2-byte opcode. Length 0 shows up everywhere inside
            // constant-filled bodies (both header halves equal), so it must never extend a chain.
            Some(l) if (2..=MAX_SYNC_LEN).contains(&l) && i + 4 + l <= buf.len() => {
                i += 4 + l;
                n += 1;
            }
            _ => break,
        }
    }
    n
}

/// Splits a mid-connection byte stream into packet bodies, finding its own
/// alignment and re-finding it after any corruption or loss.
#[derive(Default)]
pub struct Framer {
    buf: Vec<u8>,
    synced: bool,
    pub resyncs: u32,
}

impl Framer {
    pub fn reset(&mut self) {
        self.buf.clear();
        self.synced = false;
    }

    pub fn push(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.push_frames(data).into_iter().map(|(_, body)| body).collect()
    }

    /// Like `push`, but each frame comes with the first u16 of its header (it carries two bytes of the receive IV).
    pub fn push_frames(&mut self, data: &[u8]) -> Vec<(u16, Vec<u8>)> {
        self.buf.extend_from_slice(data);
        let mut out = vec![];
        loop {
            if !self.synced {
                // Keep memory bounded while waiting for a clean run.
                if self.buf.len() > 256 * 1024 {
                    let cut = self.buf.len() - 64 * 1024;
                    self.buf.drain(..cut);
                }
                let window = self.buf.len().min(6000);
                let mut best = (0usize, 0usize);
                for s in 0..window {
                    let n = chain_len(&self.buf, s, 40);
                    if n > best.0 {
                        best = (n, s);
                        if n >= 40 {
                            break;
                        }
                    }
                }
                if best.0 < 6 {
                    return out; // need more data
                }
                self.buf.drain(..best.1);
                self.synced = true;
            }
            match header_len(&self.buf, 0) {
                None => return out,
                Some(l) if !(2..=MAX_PACKET_LEN).contains(&l) => {
                    self.synced = false;
                    self.resyncs += 1;
                    self.buf.drain(..1);
                }
                Some(l) => {
                    if self.buf.len() < 4 + l {
                        return out;
                    }
                    out.push((u16::from_le_bytes([self.buf[0], self.buf[1]]), self.buf[4..4 + l].to_vec()));
                    self.buf.drain(..4 + l);
                }
            }
        }
    }
}

// ----------------------------------------------------------- reassembly

/// In-order TCP byte stream from out-of-order, duplicated, wrapping segments.
#[derive(Default)]
pub struct Reassembler {
    next: Option<u32>,
    pending: BTreeMap<u32, Vec<u8>>,
}

impl Reassembler {
    /// Returns (new contiguous bytes, true if the stream had to be restarted).
    pub fn push(&mut self, seq: u32, data: &[u8]) -> (Vec<u8>, bool) {
        let mut out = vec![];
        let next = *self.next.get_or_insert(seq);
        let diff = seq.wrapping_sub(next) as i32;
        if diff < 0 {
            let skip = (-diff) as usize;
            if data.len() <= skip {
                return (out, false); // pure duplicate
            }
            out.extend_from_slice(&data[skip..]);
            self.next = Some(next.wrapping_add(out.len() as u32));
        } else if diff > 0 {
            self.pending.insert(seq, data.to_vec());
            if self.pending.len() > 256 {
                // Lost segments we will never see: restart from here.
                self.pending.clear();
                self.next = Some(seq.wrapping_add(data.len() as u32));
                return (data.to_vec(), true);
            }
            return (out, false);
        } else {
            out.extend_from_slice(data);
            self.next = Some(next.wrapping_add(data.len() as u32));
        }
        // Pull in any pending segments that are now contiguous.
        loop {
            let n = self.next.unwrap();
            let key = self.pending.keys().find(|k| (k.wrapping_sub(n) as i32) <= 0).copied();
            let Some(k) = key else { break };
            let seg = self.pending.remove(&k).unwrap();
            let skip = n.wrapping_sub(k) as usize;
            if seg.len() > skip {
                out.extend_from_slice(&seg[skip..]);
                self.next = Some(n.wrapping_add((seg.len() - skip) as u32));
            }
        }
        (out, false)
    }
}

// ------------------------------------------------------------------ pcap

pub struct TcpSeg {
    pub src: [u8; 4],
    pub sport: u16,
    pub dst: [u8; 4],
    pub dport: u16,
    pub seq: u32,
    pub payload: Vec<u8>,
}

pub struct PcapReader<R: Read> {
    r: R,
    pub linktype: u32,
    be: bool,
    /// Capture time (unix microseconds) of the record `next_packet` returned last. Nanosecond pcaps are not converted: 0xa1b23c4d files
    /// carry nanoseconds in that field, which is read as microseconds (off by 1000x), and tcpdump's default writes microseconds.
    pub last_ts_us: u64,
}

impl<R: Read> PcapReader<R> {
    pub fn new(mut r: R) -> io::Result<Self> {
        let mut h = [0u8; 24];
        r.read_exact(&mut h)?;
        let magic = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
        let be = match magic {
            0xa1b2c3d4 | 0xa1b23c4d => false,
            0xd4c3b2a1 | 0x4d3cb2a1 => true,
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "not a pcap stream")),
        };
        let lt = [h[20], h[21], h[22], h[23]];
        let linktype = if be { u32::from_be_bytes(lt) } else { u32::from_le_bytes(lt) };
        Ok(Self { r, linktype, be, last_ts_us: 0 })
    }

    pub fn next_packet(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut h = [0u8; 16];
        match self.r.read_exact(&mut h) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let rd = |i: usize| {
            let b = [h[i], h[i + 1], h[i + 2], h[i + 3]];
            u64::from(if self.be { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
        };
        self.last_ts_us = rd(0) * 1_000_000 + rd(4);
        let cap = [h[8], h[9], h[10], h[11]];
        let len = if self.be { u32::from_be_bytes(cap) } else { u32::from_le_bytes(cap) } as usize;
        if len > 1 << 20 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "absurd packet length"));
        }
        let mut buf = vec![0u8; len];
        self.r.read_exact(&mut buf)?;
        Ok(Some(buf))
    }
}

/// Link-layer + IPv4 + TCP. Returns None for anything else.
pub fn parse_tcp(linktype: u32, pkt: &[u8]) -> Option<TcpSeg> {
    let ip = match linktype {
        276 => pkt.get(20..)?, // Linux cooked v2
        113 => pkt.get(16..)?, // Linux cooked v1
        1 => pkt.get(14..)?,   // Ethernet
        _ => return None,
    };
    if ip.len() < 20 || ip[0] >> 4 != 4 || ip[9] != 6 {
        return None;
    }
    let ihl = ((ip[0] & 15) as usize) * 4;
    let total = u16::from_be_bytes([ip[2], ip[3]]) as usize;
    let tcp = ip.get(ihl..total.min(ip.len()))?;
    if tcp.len() < 20 {
        return None;
    }
    let off = ((tcp[12] >> 4) as usize) * 4;
    Some(TcpSeg {
        src: [ip[12], ip[13], ip[14], ip[15]],
        dst: [ip[16], ip[17], ip[18], ip[19]],
        sport: u16::from_be_bytes([tcp[0], tcp[1]]),
        dport: u16::from_be_bytes([tcp[2], tcp[3]]),
        seq: u32::from_be_bytes([tcp[4], tcp[5], tcp[6], tcp[7]]),
        payload: tcp.get(off..)?.to_vec(),
    })
}

// -------------------------------------------------------------- pipeline

type FlowKey = ([u8; 4], u16, [u8; 4], u16);

#[derive(Default)]
struct OutFlow {
    re: Reassembler,
    fr: crate::send::SendFramer,
    sc: crate::send::SendCipher,
}

#[derive(Default)]
struct Flow {
    re: Reassembler,
    fr: Framer,
    rc: crate::recv::RecvCipher,
}

/// Server->client segments in, game events out.
#[derive(Default)]
pub struct NetDecoder {
    flows: HashMap<FlowKey, Flow>,
    out_flows: HashMap<FlowKey, OutFlow>,
    /// Client->server packets decrypted so far.
    pub out_packets: u64,
    /// Capture time (unix microseconds) of the segment being pushed; the dump line carries it when set.
    pub cur_ts_us: u64,
    pub packets: u64,
    pub new_flow: bool,
    /// Every opcode seen, with counts and sizes.
    pub ops: HashMap<u16, OpStat>,
    /// Opcodes that raise an alert when they arrive.
    pub alert_opcodes: HashMap<u16, AlertKind>,
    /// Opcodes whose decoded packets are printed to stderr as hex (`NEON_DUMP_OPS=007e,0471`), for decoding work.
    pub dump_ops: Vec<u16>,
}

impl NetDecoder {
    pub fn resyncs(&self) -> u64 {
        self.flows.values().map(|f| f.fr.resyncs as u64).sum()
    }
}

/// Local ports of `pid`'s TCP connections to a game server port, from /proc (its socket inodes matched against
/// /proc/<pid>/net/tcp). Server-to-client packets of this client arrive on these ports.
pub fn game_ports_of(pid: u32) -> Vec<u16> {
    let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else { return vec![] };
    let inodes: std::collections::HashSet<String> = fds
        .filter_map(|e| std::fs::read_link(e.ok()?.path()).ok())
        .filter_map(|l| l.to_str()?.strip_prefix("socket:[")?.strip_suffix(']').map(str::to_string))
        .collect();
    let mut out = vec![];
    for table in ["tcp", "tcp6"] {
        let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/net/{table}")) else { continue };
        out.extend(game_ports_in(&text, &inodes));
    }
    out
}

fn game_ports_in(table: &str, inodes: &std::collections::HashSet<String>) -> Vec<u16> {
    let port = |addr: &str| addr.rsplit(':').next().and_then(|p| u16::from_str_radix(p, 16).ok());
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let c: Vec<&str> = line.split_whitespace().collect();
            let (local, remote) = (port(c.get(1)?)?, port(c.get(2)?)?);
            (inodes.contains(*c.get(9)?) && (PORT_MIN..=PORT_MAX).contains(&remote)).then_some(local)
        })
        .collect()
}

impl NetDecoder {
    /// True when `seg` is server-to-client traffic on a game port.
    pub fn is_game_inbound(seg: &TcpSeg) -> bool {
        (PORT_MIN..=PORT_MAX).contains(&seg.sport) && !seg.payload.is_empty()
    }

    /// True when `seg` is client-to-server traffic to a game port.
    pub fn is_game_outbound(seg: &TcpSeg) -> bool {
        (PORT_MIN..=PORT_MAX).contains(&seg.dport) && !(PORT_MIN..=PORT_MAX).contains(&seg.sport) && !seg.payload.is_empty()
    }

    /// Decrypt the client's own traffic (exact: `send.rs`). Only printed (`NEON_DUMP_OPS`), there are no events for it yet.
    fn push_outbound(&mut self, seg: &TcpSeg) {
        let key = (seg.src, seg.sport, seg.dst, seg.dport);
        let flow = self.out_flows.entry(key).or_default();
        let (bytes, restarted) = flow.re.push(seg.seq, &seg.payload);
        if restarted {
            flow.fr.reset();
        }
        for (a, body) in flow.fr.push(&bytes) {
            for plain in flow.sc.frame(a, &body) {
                self.out_packets += 1;
                if let Some(op) = u16le(&plain, 0) {
                    if self.dump_ops.contains(&op) || self.dump_ops.contains(&0xffff) {
                        let ts = if self.cur_ts_us > 0 { format!("ts {:.3} ", self.cur_ts_us as f64 / 1e6) } else { String::new() };
                        eprintln!("{ts}wire op {op:04x} len {}: {}", plain.len(), plain.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
                    }
                }
            }
        }
    }

    pub fn push(&mut self, seg: &TcpSeg) -> Vec<NetEvent> {
        if Self::is_game_outbound(seg) {
            self.push_outbound(seg);
            return vec![];
        }
        if !Self::is_game_inbound(seg) {
            return vec![];
        }
        let key = (seg.src, seg.sport, seg.dst, seg.dport);
        if !self.flows.contains_key(&key) {
            self.new_flow = true; // a new connection means a new map/channel: consumers should reset
            self.flows.clear();
            self.out_flows.clear();
        }
        let flow = self.flows.entry(key).or_default();
        let (bytes, restarted) = flow.re.push(seg.seq, &seg.payload);
        if restarted {
            flow.fr.reset();
        }
        let mut events = vec![];
        for (a, body) in flow.fr.push_frames(&bytes) {
            self.packets += 1;
            if body.len() >= 2 {
                // exact decryption once the receive IV is known; the per-block heuristic until then
                let plain = flow.rc.frame(a, &body).unwrap_or_else(|| decode_payload(&body));
                if let Some(op) = u16le(&plain, 0) {
                    if self.dump_ops.contains(&op) || self.dump_ops.contains(&0xffff) {
                        let ts = if self.cur_ts_us > 0 { format!("ts {:.3} ", self.cur_ts_us as f64 / 1e6) } else { String::new() };
                        eprintln!("{ts}op {op:04x} len {}: {}", plain.len(), plain.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
                    }
                    let st = self.ops.entry(op).or_default();
                    st.count += 1;
                    st.total_len += plain.len() as u64;
                    st.min_len = if st.count == 1 { plain.len() } else { st.min_len.min(plain.len()) };
                    st.max_len = st.max_len.max(plain.len());
                    if let Some(kind) = self.alert_opcodes.get(&op) {
                        events.push(NetEvent::Alert { kind: *kind });
                    }
                }
                if let Some(ev) = parse_event(&plain) {
                    events.push(ev);
                }
            }
        }
        events
    }
}

// --------------------------------------------------------------- tracker

#[derive(Debug, Clone, PartialEq)]
pub struct MobRec {
    pub template: u32,
    pub x: i32,
    pub y: i32,
    pub last_ms: u64,
    /// Horizontal speed in px/s, estimated from successive move updates (the server only tells
    /// us where each move starts, so between updates the mob is somewhere ahead of that point).
    pub vx: f32,
    /// Where the latest move ends (equal to `x` when the packet had no destination).
    pub to_x: i32,
}

impl MobRec {
    /// Estimated position now: the last known point plus up to one second of travel.
    pub fn predicted(&self, now_ms: u64) -> (i32, i32) {
        let age = now_ms.saturating_sub(self.last_ms);
        if self.to_x != self.x {
            // The packet told us where this move ends: walk there over a typical move length.
            let f = (age as f32 / MOVE_MS).min(1.0);
            return (self.x + ((self.to_x - self.x) as f32 * f).round() as i32, self.y);
        }
        (self.x + (self.vx * age.min(1000) as f32 / 1000.0).round() as i32, self.y)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DropRec {
    pub x: i32,
    pub y: i32,
    pub owner: u32,
    pub last_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRec {
    pub name: Option<String>,
    pub pos: Option<(i32, i32)>,
    pub last_ms: u64,
}

/// Live world as seen on the wire.
#[derive(Default)]
pub struct NetTracker {
    /// Our party as last described (empty when none).
    pub party: Vec<PartyMember>,
    pub lie_detector_until: Option<u64>,
    pub mobs: HashMap<u32, MobRec>,
    pub players: HashMap<u32, PlayerRec>,
    pub drops: HashMap<u32, DropRec>,
    pub stats: NetStats,
    pub events: u64,
    /// Drop spawns seen per owner character id. The server sends a client the drops it owns (captures: 227 of 227
    /// spawns had one owner, who picked up every one that was picked up), so the dominant owner is us.
    pub drop_owners: HashMap<u32, u32>,
    /// The map we are in, from the last set-field packet (None until one arrives).
    pub map: Option<u32>,
    /// Quest states reported since the stream started: id -> (status, progress).
    pub quests: BTreeMap<u32, (u8, String)>,
    /// Items picked up since the stream started: item id -> total quantity.
    pub pickups: BTreeMap<u32, u32>,
    /// Inventory slots seen changing: (tab, slot) -> (item if known, quantity). Stacks that existed before the stream
    /// started are named by the pickup message that follows their quantity update.
    pub inventory: BTreeMap<(u8, u16), (Option<u32>, u32)>,
    /// The slot whose quantity changed last (a pickup message of item X right after names it X).
    last_inv: Option<(u8, u16)>,
}

impl NetTracker {
    /// Our own character id, learnt from drop owners: the only owner seen, or one with at least twice the drops of
    /// any other. None until then.
    pub fn own_id(&self) -> Option<u32> {
        let mut v: Vec<(u32, u32)> = self.drop_owners.iter().map(|(k, n)| (*k, *n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        match v.as_slice() {
            [] => None,
            [(id, _)] => Some(*id),
            [(id, n), (_, m), ..] => (*n >= 2 * *m).then_some(*id),
        }
    }

    /// Drops we may pick up: owned by us (by id once known). Nothing until our id is known.
    pub fn own_drops(&self) -> Vec<&DropRec> {
        match self.own_id() {
            Some(me) => self.drops.values().filter(|d| d.owner == me).collect(),
            None => vec![],
        }
    }
}

/// Latest own stats seen on the wire (None until the server sends them).
#[derive(Default, Debug, Clone, PartialEq)]
pub struct NetStats {
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

impl NetTracker {
    pub fn clear(&mut self) {
        self.mobs.clear();
        self.players.clear();
        self.drops.clear();
    }

    /// An anti-macro alert stays raised for this long after the packet that announced it.
    pub const ALERT_HOLD_MS: u64 = 120_000;

    pub fn lie_detector_active(&self, now_ms: u64) -> bool {
        self.lie_detector_until.map_or(false, |t| now_ms < t)
    }

    pub fn apply(&mut self, ev: &NetEvent, now_ms: u64) {
        self.events += 1;
        match ev {
            NetEvent::Party { members, .. } => self.party = members.clone(),
            NetEvent::PartyLeft { char_id } => {
                // Us leaving ends the party; someone else leaving only removes them.
                if self.own_id() == Some(*char_id) {
                    self.party.clear();
                } else {
                    self.party.retain(|m| m.char_id != *char_id);
                }
            }
            NetEvent::Alert { kind: AlertKind::LieDetector } => self.lie_detector_until = Some(now_ms + Self::ALERT_HOLD_MS),
            NetEvent::MobSpawn { oid, template, x, y } => {
                self.mobs.insert(*oid, MobRec { template: *template, x: *x, y: *y, last_ms: now_ms, vx: 0.0, to_x: *x });
            }
            NetEvent::MobMove { oid, x, y, to_x } => {
                // The server only sends spawns when a map is entered, so mobs that were already
                // there when we started listening show up first as movement. Only mobs send these,
                // so track them with an unknown template (0) rather than losing them.
                let m = self.mobs.entry(*oid).or_insert(MobRec { template: 0, x: *x, y: *y, last_ms: now_ms, vx: 0.0, to_x: *x });
                let dt = now_ms.saturating_sub(m.last_ms);
                let dx = *x - m.x;
                if (150..=4000).contains(&dt) && dx.abs() <= 400 && (*y - m.y).abs() <= 40 {
                    let v = (dx as f32 * 1000.0 / dt as f32).clamp(-250.0, 250.0);
                    m.vx = 0.5 * m.vx + 0.5 * v;
                } else if dt > 4000 {
                    m.vx = 0.0;
                }
                m.x = *x;
                m.y = *y;
                m.to_x = *to_x;
                m.last_ms = now_ms;
            }
            NetEvent::MobKill { oid } => {
                self.mobs.remove(oid);
            }
            NetEvent::Stats(u) => {
                // Merge: each packet only carries the fields that changed.
                for (dst, src) in [
                    (&mut self.stats.hp, u.hp),
                    (&mut self.stats.max_hp, u.max_hp),
                    (&mut self.stats.mp, u.mp),
                    (&mut self.stats.max_mp, u.max_mp),
                    (&mut self.stats.exp, u.exp),
                    (&mut self.stats.level, u.level),
                    (&mut self.stats.ap, u.ap),
                    (&mut self.stats.sp, u.sp),
                    (&mut self.stats.job, u.job),
                ] {
                    if src.is_some() {
                        *dst = src;
                    }
                }
                if u.meso.is_some() {
                    self.stats.meso = u.meso;
                }
            }
            NetEvent::Pickup { item, qty } => {
                *self.pickups.entry(*item).or_insert(0) += qty;
                if let Some(k) = self.last_inv.take() {
                    if let Some(e) = self.inventory.get_mut(&k) {
                        if e.0.is_none() && (k.0 as u32) == item / 1_000_000 {
                            e.0 = Some(*item);
                        }
                    }
                }
            }
            NetEvent::InventoryAll(all) => {
                self.inventory.retain(|(tab, _), _| !(2..=4).contains(tab));
                for &(tab, slot, item, qty) in all {
                    self.inventory.insert((tab, slot), (Some(item), qty));
                }
                self.last_inv = None;
            }
            NetEvent::Inventory { tab, slot, item, qty } => {
                let k = (*tab, *slot);
                match qty {
                    None => {
                        self.inventory.remove(&k);
                        self.last_inv = None;
                    }
                    Some(q) => {
                        let known = item.or_else(|| self.inventory.get(&k).and_then(|e| e.0));
                        self.inventory.insert(k, (known, *q));
                        self.last_inv = Some(k);
                    }
                }
            }
            NetEvent::DropSpawn { oid, owner, x, y } => {
                if !self.drops.contains_key(oid) {
                    *self.drop_owners.entry(*owner).or_insert(0) += 1; // each drop is sent twice
                }
                self.drops.insert(*oid, DropRec { x: *x, y: *y, owner: *owner, last_ms: now_ms });
            }
            NetEvent::DropRemove { oid } => {
                self.drops.remove(oid);
            }
            NetEvent::Quest { id, status, progress } => {
                self.quests.insert(*id, (*status, progress.clone()));
            }
            NetEvent::MapEnter { map } => {
                // Everything of the map we left is gone; the new map's mobs, players and drops follow as spawns.
                self.clear();
                self.map = Some(*map);
            }
            NetEvent::PlayerSpawn { char_id, name } => {
                let p = self.players.entry(*char_id).or_insert(PlayerRec { name: None, pos: None, last_ms: now_ms });
                p.name = Some(name.clone());
                p.last_ms = now_ms;
            }
            NetEvent::PlayerMove { char_id, x, y } => {
                let p = self.players.entry(*char_id).or_insert(PlayerRec { name: None, pos: None, last_ms: now_ms });
                p.pos = Some((*x, *y));
                p.last_ms = now_ms;
            }
        }
        if self.mobs.len() > 4000 {
            self.mobs.clear(); // runaway guard: a decode problem, not a real map
        }
    }

    /// On a map change keep only what the server told us about very recently: the new
    /// map's spawns arrive a moment *before* the client's map id settles.
    pub fn retain_recent(&mut self, now_ms: u64, window_ms: u64) {
        self.mobs.retain(|_, m| now_ms.saturating_sub(m.last_ms) <= window_ms);
        self.players.retain(|_, p| now_ms.saturating_sub(p.last_ms) <= window_ms);
        self.drops.retain(|_, d| now_ms.saturating_sub(d.last_ms) <= window_ms);
    }

    /// Drops disappear from the map after about a minute even without a packet we decode.
    pub fn expire_drops(&mut self, now_ms: u64, max_age_ms: u64) {
        self.drops.retain(|_, d| now_ms.saturating_sub(d.last_ms) <= max_age_ms);
    }

    /// Players with no packet for this long are assumed to have left (the removal
    /// opcode has not been identified).
    pub fn expire_players(&mut self, now_ms: u64, max_age_ms: u64) {
        self.players.retain(|_, p| now_ms.saturating_sub(p.last_ms) <= max_age_ms);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_login_packet_lists_the_whole_inventory() {
        // Captured 2026-10-25: shurella2 logging in (3370-byte set-field); the Use tab on screen matched.
        let p: Vec<u8> = (0..LOGIN.len()).step_by(2).map(|i| u8::from_str_radix(&LOGIN[i..i + 2], 16).unwrap()).collect();
        let Some(NetEvent::InventoryAll(inv)) = parse_event(&p) else { panic!("not the login inventory") };
        assert!(inv.contains(&(2, 5, 2000003, 8)), "8 Blue Potions in Use slot 5: {inv:?}");
        assert!(inv.contains(&(2, 3, 2061000, 45)) && inv.contains(&(2, 7, 2060000, 17)));
        assert_eq!(inv.iter().filter(|e| e.0 == 2).count(), 5);
    }
    const LOGIN: &str = "a901e0dfb4cec864dd012300000000000000000100000000f7020000ea0200000100001484669f580ed5d0855133ad0101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010100000000000000000000005a1a05005a1a05005000000073687572656c69613200000000000100000000204e0000357500000a00000000003700070004000400c2000000c200000071000000710000000000010009000000a1010000000000000000000023a69800070000000040e0fd3b374f01a764dd018015e84b3200000000000040e0fd3b374f01000000000000000000000000000000000040e0fd3b374f01ffffffff000000000040e0fd3b374f01ffffffff000000000040e0fd3b374f012900000000000000000000001e001e001e001e0096002c0100000005000181de0f0000008005bb46e61702ffffffff000004000006000100140007ff000100000000000000000000000000000000000000000000000000000000000000ffffffffffffffff00000000000000000040e0fd3b374f0100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000060001a22c100000008005bb46e61702ffffffff000004000004000100140007ff000100000000000000000000000000000000000000000000000000000000000000ffffffffffffffff00000000000000000040e0fd3b374f0100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000070001835b100000008005bb46e61702ffffffff000004000002000100140005ff000100000000000000000000000000000000000000000000000000000000000000ffffffffffffffff00000000000000000040e0fd3b374f01000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000b0001102c140000008005bb46e61702ffffffff000000010013000100140007ff000100000000000000000000000000000000000000000000000000000000000000ffffffffffffffff00000000000000000040e0fd3b374f010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010002528c1e0000008005bb46e61702ffffffff000100000000000000000000000000000000000000000002000292ab1e0000008005bb46e61702ffffffff0007000000000000000000000000000000000000000000030002c8721f0000008005bb46e61702ffffffff002d00000000000000000000000000000000000000000005000283841e0000008005bb46e61702ffffffff0008000000000000000000000000000000000000000000070002e06e1f0000008005bb46e61702ffffffff00110000000000000000000000000000000000000000000000000001000218823d0000008005bb46e61702ffffffff000100000000000000000000000000000000000000000003000219823d0000008005bb46e61702ffffffff0001000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010500e803000003000000008005bb46e61702e903000003000000008005bb46e61702ea03000003000000008005bb46e617024900000000000000008005bb46e617020c00000000000000008005bb46e6170200000105000000000003003031307a1c0000010030112700000000642d00000100301927000006003030383030330000011800e8030000d03235c9b361dd01e9030000d03235c9b361dd01ea030000d03235c9b361dd01eb030000d03235c9b361dd01ec030000d03235c9b361dd01ed030000d03235c9b361dd01ee030000d03235c9b361dd01ef030000d03235c9b361dd01f0030000d03235c9b361dd01f1030000d03235c9b361dd01f2030000d03235c9b361dd0110270000d016634f7864dd01f3030000d03235c9b361dd011b1e00005024d7e91364dd01f4030000d03235c9b361dd01f5030000d03235c9b361dd01f6030000d03235c9b361dd01f7030000d03235c9b361dd01f8030000d03235c9b361dd01f9030000d03235c9b361dd01fa030000d03235c9b361dd01fb030000d03235c9b361dd011a270000400b2f707764dd011b2700000064ae787c64dd010000000000000000ffc99a3bffc99a3bffc99a3bffc99a3bffc99a3bffc99a3bffc99a3bffc99a3bffc99a3bffc99a3bffc99a3bffc99a3bffc99a3bffc99a3bffc99a3b13009e69000008006974656d69643d30bb8c0100a40066724f6e4e6f743d313b616c57683d313b6275664175616c3d303b736f756c55493d313b616c457863683d313b616c46723d313b636846723d313b616c4d653d313b616c50613d313b73684d643d313b636847753d313b616c47753d313b6275664d696e3d303b61637048503d303b736f457255493d303b666c48503d31303b73684e6b3d313b6368416c3d313b616c416c3d313b616c466f6c3d313b666c4d503d3130a06900001d00303d3236313032353137333535383b313d3236313032353231333535304b04000009007169643d3130303039975f010019006372616674696e675765656b6c795f5273743d3135353531394b4a00001500646174653d303b69643d303b736c6f74706f733d30995f010015006c6f676f75743d3230323631303235313733353538038d010026004d617841503d35383b526563656e7441503d35383b446174653d323032363130323531333036e68c01001700646174653d32303236313032353b636f756e743d33333231c807000f0071315f645f5273743d31353535323532c807000f0071315f775f5273743d31353535313933c807000f0071315f645f5273743d31353535323514690000130073663d303b616c3d313b6d743d313b69643d3034c807000f0071315f775f5273743d313535353139a206000029004c617374446174653d3234323330363b636f756e7431306d696e3d313b636f756e7433306d696e3d315e3d000017006c61737474696d653d32362f31302f32352f31322f3438db8e01001500583d303b453d2d313b503d2d313b543d31373139335e4900000300543d31237f00001f006c61737444656354696d653d323032362f31302f32352030303a30363a35340000010000000001000000eeabca0bffffffff0000000000000000000000000040e0fd3b374f010100000000000000000000000040e0fd3b374f010000000006000100000016006163636570743d313b646174653d32362f31302f323103000000050077696e3d30040000000b0041504943616c6c65643d3170010000bf00666341323d373b636d624d7367323d303b7472656d323d313b704261636b323d313b7653796e63313d313b666346323d303b7173456666313d323b614f74686572323d3130303b734175746f313d313b666347323d353b734e756d313d353b615549323d303b666357323d333b6653697a65323d303b6d61675549313d313b614d696e65323d38303b64616d456666313d303b6d6f62496e66313d333b61764d656761323d313b6654797065313d313b664c6f63313d303b73436f6e313d3071010000bc0067714350333d333b6d6c55704163323d303b614d795065743d3130303b677150333d303b67714342333d333b636854696d65323d303b677142333d303b73696c5468333d303b6368506f73323d303b67714354333d333b67714345333d333b73696c5479333d303b73696c426f333d303b677154333d303b677145333d303b64616d416d74333d333b677149333d353b717354696d65323d303b6771434c333d333b776e6448744b313d313b67714c333d303b6d6c55705479323d307201000087006d5345313d303b6d5356313d303b696e6649746d3d303b614d79416e643d3130303b6d4d45313d303b6d4d313d303b764247313d31353b765345313d31353b6155725065743d3130303b764d313d31393b765356313d31353b6d416e64313d313b615572416e643d3130303b764d45313d31353b6d45313d303b7645313d31353b6d4247313d3001000000000000000024030100ffffffff000000000100ffffffff000000000100ffffffff0000000000000000000000000000000000000000000000000000000000000000000000000000000000000024030100ffffffff000000000100ffffffff000000000100ffffffff000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c030100ffffffff000000000100ffffffff000000000100ffffffff000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001000040e0fd3b374f01000040e0fd3b374f010040e0fd3b374f01088f3c790002006400000000010000000000000000000000000001ffffffff0000000000000000ffc99a3bffc99a3b000000000000000000000000000000";

    #[test]
    fn inventory_slots_from_updates_and_pickups() {
        // Captured on jib02: a Red Potion picked up into Use slot 2 (15 now), then the pickup message.
        let q = hex("72 00 01 00 01 00 00 00 00 01 02 02 00 0f 00");
        assert_eq!(parse_event(&q), Some(NetEvent::Inventory { tab: 2, slot: 2, item: None, qty: Some(15) }));
        let add = hex("72 00 01 00 01 00 00 00 00 00 02 04 00 02 c8 72 1f 00 00 00 80 05 bb 46 e6 17 02 ff ff ff ff 01 12 00 00 dd 72 73 73 aa 73 73 73 73 73 73 73 00 00 00 00 00 00 00");
        assert_eq!(parse_event(&add), Some(NetEvent::Inventory { tab: 2, slot: 4, item: Some(2061000), qty: Some(18) }));
        let mut t = NetTracker::default();
        t.apply(&parse_event(&q).unwrap(), 0);
        t.apply(&NetEvent::Pickup { item: 2000000, qty: 1 }, 1);
        t.apply(&parse_event(&add).unwrap(), 2);
        assert_eq!(t.inventory.get(&(2, 2)), Some(&(Some(2000000), 15)));
        assert_eq!(t.inventory.get(&(2, 4)), Some(&(Some(2061000), 18)));
        // A later use lowers the count and keeps the name.
        t.apply(&NetEvent::Inventory { tab: 2, slot: 2, item: None, qty: Some(14) }, 3);
        assert_eq!(t.inventory.get(&(2, 2)), Some(&(Some(2000000), 14)));
    }

    #[test]
    fn quest_records_carry_status_and_kill_counts() {
        // Captured: Mai's Final Training (1010) in progress, one Orange Mushroom killed.
        let p = hex("8b 00 01 f2 03 00 00 01 03 00 30 30 31");
        assert_eq!(parse_event(&p), Some(NetEvent::Quest { id: 1010, status: 1, progress: "001".into() }));
        // Other status messages (type 3: items, 13: info strings) are not quests.
        assert_eq!(parse_event(&hex("8b 00 03 00 02 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00")), None);
    }

    #[test]
    fn set_field_tells_the_map_we_entered() {
        // Captured on Maple Island: entering Snail Hunting Ground III (42) through its west portal (spawn 5).
        let p = hex("a9 01 70 1a 37 b9 b4 64 dd 01 24 00 00 00 00 00 00 00 00 17 00 00 00 00 74 0c 00 00 82 05 00 00 00 00 00 00 2a 00 00 00 05 3d 00 00 00 00 02 00 64 00 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 01 ff ff ff ff 00 00 00 00 00 00 00 00 ff c9 9a 3b ff c9 9a 3b 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00");
        assert_eq!(parse_event(&p), Some(NetEvent::MapEnter { map: 42 }));
        let mut t = NetTracker::default();
        t.apply(&NetEvent::MobSpawn { oid: 1, template: 2, x: 0, y: 0 }, 0);
        t.apply(&NetEvent::MapEnter { map: 42 }, 10);
        assert!(t.mobs.is_empty() && t.map == Some(42));
    }

    #[test]
    fn only_drops_owned_by_us_are_listed() {
        let mut t = NetTracker::default();
        assert!(t.own_drops().is_empty());
        for (oid, owner) in [(1, 7), (1, 7), (2, 7), (3, 7), (4, 9)] {
            t.apply(&NetEvent::DropSpawn { oid, owner, x: 0, y: 0 }, 0);
        }
        assert_eq!(t.own_id(), Some(7));
        let mut mine: Vec<u32> = t.own_drops().iter().map(|d| d.owner).collect();
        mine.sort();
        assert_eq!(mine, vec![7, 7, 7], "the drop owned by 9 is not ours");
        // Two owners equally often: unknown, so nothing is looted.
        let mut t = NetTracker::default();
        t.apply(&NetEvent::DropSpawn { oid: 1, owner: 7, x: 0, y: 0 }, 0);
        t.apply(&NetEvent::DropSpawn { oid: 2, owner: 9, x: 0, y: 0 }, 0);
        assert!(t.own_drops().is_empty());
    }

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    #[test]
    fn game_ports_are_the_local_ends_of_own_connections_to_a_game_port() {
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:E88C 0A000001:2182 01 00000000:00000000 00:00000000 00000000  1000        0 111 1
   1: 0100007F:B19B 0A000001:22D1 01 00000000:00000000 00:00000000 00000000  1000        0 112 1
   2: 0100007F:9000 0A000001:2182 01 00000000:00000000 00:00000000 00000000  1000        0 999 1
";
        let mine: std::collections::HashSet<String> = ["111", "112"].iter().map(|s| s.to_string()).collect();
        // 0x2182 = 8578 is a game port; 0x22D1 = 8913 is not; inode 999 is another process's socket.
        assert_eq!(game_ports_in(table, &mine), vec![0xE88C]);
    }

    use super::*;

    fn fixtures() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/fixtures/net_packets.json")).unwrap()
    }
    fn raw(f: &serde_json::Value, op: &str) -> Vec<u8> {
        let hex = f[op]["raw"].as_str().unwrap();
        (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect()
    }
    fn event(f: &serde_json::Value, op: &str) -> NetEvent {
        let r = raw(f, op);
        let mut fr = Framer::default();
        // a single packet is too short to sync, so frame it by hand
        let l = header_len(&r, 0).unwrap();
        assert_eq!(r.len(), 4 + l, "fixture is header+payload");
        let _ = &mut fr;
        parse_event(&decode_payload(&r[4..])).expect("parses")
    }

    #[test]
    fn decodes_a_real_mob_spawn() {
        let f = fixtures();
        let e = &f["0x3c9"]["expect"];
        assert_eq!(
            event(&f, "0x3c9"),
            NetEvent::MobSpawn {
                oid: e["oid"].as_u64().unwrap() as u32,
                template: e["template"].as_u64().unwrap() as u32,
                x: e["x"].as_i64().unwrap() as i32,
                y: e["y"].as_i64().unwrap() as i32,
            }
        );
    }

    /// Regenerates neon-protocol/tests/fixtures/plain_packets.json (full decoded plaintext of the real fixtures).
    /// Run: cargo test --release dump_plain_fixtures -- --ignored
    #[test]
    #[ignore]
    fn dump_plain_fixtures() {
        let f = fixtures();
        let mut out = serde_json::Map::new();
        for (k, v) in f.as_object().unwrap() {
            let r = raw(&f, k);
            let l = header_len(&r, 0).unwrap();
            assert_eq!(r.len(), 4 + l);
            let plain = decode_payload(&r[4..]);
            let mut e = serde_json::Map::new();
            e.insert("plain".into(), hex::encode(&plain).into());
            if let Some(x) = v.get("expect") { e.insert("expect".into(), x.clone()); }
            out.insert(k.clone(), e.into());
        }
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../neon-protocol/tests/fixtures/plain_packets.json");
        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_string_pretty(&out).unwrap()).unwrap();
    }

    #[test]
    fn decodes_update_kill_player_spawn_and_move() {
        let f = fixtures();
        let e = &f["0x3dc"]["expect"];
        assert_eq!(
            event(&f, "0x3dc"),
            NetEvent::MobMove { oid: e["oid"].as_u64().unwrap() as u32, x: e["x"].as_i64().unwrap() as i32, y: e["y"].as_i64().unwrap() as i32, to_x: 666 }
        );
        assert_eq!(event(&f, "0x3d4"), NetEvent::MobKill { oid: f["0x3d4"]["expect"]["oid"].as_u64().unwrap() as u32 });
        assert_eq!(
            event(&f, "0x22d"),
            NetEvent::PlayerSpawn { char_id: f["0x22d"]["expect"]["char_id"].as_u64().unwrap() as u32, name: "alex5".into() }
        );
        let m = &f["0x29d"]["expect"];
        assert_eq!(
            event(&f, "0x29d"),
            NetEvent::PlayerMove { char_id: m["char_id"].as_u64().unwrap() as u32, x: m["x"].as_i64().unwrap() as i32, y: m["y"].as_i64().unwrap() as i32 }
        );
    }

    #[test]
    fn real_spawn_is_a_known_floor_height() {
        // The hunting ground's platforms are 215, -85, -325, -565: a decode with the wrong
        // constant would produce nonsense coordinates instead.
        let f = fixtures();
        match event(&f, "0x3c9") {
            NetEvent::MobSpawn { template, y, .. } => {
                assert!((2..=14).contains(&template));
                assert!([215, -85, -325, -565, -391, 334, 274].contains(&y), "y={y}");
            }
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn framer_finds_alignment_mid_stream_and_survives_garbage_prefix() {
        let f = fixtures();
        let pk: Vec<Vec<u8>> = ["0x29d", "0x3dc", "0x3d4", "0x29d", "0x3dc", "0x3d4", "0x29d", "0x3dc"].iter().map(|k| raw(&f, k)).collect();
        let mut stream = vec![0xAB, 0xCD, 0xEF]; // joined mid-packet
        for p in &pk {
            stream.extend_from_slice(p);
        }
        let mut fr = Framer::default();
        let bodies = fr.push(&stream);
        assert!(bodies.len() >= pk.len() - 1, "got {} of {}", bodies.len(), pk.len());
        assert_eq!(bodies.last().unwrap().len(), pk.last().unwrap().len() - 4);
    }

    #[test]
    fn framer_does_not_sync_on_runs_of_constant_bytes() {
        // A body of one repeated byte makes every window a valid header of length 0.
        let mut fr = Framer::default();
        assert!(fr.push(&vec![0x31u8; 4000]).is_empty());
        let f = fixtures();
        let mut stream = vec![0x31u8; 500];
        for k in ["0x29d", "0x3dc", "0x3d4", "0x29d", "0x3dc", "0x3d4", "0x29d"] {
            stream.extend_from_slice(&raw(&f, k));
        }
        let bodies = fr.push(&stream);
        assert!(bodies.len() >= 6, "recovered {} packets after the constant run", bodies.len());
    }

    #[test]
    fn framer_handles_arbitrary_chunking() {
        let f = fixtures();
        let mut stream = vec![];
        for k in ["0x29d", "0x3dc", "0x3d4"].iter().cycle().take(12) {
            stream.extend_from_slice(&raw(&f, k));
        }
        let mut fr = Framer::default();
        let mut n = 0;
        for chunk in stream.chunks(37) {
            n += fr.push(chunk).len();
        }
        assert!(n >= 11, "got {n}");
    }

    #[test]
    fn reassembler_orders_dedupes_and_wraps() {
        let mut r = Reassembler::default();
        let base = u32::MAX - 3; // forces sequence wraparound
        assert_eq!(r.push(base, b"AAAA").0, b"AAAA");
        assert!(r.push(base.wrapping_add(8), b"CCCC").0.is_empty(), "out of order is held");
        assert_eq!(r.push(base.wrapping_add(4), b"BBBB").0, b"BBBBCCCC", "gap filled releases both");
        assert!(r.push(base, b"AAAA").0.is_empty(), "retransmit ignored");
        assert_eq!(r.push(base.wrapping_add(10), b"CCDD").0, b"DD", "partial overlap trimmed");
    }

    fn sll2_tcp(sport: u16, seq: u32, payload: &[u8]) -> Vec<u8> {
        let mut p = vec![0x08, 0x00, 0, 0, 0, 0, 0, 2, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0]; // 20-byte SLL2 header
        let total = 20 + 20 + payload.len();
        let mut ip = vec![0x45, 0, (total >> 8) as u8, total as u8, 0, 0, 0, 0, 64, 6, 0, 0, 44, 234, 179, 68, 10, 0, 0, 101];
        let mut tcp = vec![(sport >> 8) as u8, sport as u8, 0x8e, 0x6a];
        tcp.extend_from_slice(&seq.to_be_bytes());
        tcp.extend_from_slice(&[0, 0, 0, 0, 0x50, 0x18, 0, 0, 0, 0, 0, 0]);
        p.append(&mut ip);
        p.append(&mut tcp);
        p.extend_from_slice(payload);
        p
    }

    #[test]
    fn parses_cooked_v2_tcp_segment() {
        let seg = parse_tcp(276, &sll2_tcp(8578, 1234, b"hello")).unwrap();
        assert_eq!((seg.sport, seg.seq, seg.payload.as_slice()), (8578, 1234, &b"hello"[..]));
        assert_eq!(seg.src, [44, 234, 179, 68]);
        assert!(NetDecoder::is_game_inbound(&seg));
        let other = parse_tcp(276, &sll2_tcp(443, 1, b"x")).unwrap();
        assert!(!NetDecoder::is_game_inbound(&other), "HTTPS is not game traffic");
    }

    #[test]
    fn pcap_reader_roundtrip() {
        let pkt = sll2_tcp(8578, 5, b"abc");
        let mut file = vec![];
        file.extend_from_slice(&0xa1b2c3d4u32.to_le_bytes());
        file.extend_from_slice(&[2, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        file.extend_from_slice(&65535u32.to_le_bytes());
        file.extend_from_slice(&276u32.to_le_bytes());
        for i in 0..2u32 {
            file.extend_from_slice(&(1_793_000_000u32 + i).to_le_bytes()); // seconds
            file.extend_from_slice(&(250_000u32 + i).to_le_bytes()); // microseconds
            file.extend_from_slice(&(pkt.len() as u32).to_le_bytes());
            file.extend_from_slice(&(pkt.len() as u32).to_le_bytes());
            file.extend_from_slice(&pkt);
        }
        let mut r = PcapReader::new(&file[..]).unwrap();
        assert_eq!(r.linktype, 276);
        let mut n = 0;
        while let Some(p) = r.next_packet().unwrap() {
            assert_eq!(parse_tcp(276, &p).unwrap().payload, b"abc");
            assert_eq!(r.last_ts_us, (1_793_000_000 + n) * 1_000_000 + 250_000 + n, "the record's capture time is kept");
            n += 1;
        }
        assert_eq!(n, 2);
        assert!(PcapReader::new(&b"not a pcap file at all.........."[..]).is_err());
    }

    #[test]
    fn end_to_end_decoder_emits_events_from_segments() {
        let f = fixtures();
        let mut stream = vec![];
        for k in ["0x3c9", "0x3dc", "0x3d4", "0x22d", "0x29d", "0x3c9", "0x3dc", "0x3d4"] {
            stream.extend_from_slice(&raw(&f, k));
        }
        let mut dec = NetDecoder::default();
        let mut events = vec![];
        let mut seq = 1_000u32;
        for chunk in stream.chunks(1460) {
            let seg = parse_tcp(276, &sll2_tcp(8578, seq, chunk)).unwrap();
            seq += chunk.len() as u32;
            events.extend(dec.push(&seg));
        }
        assert!(dec.new_flow);
        let spawns = events.iter().filter(|e| matches!(e, NetEvent::MobSpawn { .. })).count();
        assert!(spawns >= 1 && events.len() >= 6, "events {events:?}");
    }

    #[test]
    fn tracker_follows_spawn_move_kill_and_expires_players() {
        let mut t = NetTracker::default();
        t.apply(&NetEvent::MobSpawn { oid: 1, template: 9, x: 10, y: 215 }, 0);
        t.apply(&NetEvent::MobMove { oid: 1, x: 40, y: 215, to_x: 40 }, 10);
        t.apply(&NetEvent::MobMove { oid: 99, x: 1, y: 1, to_x: 1 }, 10); // spawned before we listened
        assert_eq!(t.mobs[&1], MobRec { template: 9, x: 40, y: 215, last_ms: 10, vx: 0.0, to_x: 40 });
        assert_eq!(t.mobs[&99].template, 0, "unknown template, but the mob is tracked");
        assert_eq!(t.mobs.len(), 2);
        t.apply(&NetEvent::MobSpawn { oid: 99, template: 5, x: 1, y: 1 }, 11);
        assert_eq!(t.mobs[&99].template, 5, "a later spawn fills the template in");
        t.apply(&NetEvent::MobKill { oid: 99 }, 12);
        t.apply(&NetEvent::MobKill { oid: 1 }, 20);
        assert!(t.mobs.is_empty());
        t.apply(&NetEvent::PlayerSpawn { char_id: 7, name: "Bob".into() }, 0);
        t.apply(&NetEvent::PlayerMove { char_id: 7, x: 5, y: 274 }, 1000);
        t.expire_players(5000, 10_000);
        assert_eq!(t.players[&7].pos, Some((5, 274)));
        t.expire_players(20_000, 10_000);
        assert!(t.players.is_empty());
    }

    #[test]
    fn map_change_keeps_only_recent_entities() {
        let mut t = NetTracker::default();
        t.apply(&NetEvent::MobSpawn { oid: 1, template: 3, x: 0, y: 0 }, 1_000); // old map
        t.apply(&NetEvent::MobSpawn { oid: 2, template: 5, x: 0, y: 0 }, 9_500); // new map, just before id settled
        t.retain_recent(10_000, 3_000);
        assert!(!t.mobs.contains_key(&1) && t.mobs.contains_key(&2));
    }

    #[test]
    fn decodes_a_real_drop_and_its_removal() {
        let f = fixtures();
        let e = &f["0x471"]["expect"];
        assert_eq!(
            event(&f, "0x471"),
            NetEvent::DropSpawn {
                oid: e["oid"].as_u64().unwrap() as u32,
                owner: e["owner"].as_u64().unwrap() as u32,
                x: e["x"].as_i64().unwrap() as i32,
                y: e["y"].as_i64().unwrap() as i32,
            }
        );
        assert_eq!(event(&f, "0x472"), NetEvent::DropRemove { oid: f["0x472"]["expect"]["oid"].as_u64().unwrap() as u32 });
    }

    #[test]
    fn tracker_tracks_drops_and_dedupes_the_double_broadcast() {
        let mut t = NetTracker::default();
        let spawn = NetEvent::DropSpawn { oid: 7, owner: 1, x: 100, y: 215 };
        t.apply(&spawn, 0);
        t.apply(&spawn, 1); // the server sends the same drop twice
        assert_eq!(t.drops.len(), 1);
        t.apply(&NetEvent::DropRemove { oid: 7 }, 2);
        assert!(t.drops.is_empty());
        t.apply(&spawn, 3);
        t.expire_drops(100_000, 60_000);
        assert!(t.drops.is_empty(), "stale drops expire");
    }

    #[test]
    fn small_packets_use_the_opcode_aware_constant() {
        // A 12-byte packet where no byte dominates would be mis-decoded by the plain mode.
        // plain: opcode 0x0456, then mixed values; constant K = 0x5e
        let plain: [u8; 12] = [0x56, 0x04, 0xa8, 0x01, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00];
        let k = 0x5e;
        let raw: Vec<u8> = plain.iter().map(|b| b.wrapping_add(k)).collect();
        assert_eq!(decode_payload(&raw), plain);
        // a block dominated by 0xFD (-3) plain values: mode would pick K-3
        let mut plain2 = vec![0x81u8, 0x02, 0x44, 0x55];
        plain2.extend(std::iter::repeat(0xFD).take(40));
        plain2.extend([0u8; 6]);
        let raw2: Vec<u8> = plain2.iter().map(|b| b.wrapping_add(0xa4)).collect();
        assert_eq!(decode_payload(&raw2), plain2, "0xFD-heavy packet must still decode with the right constant");
    }

    #[test]
    fn decodes_real_stat_updates_and_merges_them() {
        let f = fixtures();
        assert_eq!(
            event(&f, "0x7e_exp"),
            NetEvent::Stats(StatUpdate { exp: Some(f["0x7e_exp"]["expect"]["value"].as_u64().unwrap() as u32), ..Default::default() })
        );
        assert_eq!(
            event(&f, "0x7e_hp"),
            NetEvent::Stats(StatUpdate { hp: Some(f["0x7e_hp"]["expect"]["value"].as_u64().unwrap() as u32), ..Default::default() })
        );
        let mut t = NetTracker::default();
        t.apply(&event(&f, "0x7e_exp"), 1);
        t.apply(&event(&f, "0x7e_hp"), 2);
        assert!(t.stats.exp.is_some() && t.stats.hp.is_some(), "partial updates merge, not overwrite");
    }

    #[test]
    fn stat_mask_with_an_unknown_bit_stops_instead_of_misreading() {
        // mask = LEVEL(0x10, size unverified) | HP(0x400): HP's offset depends on LEVEL's size.
        // A level-up captured from a new Beginner (level 2: 66/66 HP, 17/17 MP, 5 AP, 1 SP, 1 EXP).
        let up = hex("7e 00 00 00 01 10 fc 01 00 02 00 00 00 42 00 00 00 42 00 00 00 11 00 00 00 11 00 00 00 05 00 01 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00");
        assert_eq!(parse_event(&up), Some(NetEvent::Stats(StatUpdate { hp: Some(66), max_hp: Some(66), mp: Some(17), max_mp: Some(17), exp: Some(1), level: Some(2), ap: Some(5), sp: Some(1), ..Default::default() })));
        // Spending AP: STR 13 and 4 AP left (byte 2 is 1 on these).
        let spend = hex("7e 00 01 00 01 40 40 00 00 0d 00 04 00 01 02 00");
        assert_eq!(parse_event(&spend), Some(NetEvent::Stats(StatUpdate { ap: Some(4), ..Default::default() })));
        // mask = HP | EXP | FAME(0x20000, size unknown) | MESO: HP and EXP precede the unknown bit and still decode
        let mut q = vec![0u8; 40];
        q[0] = 0x7e;
        q[5..9].copy_from_slice(&(0x400u32 | 0x10000 | 0x20000 | 0x40000).to_le_bytes());
        q[9..13].copy_from_slice(&100u32.to_le_bytes());
        q[13..17].copy_from_slice(&59u32.to_le_bytes());
        assert_eq!(parse_event(&q), Some(NetEvent::Stats(StatUpdate { hp: Some(100), exp: Some(59), ..Default::default() })));
    }

    #[test]
    fn decodes_mesos_job_and_pickups_from_captured_packets() {
        // 2026-10-25, KryJib01 on Maple Island: an 11-meso pickup message, then mesos 555 -> 566 (8-byte field).
        assert_eq!(parse_event(&hex("7e 00 01 00 01 00 00 04 00 2b 02 00 00 00 00 00 00 00 00")), Some(NetEvent::Stats(StatUpdate { meso: Some(555), ..Default::default() })));
        assert_eq!(parse_event(&hex("8b 00 00 00 01 00 0b 00 00 00 00 00 00 00 00 00")), None, "a meso pickup message is not an item");
        // EXP-only: the 8-byte field (421) followed by the same 2 trailing bytes as an HP-only update.
        assert_eq!(parse_event(&hex("7e 00 00 00 01 00 00 01 00 a5 01 00 00 00 00 00 00 00 00")), Some(NetEvent::Stats(StatUpdate { exp: Some(421), ..Default::default() })));
        // One item 4000004 picked up.
        assert_eq!(parse_event(&hex("8b 00 00 00 00 04 09 3d 00 01 00 00 00 00")), Some(NetEvent::Pickup { item: 4000004, qty: 1 }));
        let mut t = NetTracker::default();
        t.apply(&NetEvent::Pickup { item: 4000004, qty: 1 }, 1);
        t.apply(&NetEvent::Pickup { item: 4000004, qty: 2 }, 2);
        assert_eq!(t.pickups.get(&4000004), Some(&3));
        // JOB (0x20) | SP: the job is read (Warrior 100), the walk stops after it (its size is not verified).
        let mut j = hex("7e 00 00 00 01 20 80 00 00 64 00 01 00 00 00");
        assert_eq!(parse_event(&j), Some(NetEvent::Stats(StatUpdate { job: Some(100), ..Default::default() })));
        j[9] = 0x65; // 101 is no job
        assert_eq!(parse_event(&j), None);
        assert!(is_job(0) && is_job(100) && is_job(110) && is_job(132) && is_job(512) && !is_job(101) && !is_job(600) && !is_job(140));
    }

    #[test]
    fn rejects_garbage_coordinates() {
        let mut p = vec![0u8; 210];
        p[0] = 0xc9;
        p[1] = 0x03;
        p[8] = 9;
        p[206] = 0xff;
        p[207] = 0x7f; // x = 32767
        assert_eq!(parse_event(&p), None);
    }

    #[test]
    fn mob_velocity_is_estimated_from_successive_moves() {
        let mut t = NetTracker::default();
        t.apply(&NetEvent::MobSpawn { oid: 1, template: 2, x: 100, y: 215 }, 0);
        t.apply(&NetEvent::MobMove { oid: 1, x: 140, y: 215, to_x: 140 }, 1000); // 40 px in 1 s
        t.apply(&NetEvent::MobMove { oid: 1, x: 180, y: 215, to_x: 180 }, 2000);
        let m = &t.mobs[&1];
        assert!(m.vx > 25.0 && m.vx <= 40.0, "vx {}", m.vx);
        let (px, py) = m.predicted(2500);
        assert!(px > 180 && px <= 200, "predicted {px}");
        assert_eq!(py, 215);
        // Extrapolation is capped at one second of travel.
        assert!(m.predicted(60_000).0 <= 180 + 40);
    }

    #[test]
    fn a_mob_that_stops_decays_to_zero_speed() {
        let mut t = NetTracker::default();
        t.apply(&NetEvent::MobSpawn { oid: 1, template: 2, x: 100, y: 215 }, 0);
        t.apply(&NetEvent::MobMove { oid: 1, x: 160, y: 215, to_x: 160 }, 1000);
        for i in 2..7 {
            t.apply(&NetEvent::MobMove { oid: 1, x: 160, y: 215, to_x: 160 }, i * 1000);
        }
        assert!(t.mobs[&1].vx.abs() < 3.0);
    }

    #[test]
    fn a_registered_opcode_raises_the_lie_detector_alert_and_holds_it() {
        let mut t = NetTracker::default();
        assert!(!t.lie_detector_active(0));
        t.apply(&NetEvent::Alert { kind: AlertKind::LieDetector }, 1_000);
        assert!(t.lie_detector_active(1_001) && t.lie_detector_active(1_000 + NetTracker::ALERT_HOLD_MS - 1));
        assert!(!t.lie_detector_active(1_000 + NetTracker::ALERT_HOLD_MS));
    }

    #[test]
    fn decoder_counts_every_opcode_and_alerts_on_registered_ones() {
        let f = fixtures();
        let mut stream = vec![];
        for k in ["0x29d", "0x3dc", "0x3d4", "0x29d", "0x3dc", "0x3d4", "0x29d", "0x3dc", "0x3d4", "0x29d"] {
            stream.extend_from_slice(&raw(&f, k));
        }
        let mut d = NetDecoder::default();
        d.alert_opcodes.insert(0x03d4, AlertKind::LieDetector);
        let seg = TcpSeg { src: [1, 1, 1, 1], sport: PORT_MIN, dst: [2, 2, 2, 2], dport: 50000, seq: 1, payload: stream };
        let evs = d.push(&seg);
        let alerts = evs.iter().filter(|e| matches!(e, NetEvent::Alert { kind: AlertKind::LieDetector })).count();
        assert!(alerts >= 1, "{evs:?}");
        let seen = d.ops.get(&0x03d4).map(|o| o.count).unwrap_or(0);
        assert_eq!(alerts as u64, seen, "one alert per registered packet");
        assert!(d.ops.get(&0x029d).map_or(0, |o| o.count) >= 2);
        assert!(d.ops.values().all(|o| o.min_len <= o.max_len && o.total_len >= o.min_len as u64));
    }
}

#[cfg(test)]
mod party_tests {
    use super::*;

    fn pkt(key: &str) -> Vec<u8> {
        let f: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/party_packets.json")).unwrap();
        let h = f[key].as_str().unwrap();
        (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn a_created_party_lists_its_leader() {
        let Some(NetEvent::Party { id, members }) = parse_event(&pkt("create586")) else { panic!("not a party") };
        assert_eq!(id, 0x6f3b2);
        assert_eq!(members, vec![PartyMember { char_id: 0x51a5a, name: "shurelia2".into(), job: 100, level: 10, channel: 34 }]);
    }

    #[test]
    fn a_join_lists_both_members_with_their_own_level_and_channel() {
        let Some(NetEvent::Party { id, members }) = parse_event(&pkt("join1267")) else { panic!("not a party") };
        assert_eq!(id, 0x6f3b2);
        assert_eq!(members.len(), 2, "{members:?}");
        assert_eq!((members[0].name.as_str(), members[0].level, members[0].channel), ("shurelia2", 10, 34));
        assert_eq!((members[1].name.as_str(), members[1].job, members[1].level, members[1].channel), ("KryJib01", 0, 8, 38));
    }

    #[test]
    fn a_leave_names_the_character() {
        assert_eq!(parse_event(&pkt("leave8")), Some(NetEvent::PartyLeft { char_id: 0x51a5a }));
    }
}

