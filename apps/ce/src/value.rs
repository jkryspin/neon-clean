//! Value types, encoding and comparison for the scanner.
//!
//! Numbers are stored little-endian at their natural width; strings and AOB patterns
//! are stored verbatim. Comparisons decode to `i128` (integers) or `f64` (floats) so a
//! frozen `4 Bytes` entry can never be confused with a neighbouring float.

use anyhow::{bail, Result};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VType {
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    StrAscii,
    StrUtf16,
    Aob,
}

impl VType {
    pub fn parse(s: &str) -> Result<VType> {
        let t = s.trim().to_ascii_lowercase();
        Ok(match t.as_str() {
            "byte" | "1 byte" | "u8" | "b" => VType::U8,
            "2 bytes" | "u16" | "short" | "w" => VType::U16,
            "4 bytes" | "u32" | "int" | "d" => VType::U32,
            "8 bytes" | "u64" | "long" | "q" => VType::U64,
            "float" | "f32" => VType::F32,
            "double" | "f64" => VType::F64,
            "string" | "ascii" | "utf8" => VType::StrAscii,
            "utf16" | "wstring" => VType::StrUtf16,
            "aob" | "bytes" | "pattern" => VType::Aob,
            other => bail!("unknown value type '{other}'"),
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            VType::U8 => "byte",
            VType::U16 => "2 bytes",
            VType::U32 => "4 bytes",
            VType::U64 => "8 bytes",
            VType::F32 => "float",
            VType::F64 => "double",
            VType::StrAscii => "string",
            VType::StrUtf16 => "utf16",
            VType::Aob => "aob",
        }
    }

    /// Fixed width, or 0 when the width comes from the value itself.
    pub fn width(&self) -> usize {
        match self {
            VType::U8 => 1,
            VType::U16 => 2,
            VType::U32 => 4,
            VType::U64 | VType::F64 => 8,
            VType::F32 => 4,
            VType::StrAscii | VType::StrUtf16 | VType::Aob => 0,
        }
    }

    pub fn is_float(&self) -> bool {
        matches!(self, VType::F32 | VType::F64)
    }

    pub fn is_text(&self) -> bool {
        matches!(self, VType::StrAscii | VType::StrUtf16 | VType::Aob)
    }

    /// Encode user input (decimal, 0x hex, float literal, quoted string, AOB).
    pub fn encode(&self, text: &str) -> Result<Vec<u8>> {
        let t = text.trim();
        match self {
            VType::Aob => parse_aob(t),
            VType::StrAscii => Ok(t.trim_matches('"').as_bytes().to_vec()),
            VType::StrUtf16 => {
                let s = t.trim_matches('"');
                let mut out = Vec::with_capacity(s.len() * 2);
                for u in s.encode_utf16() {
                    out.extend_from_slice(&u.to_le_bytes());
                }
                Ok(out)
            }
            VType::F32 => {
                let v: f32 = t.parse().map_err(|_| anyhow::anyhow!("not a float: {t}"))?;
                Ok(v.to_le_bytes().to_vec())
            }
            VType::F64 => {
                let v: f64 = t.parse().map_err(|_| anyhow::anyhow!("not a float: {t}"))?;
                Ok(v.to_le_bytes().to_vec())
            }
            _ => {
                let n = parse_int(t)?;
                let w = self.width();
                let max = if w == 8 {
                    u64::MAX
                } else {
                    (1u64 << (w * 8)) - 1
                };
                if n > max {
                    bail!("{n} does not fit in {}", self.name());
                }
                Ok(n.to_le_bytes()[..w].to_vec())
            }
        }
    }
}

fn parse_int(t: &str) -> Result<u64> {
    let t = t.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return Ok(u64::from_str_radix(h, 16).map_err(|_| anyhow::anyhow!("bad hex: {t}"))?);
    }
    t.parse::<u64>()
        .map_err(|_| anyhow::anyhow!("not a number: {t}"))
}

/// `48 8B ?? 05` -> bytes with `None` for wildcards.
pub fn parse_aob(t: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for tok in t.split_whitespace() {
        if tok == "??" || tok == "?" {
            out.push(0);
            continue;
        }
        if tok.len() != 2 {
            bail!("bad AOB token '{tok}'");
        }
        out.push(
            u8::from_str_radix(tok, 16).map_err(|_| anyhow::anyhow!("bad AOB token '{tok}'"))?,
        );
    }
    if out.is_empty() {
        bail!("empty pattern");
    }
    Ok(out)
}

/// Width of the stored value in bytes.
pub fn value_len(t: VType, v: &[u8]) -> usize {
    if t.width() == 0 {
        v.len()
    } else {
        t.width()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Between,
    Increased,
    Decreased,
    Changed,
    Unchanged,
    SameAsFirst,
}

impl Cmp {
    pub fn parse(s: &str) -> Result<Cmp> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "eq" | "exact" | "=" | "==" => Cmp::Eq,
            "ne" | "!=" => Cmp::Ne,
            "lt" | "<" => Cmp::Lt,
            "gt" | ">" => Cmp::Gt,
            "lte" | "<=" => Cmp::LtEq,
            "gte" | ">=" => Cmp::GtEq,
            "between" | "range" => Cmp::Between,
            "increased" | "inc" | "+" => Cmp::Increased,
            "decreased" | "dec" | "-" => Cmp::Decreased,
            "changed" | "diff" => Cmp::Changed,
            "unchanged" | "same" => Cmp::Unchanged,
            "sameasfirst" | "first" => Cmp::SameAsFirst,
            other => bail!("unknown comparison '{other}'"),
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Cmp::Eq => "eq",
            Cmp::Ne => "ne",
            Cmp::Lt => "lt",
            Cmp::Gt => "gt",
            Cmp::LtEq => "lte",
            Cmp::GtEq => "gte",
            Cmp::Between => "between",
            Cmp::Increased => "increased",
            Cmp::Decreased => "decreased",
            Cmp::Changed => "changed",
            Cmp::Unchanged => "unchanged",
            Cmp::SameAsFirst => "sameasfirst",
        }
    }

    /// Does this comparison need a reference value from the caller?
    pub fn needs_arg(&self) -> bool {
        matches!(
            self,
            Cmp::Eq | Cmp::Ne | Cmp::Lt | Cmp::Gt | Cmp::LtEq | Cmp::GtEq | Cmp::Between
        )
    }
}

#[derive(Clone, Debug)]
enum Num {
    Int(i128),
    Float(f64),
}

fn decode_num(t: VType, b: &[u8]) -> Option<Num> {
    let need = t.width();
    if b.len() < need || need == 0 {
        return None;
    }
    Some(match t {
        VType::U8 => Num::Int(b[0] as i128),
        VType::U16 => Num::Int(u16::from_le_bytes([b[0], b[1]]) as i128),
        VType::U32 => Num::Int(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i128),
        VType::U64 => Num::Int(u64::from_le_bytes(b[..8].try_into().ok()?) as i128),
        VType::F32 => Num::Float(f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64),
        VType::F64 => Num::Float(f64::from_le_bytes(b[..8].try_into().ok()?)),
        _ => return None,
    })
}

fn differs(t: VType, a: &[u8], b: &[u8]) -> bool {
    if t.is_float() {
        return match (decode_num(t, a), decode_num(t, b)) {
            (Some(Num::Float(x)), Some(Num::Float(y))) => (x - y).abs() > 1e-4,
            _ => true,
        };
    }
    if let (Some(Num::Int(x)), Some(Num::Int(y))) = (decode_num(t, a), decode_num(t, b)) {
        return x != y;
    }
    a != b
}

fn num_cmp(cmp: Cmp, a: &Num, b: &Num) -> bool {
    use std::cmp::Ordering::*;
    let ord = match (a, b) {
        (Num::Int(x), Num::Int(y)) => x.cmp(y),
        (Num::Float(x), Num::Float(y)) => x.partial_cmp(y).unwrap_or(Equal),
        (Num::Int(x), Num::Float(y)) => (*x as f64).partial_cmp(y).unwrap_or(Equal),
        (Num::Float(x), Num::Int(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(Equal),
    };
    match cmp {
        Cmp::Eq => ord == Equal,
        Cmp::Ne => ord != Equal,
        Cmp::Lt => ord == Less,
        Cmp::Gt => ord == Greater,
        Cmp::LtEq => ord != Greater,
        Cmp::GtEq => ord != Less,
        _ => false,
    }
}

/// `cur` is the value now, `prev` the value at the previous scan, `first` the value at
/// the first scan. `want`/`want2` are only used by the argument-taking comparisons.
pub fn matches(
    t: VType,
    cmp: Cmp,
    cur: &[u8],
    prev: &[u8],
    first: &[u8],
    want: Option<&[u8]>,
    want2: Option<&[u8]>,
) -> bool {
    match cmp {
        Cmp::Changed => differs(t, cur, prev),
        Cmp::Unchanged => !differs(t, cur, prev),
        Cmp::SameAsFirst => !differs(t, cur, first),
        Cmp::Increased | Cmp::Decreased => {
            if t.is_text() {
                return false;
            }
            match (decode_num(t, cur), decode_num(t, prev)) {
                (Some(Num::Int(x)), Some(Num::Int(y))) => {
                    if cmp == Cmp::Increased {
                        x > y
                    } else {
                        x < y
                    }
                }
                (Some(Num::Float(x)), Some(Num::Float(y))) => {
                    if cmp == Cmp::Increased {
                        x > y + 1e-4
                    } else {
                        x < y - 1e-4
                    }
                }
                _ => false,
            }
        }
        _ => {
            let Some(w) = want else { return false };
            if t == VType::Aob {
                return aob_at(cur, w) == Some(0);
            }
            if t.is_text() {
                return match cmp {
                    Cmp::Eq => cur == w,
                    Cmp::Ne => cur != w,
                    _ => false,
                };
            }
            let (Some(a), Some(b)) = (decode_num(t, cur), decode_num(t, w)) else {
                return false;
            };
            if cmp == Cmp::Between {
                let Some(w2) = want2 else { return false };
                let Some(c) = decode_num(t, w2) else {
                    return false;
                };
                return num_cmp(Cmp::GtEq, &a, &b) && num_cmp(Cmp::LtEq, &a, &c);
            }
            num_cmp(cmp, &a, &b)
        }
    }
}

/// AOB pattern match; bytes equal to 0 are wildcards, 0 is not matched literally.
pub fn aob_at(hay: &[u8], pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || hay.len() < pat.len() {
        return None;
    }
    (0..=hay.len() - pat.len()).find(|&i| {
        pat.iter()
            .enumerate()
            .all(|(j, b)| *b == 0 || hay[i + j] == *b)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_numbers_little_endian() {
        assert_eq!(VType::U32.encode("0x1020304").unwrap(), vec![4, 3, 2, 1]);
        assert_eq!(VType::U16.encode("258").unwrap(), vec![2, 1]);
        assert_eq!(
            VType::F32.encode("1.5").unwrap(),
            1.5f32.to_le_bytes().to_vec()
        );
        assert_eq!(VType::StrAscii.encode("\"ab\"").unwrap(), b"ab".to_vec());
        assert_eq!(VType::StrUtf16.encode("a").unwrap(), vec![b'a', 0]);
    }

    #[test]
    fn rejects_overflow() {
        assert!(VType::U8.encode("256").is_err());
        assert_eq!(VType::U8.encode("255").unwrap(), vec![255]);
    }

    #[test]
    fn parses_aob_with_wildcards() {
        assert_eq!(parse_aob("48 ?? 05").unwrap(), vec![0x48, 0, 0x05]);
        assert!(parse_aob("4").is_err());
    }

    #[test]
    fn compares_same_type() {
        let t = VType::U32;
        let a = t.encode("100").unwrap();
        let b = t.encode("90").unwrap();
        assert!(matches(t, Cmp::Increased, &a, &b, &b, None, None));
        assert!(!matches(t, Cmp::Decreased, &a, &b, &b, None, None));
        assert!(matches(t, Cmp::Eq, &a, &b, &b, Some(&b), None) == false);
        let want = t.encode("100").unwrap();
        assert!(matches(t, Cmp::Eq, &a, &b, &b, Some(&want), None));
        assert!(matches(t, Cmp::Between, &a, &b, &b, Some(&b), Some(&want)));
        assert!(matches(t, Cmp::Changed, &a, &b, &b, None, None));
        assert!(!matches(t, Cmp::Unchanged, &a, &b, &b, None, None));
    }

    #[test]
    fn floats_use_epsilon() {
        let t = VType::F32;
        let a = t.encode("1.00001").unwrap();
        let b = t.encode("1.0").unwrap();
        assert!(!matches(t, Cmp::Changed, &a, &b, &b, None, None));
        let c = t.encode("1.1").unwrap();
        assert!(matches(t, Cmp::Changed, &c, &b, &b, None, None));
    }

    #[test]
    fn aob_matches_with_wildcards() {
        assert_eq!(aob_at(&[1, 2, 3, 4], &[2, 0, 4]), Some(1));
        assert_eq!(aob_at(&[1, 2, 3, 4], &[9]), None);
    }
}
