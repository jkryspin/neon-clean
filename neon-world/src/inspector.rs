//! Packet inspector: a ring of recent packets and a per-opcode histogram, for mapping opcodes by eye.

use neon_protocol::{Decoded, Dir, Registry, Value};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

const RING: usize = 400;
const HEX_BYTES: usize = 64;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    pub t_ms: u64,
    pub op: String,
    pub len: usize,
    pub name: Option<String>,
    pub status: String,
    /// Registry-decoded fields (verified opcodes only), as text.
    pub fields: Vec<(String, String)>,
    /// First bytes of the plaintext, hex.
    pub hex: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OpStat {
    pub op: String,
    pub name: Option<String>,
    pub status: String,
    pub count: u64,
    pub min_len: usize,
    pub max_len: usize,
}

#[derive(Debug, Default)]
pub struct Inspector {
    ring: VecDeque<Entry>,
    stats: BTreeMap<u16, (u64, usize, usize)>,
    pub total: u64,
}

fn show(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        other => other.as_i64().map(|n| n.to_string()).unwrap_or_default(),
    }
}

impl Inspector {
    pub fn record(&mut self, reg: &Registry, plain: &[u8], t_ms: u64) {
        if plain.len() < 2 {
            return;
        }
        let id = u16::from_le_bytes([plain[0], plain[1]]);
        self.total += 1;
        let st = self.stats.entry(id).or_insert((0, usize::MAX, 0));
        st.0 += 1;
        st.1 = st.1.min(plain.len());
        st.2 = st.2.max(plain.len());
        let decoded: Option<Decoded> = reg.decode(Dir::In, plain);
        let info = reg.get(Dir::In, id);
        let hex: String = plain.iter().take(HEX_BYTES).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
        self.ring.push_back(Entry {
            t_ms,
            op: format!("{id:#06x}"),
            len: plain.len(),
            name: info.and_then(|o| o.name.clone()),
            status: info.map(|o| format!("{:?}", o.status).to_lowercase()).unwrap_or_else(|| "unregistered".into()),
            fields: decoded.map(|d| d.fields.iter().map(|(n, v)| (n.clone(), show(v))).collect()).unwrap_or_default(),
            hex,
        });
        if self.ring.len() > RING {
            self.ring.pop_front();
        }
    }

    pub fn recent(&self, limit: usize) -> Vec<&Entry> {
        self.ring.iter().rev().take(limit).collect()
    }

    pub fn histogram(&self, reg: &Registry) -> Vec<OpStat> {
        let mut v: Vec<OpStat> = self
            .stats
            .iter()
            .map(|(id, (count, lo, hi))| {
                let info = reg.get(Dir::In, *id);
                OpStat {
                    op: format!("{id:#06x}"),
                    name: info.and_then(|o| o.name.clone()),
                    status: info.map(|o| format!("{:?}", o.status).to_lowercase()).unwrap_or_else(|| "unregistered".into()),
                    count: *count,
                    min_len: *lo,
                    max_len: *hi,
                }
            })
            .collect();
        v.sort_by(|a, b| b.count.cmp(&a.count));
        v
    }
}
