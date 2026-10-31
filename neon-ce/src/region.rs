//! Where a scan looks.

use anyhow::{bail, Context, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Private,
    Mapped,
    Image,
    /// Only the images the process has loaded (excluding the main exe), by name.
    Module(String),
    /// Private read/write regions of at least 64 KiB, minus obvious stacks.
    Heap,
    /// An explicit address window `range:BASE+SIZE` (hex), e.g. to stay near a known
    /// field such as the player's HP.
    Range(u64, u64),
}

impl Scope {
    pub fn parse(s: &str) -> Result<Scope> {
        let t = s.trim();
        if let Some(r) = t.strip_prefix("range:") {
            let (b, size) = r
                .split_once('+')
                .context("range needs BASE+SIZE in hex")?;
            return Ok(Scope::Range(
                u64::from_str_radix(b.trim_start_matches("0x"), 16)?,
                u64::from_str_radix(size.trim_start_matches("0x"), 16)?,
            ));
        }
        if let Some(name) = t.strip_prefix("module:") {
            if name.is_empty() {
                bail!("module scope needs a name");
            }
            return Ok(Scope::Module(name.to_string()));
        }
        Ok(match t.to_ascii_lowercase().as_str() {
            "all" | "" => Scope::All,
            "private" | "priv" => Scope::Private,
            "mapped" => Scope::Mapped,
            "image" | "modules" => Scope::Image,
            "heap" => Scope::Heap,
            other => bail!("unknown scope '{other}'"),
        })
    }

    pub fn name(&self) -> String {
        match self {
            Scope::All => "all".into(),
            Scope::Private => "private".into(),
            Scope::Mapped => "mapped".into(),
            Scope::Image => "image".into(),
            Scope::Module(m) => format!("module:{m}"),
            Scope::Heap => "heap".into(),
            Scope::Range(b, s) => format!("range:{b:X}+{s:X}"),
        }
    }
}

/// Default heuristics for `Scope::Heap`.
pub const HEAP_MIN: u64 = 64 << 10;
