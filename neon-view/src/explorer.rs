//! Opcode explorer: the reverse-engineering workbench behind "what to map next".
//!
//! For every (direction, opcode) it keeps recent samples and answers, byte by byte: is this byte constant or does it
//! vary, does a 4-byte field here hold the id of a mob / player / drop we already know, and does the registry already
//! explain it. Nothing here names anything: it shows evidence, a person (or a test) decides what it means.

use neon_protocol::{Dir, Registry};
use neon_world::Event;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet, VecDeque};

const KEEP: usize = 400;
/// Only the first bytes of long packets are analysed (the interesting headers are short).
const MAX_ANALYSED: usize = 512;
const SHOW_SAMPLES: usize = 8;

#[derive(Debug, Default)]
pub struct Ids {
    pub mobs: HashSet<u32>,
    pub players: HashSet<u32>,
    pub drops: HashSet<u32>,
}

impl Ids {
    fn kinds(&self, v: u32) -> [bool; 3] {
        [self.mobs.contains(&v), self.players.contains(&v), self.drops.contains(&v)]
    }
}

#[derive(Debug, Default)]
struct PerOp {
    count: u64,
    lens: BTreeMap<usize, u64>,
    samples: VecDeque<Vec<u8>>,
}

#[derive(Debug, Default)]
pub struct Explorer {
    ops: BTreeMap<(Dir, u16), PerOp>,
    pub ids: Ids,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OffsetInfo {
    pub off: usize,
    pub distinct: usize,
    /// The byte value when it never changes.
    pub constant: Option<u8>,
    pub min: u8,
    pub max: u8,
    /// The three most common byte values here and how often (a byte that is mostly one value is a flag, not a counter).
    pub top: Vec<(u8, usize)>,
    /// Range and distinct count of the little-endian u16 starting here (None at the last byte): ranges that look like
    /// coordinates, ids or timestamps are the first clue to a field's type.
    pub u16_range: Option<(u16, u16)>,
    pub u16_distinct: Option<usize>,
    /// `mob`, `player` or `drop` when a 4-byte field starting here holds known ids (only set on the first byte).
    pub id_ref: Option<String>,
    pub id_share: Option<f64>,
    /// Name of the registry field that covers this byte.
    pub field: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exploration {
    pub dir: String,
    pub op: String,
    pub name: Option<String>,
    pub status: String,
    pub count: u64,
    pub lens: Vec<(usize, u64)>,
    pub dominant_len: usize,
    /// How many retained samples have the dominant length (the per-byte statistics use only these).
    pub samples_used: usize,
    pub offsets: Vec<OffsetInfo>,
    /// Bytes explained by the registry (opcode included) of the analysed bytes, and the same as a share.
    pub explained: usize,
    pub analysed: usize,
    /// Bytes that vary, are not an id reference and are not explained by the registry: the work left.
    pub unexplained_varying: usize,
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Todo {
    pub dir: String,
    pub op: String,
    pub name: Option<String>,
    pub status: String,
    pub count: u64,
    pub unexplained_varying: usize,
    /// count x unexplained varying bytes: how much of what we see is still a mystery.
    pub score: u64,
}

fn dir_str(d: Dir) -> &'static str {
    match d {
        Dir::In => "in",
        Dir::Out => "out",
    }
}

impl Explorer {
    pub fn record(&mut self, dir: Dir, plain: &[u8]) {
        if plain.len() < 2 {
            return;
        }
        let id = u16::from_le_bytes([plain[0], plain[1]]);
        let e = self.ops.entry((dir, id)).or_default();
        e.count += 1;
        *e.lens.entry(plain.len()).or_default() += 1;
        e.samples.push_back(plain[..plain.len().min(MAX_ANALYSED)].to_vec());
        if e.samples.len() > KEEP {
            e.samples.pop_front();
        }
    }

    /// Learn object ids from events we trust, so unknown packets can be tested against them.
    pub fn learn(&mut self, e: &Event) {
        match e {
            Event::MobSpawn { oid, .. } | Event::MobMove { oid, .. } => {
                self.ids.mobs.insert(*oid);
            }
            Event::PlayerSpawn { char_id, .. } | Event::PlayerMove { char_id, .. } => {
                self.ids.players.insert(*char_id);
            }
            Event::DropSpawn { oid, .. } => {
                self.ids.drops.insert(*oid);
            }
            _ => {}
        }
        // ids of real objects are large; tiny numbers would match every counter
        for s in [&mut self.ids.mobs, &mut self.ids.players, &mut self.ids.drops] {
            s.retain(|v| *v >= 100_000);
        }
    }

    pub fn explore(&self, reg: &Registry, dir: Dir, id: u16) -> Option<Exploration> {
        let e = self.ops.get(&(dir, id))?;
        let (&dominant_len, _) = e.lens.iter().max_by_key(|(_, c)| **c)?;
        let used: Vec<&Vec<u8>> = e.samples.iter().filter(|s| s.len() == dominant_len.min(MAX_ANALYSED) && true).collect();
        let n = dominant_len.min(MAX_ANALYSED);
        // the entry (variant) that describes the most recent packet of the dominant length, and the bytes it explains
        let probe = used.last().map(|v| v.as_slice()).unwrap_or(&[]);
        let info = reg.select(dir, probe).or_else(|| reg.get(dir, id));
        let mut cover: Vec<Option<String>> = vec![None; n];
        for span in reg.layout(dir, probe) {
            for b in span.off..(span.off + span.size).min(n) {
                cover[b] = Some(span.name.clone());
            }
        }
        if cover.len() >= 2 && cover[0].is_none() {
            cover[0] = Some("opcode".into());
            cover[1] = Some("opcode".into());
        }
        let mut offsets: Vec<OffsetInfo> = Vec::with_capacity(n);
        for off in 0..n {
            let vals: Vec<u8> = used.iter().map(|s| s[off]).collect();
            let set: HashSet<u8> = vals.iter().copied().collect();
            let mut freq: std::collections::BTreeMap<u8, usize> = Default::default();
            for v in &vals {
                *freq.entry(*v).or_default() += 1;
            }
            let mut top: Vec<(u8, usize)> = freq.into_iter().collect();
            top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            top.truncate(3);
            let u16s: Vec<u16> = if off + 1 < n { used.iter().map(|s| u16::from_le_bytes([s[off], s[off + 1]])).collect() } else { vec![] };
            offsets.push(OffsetInfo {
                off,
                top,
                u16_range: u16s.iter().copied().min().zip(u16s.iter().copied().max()),
                u16_distinct: (!u16s.is_empty()).then(|| u16s.iter().collect::<HashSet<_>>().len()),
                distinct: set.len(),
                constant: (set.len() == 1).then(|| vals[0]),
                min: vals.iter().copied().min().unwrap_or(0),
                max: vals.iter().copied().max().unwrap_or(0),
                id_ref: None,
                id_share: None,
                field: cover[off].clone(),
            });
        }
        // id references: a 4-byte window whose values are mostly known ids and not constant
        if used.len() >= 8 && n >= 6 {
            let mut skip_until = 0;
            for off in 2..n.saturating_sub(3) {
                if off < skip_until {
                    continue;
                }
                let vals: Vec<u32> = used.iter().map(|s| u32::from_le_bytes([s[off], s[off + 1], s[off + 2], s[off + 3]])).collect();
                if vals.iter().collect::<HashSet<_>>().len() < 3 {
                    continue;
                }
                let mut hits = [0usize; 3];
                for v in &vals {
                    for (i, k) in self.ids.kinds(*v).iter().enumerate() {
                        if *k {
                            hits[i] += 1;
                        }
                    }
                }
                let (best, &h) = hits.iter().enumerate().max_by_key(|(_, h)| **h).unwrap();
                let share = h as f64 / vals.len() as f64;
                if share >= 0.6 {
                    offsets[off].id_ref = Some(["mob", "player", "drop"][best].to_string());
                    offsets[off].id_share = Some((share * 1000.0).round() / 1000.0);
                    skip_until = off + 4;
                }
            }
        }
        let mut ref_cover = vec![false; n];
        for o in &offsets {
            if o.id_ref.is_some() {
                for b in o.off..(o.off + 4).min(n) {
                    ref_cover[b] = true;
                }
            }
        }
        let explained = (0..n).filter(|b| cover[*b].is_some() || ref_cover[*b]).count();
        let unexplained_varying = offsets.iter().filter(|o| o.distinct > 1 && o.field.is_none() && !ref_cover[o.off]).count();
        let mut lens: Vec<(usize, u64)> = e.lens.iter().map(|(l, c)| (*l, *c)).collect();
        lens.sort_by(|a, b| b.1.cmp(&a.1));
        lens.truncate(12);
        Some(Exploration {
            dir: dir_str(dir).to_string(),
            op: format!("{id:#06x}"),
            name: info.and_then(|o| o.name.clone()),
            status: info.map(|o| format!("{:?}", o.status).to_lowercase()).unwrap_or_else(|| "unregistered".into()),
            count: e.count,
            lens,
            dominant_len,
            samples_used: used.len(),
            offsets,
            explained,
            analysed: n,
            unexplained_varying,
            samples: e.samples.iter().rev().take(SHOW_SAMPLES).map(|s| s.iter().take(96).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")).collect(),
        })
    }

    /// Opcodes ordered by how much of their traffic is still unexplained.
    pub fn todo(&self, reg: &Registry, limit: usize) -> Vec<Todo> {
        let mut out: Vec<Todo> = self
            .ops
            .keys()
            .filter_map(|(d, id)| self.explore(reg, *d, *id))
            .map(|x| Todo {
                score: x.count * x.unexplained_varying as u64,
                dir: x.dir,
                op: x.op,
                name: x.name,
                status: x.status,
                count: x.count,
                unexplained_varying: x.unexplained_varying,
            })
            .filter(|t| t.score > 0)
            .collect();
        out.sort_by(|a, b| b.score.cmp(&a.score));
        out.truncate(limit);
        out
    }
}
