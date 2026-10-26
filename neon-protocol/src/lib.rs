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

#[derive(Debug, Clone, Deserialize)]
pub struct Opcode {
    pub id: u16,
    pub dir: Dir,
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
    ops: BTreeMap<(Dir, u16), Opcode>,
}

impl Registry {
    /// The registry compiled into the crate.
    pub fn builtin() -> Result<Self, Error> {
        Self::parse(SOURCE)
    }

    pub fn parse(text: &str) -> Result<Self, Error> {
        let file: File = toml::from_str(text).map_err(|e| Error::Parse(e.to_string()))?;
        let mut ops = BTreeMap::new();
        let mut problems = Vec::new();
        for op in file.opcode {
            problems.extend(validate(&op));
            let key = (op.dir, op.id);
            if ops.insert(key, op).is_some() {
                problems.push(format!("duplicate {:?} {:#06x}", key.0, key.1));
            }
        }
        if problems.is_empty() {
            Ok(Self { ops })
        } else {
            Err(Error::Invalid(problems))
        }
    }

    pub fn get(&self, dir: Dir, id: u16) -> Option<&Opcode> {
        self.ops.get(&(dir, id))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Opcode> {
        self.ops.values()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn count(&self, dir: Dir, status: Status) -> usize {
        self.ops.values().filter(|o| o.dir == dir && o.status == status).count()
    }

    /// Decode a plaintext packet (starting with its 2-byte little-endian opcode) with the registered fields.
    /// Returns None when the opcode is unknown or the packet violates its size bounds or a field runs past the end.
    pub fn decode(&self, dir: Dir, plain: &[u8]) -> Option<Decoded> {
        if plain.len() < 2 {
            return None;
        }
        let id = u16::from_le_bytes([plain[0], plain[1]]);
        let op = self.get(dir, id)?;
        if op.len_min.map_or(false, |m| plain.len() < m) || op.len_max.map_or(false, |m| plain.len() > m) {
            return None;
        }
        let mut fields = Vec::with_capacity(op.field.len());
        for f in &op.field {
            let v = read(plain, f)?;
            if !in_bounds(&v, f) {
                return None;
            }
            fields.push((f.name.clone(), v));
        }
        Some(Decoded { dir, id, name: op.name.clone(), fields })
    }

    /// A Markdown table of the registry (names, status, summaries); counts come from the caller's logs.
    pub fn markdown(&self, dir: Dir) -> String {
        let mut s = String::from("| Opcode | Name | Status | Size | Summary |\n|---|---|---|---|---|\n");
        for o in self.ops.values().filter(|o| o.dir == dir) {
            let size = match (o.len_min, o.len_max) {
                (Some(a), Some(b)) if a == b => a.to_string(),
                (Some(a), Some(b)) => format!("{a}-{b}"),
                (Some(a), None) => format!("{a}+"),
                _ => String::new(),
            };
            s += &format!(
                "| `{:#06x}` | {} | {:?} | {} | {} |\n",
                o.id,
                o.name.as_deref().unwrap_or(""),
                o.status,
                size,
                o.summary.as_deref().unwrap_or("")
            );
        }
        s
    }
}

fn in_bounds(v: &Value, f: &Field) -> bool {
    let n = match v {
        Value::Str(t) => {
            if f.charset.as_deref() == Some("ident") && !t.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
                return false;
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
