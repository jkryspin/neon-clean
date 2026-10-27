//! Neon opcode registry: the one place that says what each MapleStory Classic World packet is.
//!
//! `opcodes.toml` is the source of truth. It records, per direction and opcode, a name (only with evidence), a
//! status, size bounds and, for verified packets, field layouts that [`Registry::decode`] can read generically.
//! Field layouts are tested against real captured packets in `tests/`.

use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

const SOURCE: &str = include_str!("../opcodes.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    /// Server to client.
    In,
    /// Client to server.
    Out,
    /// Client to server exactly as it travelled (opcode numbers are scrambled per connection); no registry entries describe it.
    Wire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Only seen on the wire.
    Observed,
    /// Shape known from payloads; meaning not known.
    Structure,
    /// Decoded by neon-stream, layout not expressed in the registry yet.
    Decoded,
    /// Field layout tested against real packets.
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ty {
    U8,
    U16,
    U32,
    I16,
    I32,
    /// u16 length at `off`, then that many bytes of text.
    Str16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Field {
    pub name: String,
    pub ty: Ty,
    pub off: usize,
    /// Inclusive bounds. For numbers the value; for `str16` the text length. A packet outside them does not decode:
    /// the capture's decoding constant is a heuristic, so noise has to be rejected here, not downstream.
    pub min: Option<i64>,
    pub max: Option<i64>,
    /// `"ident"`: ASCII letters, digits and `_` only (str16 fields).
    pub charset: Option<String>,
}

/// A variant is chosen when the byte at `off` equals `eq` (the packet is long enough to have it).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Cond {
    pub off: usize,
    pub eq: u8,
}

/// A repeated record: `count = (len - start) / stride` records, field offsets relative to each record's start.
/// A packet whose tail is not a whole number of records does not match.
#[derive(Debug, Clone, Deserialize)]
pub struct Records {
    pub name: String,
    pub start: usize,
    pub stride: usize,
    /// Bytes after the last record (a fixed trailer).
    #[serde(default)]
    pub tail: usize,
    /// If set, the byte at this offset must equal the record count: a strong check against accidental matches.
    pub count_at: Option<usize>,
    #[serde(default)]
    pub field: Vec<Field>,
}

impl Records {
    /// How many records a packet of this length and content holds, if it fits the layout.
    pub fn count_in(&self, plain: &[u8]) -> Option<usize> {
        let body = plain.len().checked_sub(self.start + self.tail)?;
        if body % self.stride != 0 {
            return None;
        }
        let n = body / self.stride;
        match self.count_at {
            Some(at) if plain.get(at).map(|b| usize::from(*b)) != Some(n) => None,
            _ => Some(n),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Opcode {
    pub id: u16,
    pub dir: Dir,
    /// Several entries may share an opcode: the first whose `when` holds and whose size fits is used.
    pub when: Option<Cond>,
    pub records: Option<Records>,
    pub name: Option<String>,
    pub status: Status,
    pub summary: Option<String>,
    pub len_min: Option<usize>,
    pub len_max: Option<usize>,
    #[serde(default)]
    pub field: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    U8(u8),
    U16(u16),
    U32(u32),
    I16(i16),
    I32(i32),
    Str(String),
}

impl Value {
    pub fn as_i64(&self) -> Option<i64> {
        Some(match self {
            Value::U8(v) => *v as i64,
            Value::U16(v) => *v as i64,
            Value::U32(v) => *v as i64,
            Value::I16(v) => *v as i64,
            Value::I32(v) => *v as i64,
            Value::Str(_) => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub dir: Dir,
    pub id: u16,
    pub name: Option<String>,
    pub fields: Vec<(String, Value)>,
}

impl Decoded {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

#[derive(Debug)]
pub enum Error {
    Parse(String),
    Invalid(Vec<String>),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Parse(e) => write!(f, "opcodes.toml: {e}"),
            Error::Invalid(v) => write!(f, "registry invalid:\n  {}", v.join("\n  ")),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Deserialize)]
struct File {
    opcode: Vec<Opcode>,
}

#[derive(Debug, Clone)]
pub struct Registry {
    ops: BTreeMap<(Dir, u16), Vec<Opcode>>,
}

/// A byte range of a packet that the registry explains, with the field's name.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub off: usize,
    pub size: usize,
    pub name: String,
}

fn field_size(f: &Field) -> usize {
    match f.ty {
        Ty::U8 => 1,
        Ty::U16 | Ty::I16 | Ty::Str16 => 2,
        Ty::U32 | Ty::I32 => 4,
    }
}

impl Registry {
    /// The registry compiled into the crate.
    pub fn builtin() -> Result<Self, Error> {
        Self::parse(SOURCE)
    }

    pub fn parse(text: &str) -> Result<Self, Error> {
        let file: File = toml::from_str(text).map_err(|e| Error::Parse(e.to_string()))?;
        let mut ops: BTreeMap<(Dir, u16), Vec<Opcode>> = BTreeMap::new();
        let mut problems = Vec::new();
        for op in file.opcode {
            problems.extend(validate(&op));
            let group = ops.entry((op.dir, op.id)).or_default();
            let key = op.when.map(|c| (c.off, c.eq));
            let range = |o: &Opcode| (o.len_min.unwrap_or(0), o.len_max.unwrap_or(usize::MAX));
            let overlaps = |o: &Opcode| {
                let (a0, a1) = range(o);
                let (b0, b1) = range(&op);
                a0 <= b1 && b0 <= a1
            };
            // entries with a record layout discriminate themselves (the layout must fit), so only plain entries can clash
            if group.iter().any(|o| o.when.map(|c| (c.off, c.eq)) == key && overlaps(o) && o.records.is_none() && op.records.is_none()) {
                problems.push(format!(
                    "duplicate {:?} {:#06x}{}",
                    op.dir,
                    op.id,
                    key.map_or(String::new(), |(o, e)| format!(" (when byte {o} == {e})"))
                ));
            }
            group.push(op);
        }
        if problems.is_empty() {
            Ok(Self { ops })
        } else {
            Err(Error::Invalid(problems))
        }
    }

    /// The first entry for an opcode (its primary description), whatever the packet looks like.
    pub fn get(&self, dir: Dir, id: u16) -> Option<&Opcode> {
        self.ops.get(&(dir, id)).and_then(|g| g.first())
    }

    /// Every entry for an opcode (several when it has variants).
    pub fn variants(&self, dir: Dir, id: u16) -> &[Opcode] {
        self.ops.get(&(dir, id)).map_or(&[], |g| g.as_slice())
    }

    /// Entries whose condition holds and whose size fits, in file order (groups are tiny: a Vec is fine).
    fn candidates(&self, dir: Dir, plain: &[u8]) -> Vec<&Opcode> {
        if plain.len() < 2 {
            return Vec::new();
        }
        self.variants(dir, u16::from_le_bytes([plain[0], plain[1]]))
            .iter()
            .filter(|op| {
                op.when.map_or(true, |c| plain.get(c.off) == Some(&c.eq))
                    && op.len_min.map_or(true, |m| plain.len() >= m)
                    && op.len_max.map_or(true, |m| plain.len() <= m)
                    && op.records.as_ref().map_or(true, |r| r.count_in(plain).is_some())
            })
            .collect()
    }

    /// The entry that describes this particular packet: the first candidate whose fields also decode (bounds hold).
    pub fn select(&self, dir: Dir, plain: &[u8]) -> Option<&Opcode> {
        self.candidates(dir, plain).into_iter().find(|op| Self::decode_with(op, dir, plain).is_some())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Opcode> {
        self.ops.values().flatten()
    }

    /// Number of entries (an opcode with variants counts once per variant).
    pub fn len(&self) -> usize {
        self.ops.values().map(|g| g.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn count(&self, dir: Dir, status: Status) -> usize {
        self.iter().filter(|o| o.dir == dir && o.status == status).count()
    }

    /// Decode a plaintext packet (starting with its 2-byte little-endian opcode) with the registered fields of the
    /// variant that fits it. Repeated records come out as `name[i].field`. Returns None when the opcode is unknown,
    /// no variant fits, or a field runs past the end or is out of its bounds.
    pub fn decode(&self, dir: Dir, plain: &[u8]) -> Option<Decoded> {
        self.candidates(dir, plain).into_iter().find_map(|op| Self::decode_with(op, dir, plain))
    }

    fn decode_with(op: &Opcode, dir: Dir, plain: &[u8]) -> Option<Decoded> {
        let id = u16::from_le_bytes([plain[0], plain[1]]);
        let mut fields = Vec::with_capacity(op.field.len());
        for f in &op.field {
            let v = read(plain, f)?;
            if !in_bounds(&v, f) {
                return None;
            }
            fields.push((f.name.clone(), v));
        }
        if let Some(r) = &op.records {
            let n = r.count_in(plain)?;
            for i in 0..n {
                for f in &r.field {
                    let abs = Field { off: r.start + i * r.stride + f.off, ..f.clone() };
                    let v = read(plain, &abs)?;
                    if !in_bounds(&v, &abs) {
                        return None;
                    }
                    fields.push((format!("{}[{i}].{}", r.name, f.name), v));
                }
            }
        }
        Some(Decoded { dir, id, name: op.name.clone(), fields })
    }

    /// Write field values back into a real packet, the exact inverse of [`Registry::decode`].
    ///
    /// `template` is a packet the server really sent (it supplies the bytes no field covers - see `write`'s
    /// note on why building from nothing would be guessing). `fields` are the values to patch in, named exactly
    /// as `decode` reports them, including record fields as `name[i].field`. Unlisted fields keep the
    /// template's value. Returns the patched packet, or an error naming the field that could not be written.
    ///
    /// Round-trip property, enforced by tests over real captures:
    /// `encode_onto(dir, sample, &decode(dir, sample).fields) == sample`.
    pub fn encode_onto(&self, dir: Dir, template: &[u8], fields: &[(String, Value)]) -> Result<Vec<u8>, String> {
        let op = self.select(dir, template).ok_or("no registry entry fits this packet")?;
        let mut out = template.to_vec();
        let mut wanted: std::collections::HashMap<&str, &Value> = std::collections::HashMap::new();
        for (n, v) in fields {
            wanted.insert(n.as_str(), v);
        }
        for f in &op.field {
            if let Some(v) = wanted.remove(f.name.as_str()) {
                write(&mut out, f, v)?;
            }
        }
        if let Some(r) = &op.records {
            let n = r.count_in(template).ok_or("template does not fit the record layout")?;
            for i in 0..n {
                for f in &r.field {
                    let key = format!("{}[{i}].{}", r.name, f.name);
                    if let Some(v) = wanted.remove(key.as_str()) {
                        let abs = Field { off: r.start + i * r.stride + f.off, ..f.clone() };
                        write(&mut out, &abs, v)?;
                    }
                }
            }
        }
        if !wanted.is_empty() {
            let mut unknown: Vec<&str> = wanted.keys().copied().collect();
            unknown.sort();
            return Err(format!("no such field(s) for {:#06x}: {}", op.id, unknown.join(", ")));
        }
        Ok(out)
    }

    /// Which bytes of this packet the registry explains (the opcode itself, fields, record fields).
    pub fn layout(&self, dir: Dir, plain: &[u8]) -> Vec<Span> {
        let mut out = Vec::new();
        let Some(op) = self.select(dir, plain) else { return out };
        out.push(Span { off: 0, size: 2.min(plain.len()), name: "opcode".into() });
        if let Some(c) = op.when {
            out.push(Span { off: c.off, size: 1, name: "mode".into() });
        }
        for f in &op.field {
            out.push(Span { off: f.off, size: field_size(f), name: f.name.clone() });
        }
        if let Some(r) = &op.records {
            for i in 0..r.count_in(plain).unwrap_or(0) {
                for f in &r.field {
                    out.push(Span { off: r.start + i * r.stride + f.off, size: field_size(f), name: format!("{}[{i}].{}", r.name, f.name) });
                }
            }
        }
        out.retain(|s| s.off < plain.len());
        out
    }

    /// A Markdown table of the registry (names, status, summaries); counts come from the caller's logs.
    pub fn markdown(&self, dir: Dir) -> String {
        let mut s = String::from("| Opcode | Name | Status | Size | Summary |\n|---|---|---|---|---|\n");
        for o in self.iter().filter(|o| o.dir == dir) {
            let size = match (o.len_min, o.len_max) {
                (Some(a), Some(b)) if a == b => a.to_string(),
                (Some(a), Some(b)) => format!("{a}-{b}"),
                (Some(a), None) => format!("{a}+"),
                _ => String::new(),
            };
            let cond = o.when.map_or(String::new(), |c| format!(" (byte {} = {})", c.off, c.eq));
            s += &format!(
                "| `{:#06x}` | {}{} | {:?} | {} | {} |\n",
                o.id,
                o.name.as_deref().unwrap_or(""),
                cond,
                o.status,
                size,
                o.summary.as_deref().unwrap_or("")
            );
        }
        s
    }
}

/// Why `write`/`encode_onto` are patch-based rather than building a packet from nothing: for almost every
/// opcode the registry explains only SOME of the bytes (see each entry's "unexplained and varying" count in the
/// explorer). Synthesising a packet from field values alone would mean inventing the rest, which would be
/// guessing dressed up as a protocol. Patching a real captured packet is honest about that: the bytes we
/// understand come from the caller, the ones we don't are carried over from a packet the server really sent.
/// This is enough for Tier B replay (take a real packet, change the fields you mean to change) and it is
/// exactly round-trip testable: `encode_onto(sample, decode(sample)) == sample`.
fn write(p: &mut [u8], f: &Field, v: &Value) -> Result<(), String> {
    let need = |n: usize| -> Result<(), String> {
        if f.off + n > p.len() { Err(format!("field {} at {} runs past the {}-byte packet", f.name, f.off, p.len())) } else { Ok(()) }
    };
    match (f.ty, v) {
        (Ty::U8, Value::U8(x)) => { need(1)?; p[f.off] = *x; }
        (Ty::U16, Value::U16(x)) => { need(2)?; p[f.off..f.off + 2].copy_from_slice(&x.to_le_bytes()); }
        (Ty::U32, Value::U32(x)) => { need(4)?; p[f.off..f.off + 4].copy_from_slice(&x.to_le_bytes()); }
        (Ty::I16, Value::I16(x)) => { need(2)?; p[f.off..f.off + 2].copy_from_slice(&x.to_le_bytes()); }
        (Ty::I32, Value::I32(x)) => { need(4)?; p[f.off..f.off + 4].copy_from_slice(&x.to_le_bytes()); }
        (Ty::Str16, Value::Str(s)) => {
            // Only a same-length string can be patched in place: a different length would move every byte after
            // it, including bytes this registry does not understand, so refuse rather than corrupt the packet.
            need(2)?;
            let cur = u16::from_le_bytes([p[f.off], p[f.off + 1]]) as usize;
            if s.len() != cur {
                return Err(format!("field {}: in-place patch needs the same length (packet has {cur}, got {})", f.name, s.len()));
            }
            need(2 + s.len())?;
            p[f.off + 2..f.off + 2 + s.len()].copy_from_slice(s.as_bytes());
        }
        (ty, got) => return Err(format!("field {}: expected {ty:?}, got {got:?}", f.name)),
    }
    if !in_bounds(v, f) {
        return Err(format!("field {}: value {v:?} is outside the registered min/max", f.name));
    }
    Ok(())
}

fn in_bounds(v: &Value, f: &Field) -> bool {
    let n = match v {
        Value::Str(t) => {
            match f.charset.as_deref() {
                Some("ident") if !t.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') => return false,
                // ordinary text: printable ASCII only (anything else is more likely decode noise than a message)
                Some("printable") if !t.bytes().all(|c| (0x20..=0x7e).contains(&c)) => return false,
                _ => {}
            }
            t.len() as i64
        }
        other => other.as_i64().unwrap_or(0),
    };
    f.min.map_or(true, |m| n >= m) && f.max.map_or(true, |m| n <= m)
}

fn read(p: &[u8], f: &Field) -> Option<Value> {
    let at = |n: usize| p.get(f.off..f.off.checked_add(n)?);
    Some(match f.ty {
        Ty::U8 => Value::U8(*at(1)?.first()?),
        Ty::U16 => Value::U16(u16::from_le_bytes(at(2)?.try_into().ok()?)),
        Ty::U32 => Value::U32(u32::from_le_bytes(at(4)?.try_into().ok()?)),
        Ty::I16 => Value::I16(i16::from_le_bytes(at(2)?.try_into().ok()?)),
        Ty::I32 => Value::I32(i32::from_le_bytes(at(4)?.try_into().ok()?)),
        Ty::Str16 => {
            let n = u16::from_le_bytes(at(2)?.try_into().ok()?) as usize;
            let start = f.off + 2;
            Value::Str(String::from_utf8_lossy(p.get(start..start.checked_add(n)?)?).into_owned())
        }
    })
}

fn validate(op: &Opcode) -> Vec<String> {
    let tag = format!("{:?} {:#06x}", op.dir, op.id);
    let mut out = Vec::new();
    if let (Some(a), Some(b)) = (op.len_min, op.len_max) {
        if a > b {
            out.push(format!("{tag}: len_min {a} > len_max {b}"));
        }
    }
    if op.status == Status::Verified {
        if op.name.is_none() {
            out.push(format!("{tag}: verified opcodes need a name"));
        }
        if op.field.is_empty() {
            out.push(format!("{tag}: verified opcodes need at least one field"));
        }
    }
    if op.name.is_some() && op.status == Status::Observed {
        out.push(format!("{tag}: an observed-only opcode must not carry a name"));
    }
    if let (Some(c), Some(min)) = (op.when, op.len_min) {
        if c.off >= min {
            out.push(format!("{tag}: when byte {} is beyond len_min {min}", c.off));
        }
    }
    if let Some(r) = &op.records {
        if r.stride == 0 {
            out.push(format!("{tag}: records {} has stride 0", r.name));
        }
        if let Some(at) = r.count_at {
            if at >= r.start {
                out.push(format!("{tag}: records {} count_at {at} lies inside the records", r.name));
            }
        }
        if r.start < 2 {
            out.push(format!("{tag}: records {} start {} overlaps the opcode", r.name, r.start));
        }
        for f in &r.field {
            if f.off + field_size(f) > r.stride {
                out.push(format!("{tag}: record field {} ends beyond the stride {}", f.name, r.stride));
            }
        }
    }
    let mut seen = HashSet::new();
    for f in &op.field {
        if !seen.insert(&f.name) {
            out.push(format!("{tag}: duplicate field {}", f.name));
        }
        let end = f.off + match f.ty {
            Ty::U8 => 1,
            Ty::U16 | Ty::I16 | Ty::Str16 => 2,
            Ty::U32 | Ty::I32 => 4,
        };
        if let (Some(a), Some(b)) = (f.min, f.max) {
            if a > b {
                out.push(format!("{tag}: field {} min {a} > max {b}", f.name));
            }
        }
        if f.charset.is_some() && f.ty != Ty::Str16 {
            out.push(format!("{tag}: field {} has a charset but is not str16", f.name));
        }
        if let Some(min) = op.len_min {
            if end > min {
                out.push(format!("{tag}: field {} ends at {end}, beyond len_min {min}", f.name));
            }
        }
    }
    out
}

/// Client->server packets carry opcode numbers on the wire that differ from the ones the registry describes (the client's
/// internal numbering), and for game packets the numbering is **different on every connection** (the same keepalive is 0x8786 on
/// one account and 0xf5e8 on another, 2026-10-26). A `WireMap` is the translation for one connection; it is derived from that
/// connection's traffic (`neon-calib/tools/wiremap.py`) and stored next to its log as `{"8786": "02f0", ...}` (hex, no prefix).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WireMap(BTreeMap<u16, u16>);

impl WireMap {
    pub fn from_json(text: &str) -> Result<WireMap, String> {
        let v: BTreeMap<String, String> = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut m = BTreeMap::new();
        for (w, i) in v {
            let w = u16::from_str_radix(w.trim_start_matches("0x"), 16).map_err(|e| format!("wire opcode {w}: {e}"))?;
            let i = u16::from_str_radix(i.trim_start_matches("0x"), 16).map_err(|e| format!("internal opcode {i}: {e}"))?;
            m.insert(w, i);
        }
        Ok(WireMap(m))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The packet with its opcode rewritten to the internal one; `None` when this wire opcode is not in the map.
    pub fn translate(&self, plain: &[u8]) -> Option<Vec<u8>> {
        if plain.len() < 2 {
            return None;
        }
        let id = *self.0.get(&u16::from_le_bytes([plain[0], plain[1]]))?;
        let mut out = plain.to_vec();
        out[..2].copy_from_slice(&id.to_le_bytes());
        Some(out)
    }
}
