//! Cheat tables: the subset of Cheat Engine's `.ct` XML we support.
//!
//! Supported: `<CheatEntry>` with `<ID>`, `<Description>`, `<VariableType>`,
//! `<Address>` (absolute hex, `0x` hex, or `Module.exe+OFFSET`), `<Offsets>` pointer
//! chain, `<Value>`, `<Active>`. Everything else is preserved on save only as far as we
//! model it; unknown elements are ignored on load.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::proc::Process;
use crate::value::VType;

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: u32,
    pub description: String,
    pub vtype: String,
    /// `None` for an absolute address.
    pub module: Option<String>,
    pub static_offset: u64,
    pub offsets: Vec<u64>,
    pub value: String,
    pub active: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Table {
    pub path: String,
    pub entries: Vec<Entry>,
}

/// CE names -> our types.
fn vtype_from_ce(s: &str) -> VType {
    match s.trim().to_ascii_lowercase().as_str() {
        "byte" => VType::U8,
        "2 bytes" => VType::U16,
        "8 bytes" => VType::U64,
        "float" => VType::F32,
        "double" => VType::F64,
        "string" => VType::StrAscii,
        "unicode string" => VType::StrUtf16,
        "array of byte" | "aob" => VType::Aob,
        _ => VType::U32,
    }
}

fn ce_from_vtype(t: VType) -> &'static str {
    match t {
        VType::U8 => "Byte",
        VType::U16 => "2 Bytes",
        VType::U32 => "4 Bytes",
        VType::U64 => "8 Bytes",
        VType::F32 => "Float",
        VType::F64 => "Double",
        VType::StrAscii => "String",
        VType::StrUtf16 => "Unicode String",
        VType::Aob => "Array of Byte",
    }
}

fn text_of(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let i = block.find(&open)? + open.len();
    let j = block[i..].find(&close)? + i;
    Some(unescape(block[i..j].trim()))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `"Module.exe+1A2B"`, `"1A2B"` (hex) or `"0x1A2B"`.
pub fn parse_address(s: &str) -> Result<(Option<String>, u64)> {
    let s = s.trim();
    if let Some(plus) = s.rfind('+') {
        let (module, off) = s.split_at(plus);
        let off = u64::from_str_radix(off[1..].trim().trim_start_matches("0x"), 16)
            .with_context(|| format!("bad offset in '{s}'"))?;
        if module.trim().is_empty() {
            bail!("missing module in '{s}'");
        }
        return Ok((Some(module.trim().to_string()), off));
    }
    let hex = s.trim_start_matches("0x").trim_start_matches("0X");
    let a = u64::from_str_radix(hex, 16).with_context(|| format!("bad address '{s}'"))?;
    Ok((None, a))
}

pub fn load(path: &Path) -> Result<Table> {
    let xml = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut entries = Vec::new();
    let mut rest = xml.as_str();
    let mut id = 0u32;
    while let Some(start) = rest.find("<CheatEntry>") {
        let after = &rest[start + "<CheatEntry>".len()..];
        let Some(end) = after.find("</CheatEntry>") else {
            break;
        };
        let block = &after[..end];
        rest = &after[end + "</CheatEntry>".len()..];
        if block.contains("<GroupHeader>") {
            continue;
        }
        let address = match text_of(block, "Address") {
            Some(a) => a,
            None => continue,
        };
        let (module, static_offset) = match parse_address(&address) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let mut offsets = Vec::new();
        if let Some(list) = text_of(block, "Offsets") {
            for part in list.split("<Offset>").skip(1) {
                let Some(end) = part.find("</Offset>") else {
                    continue;
                };
                let v = part[..end].trim().trim_start_matches("0x");
                if let Ok(o) = u64::from_str_radix(v, 16) {
                    offsets.push(o);
                }
            }
        }
        id += 1;
        entries.push(Entry {
            id: text_of(block, "ID")
                .and_then(|v| v.parse().ok())
                .unwrap_or(id),
            description: text_of(block, "Description").unwrap_or_default(),
            vtype: vtype_from_ce(&text_of(block, "VariableType").unwrap_or_default())
                .name()
                .into(),
            module,
            static_offset,
            offsets,
            value: text_of(block, "Value").unwrap_or_default(),
            active: text_of(block, "Active")
                .map(|v| v.trim() == "1")
                .unwrap_or(false),
        });
    }
    if entries.is_empty() {
        bail!("no usable cheat entries in {}", path.display());
    }
    Ok(Table {
        path: path.display().to_string(),
        entries,
    })
}

pub fn save(table: &Table, path: &Path) -> Result<()> {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<CheatTable>\n  <CheatEntries>\n",
    );
    for e in &table.entries {
        let addr = match &e.module {
            Some(m) => format!("{m}+{:X}", e.static_offset),
            None => format!("{:X}", e.static_offset),
        };
        out.push_str("    <CheatEntry>\n");
        out.push_str(&format!("      <ID>{}</ID>\n", e.id));
        out.push_str(&format!(
            "      <Description>\"{}\"</Description>\n",
            escape(&e.description)
        ));
        out.push_str(&format!(
            "      <VariableType>{}</VariableType>\n",
            ce_from_vtype(VType::parse(&e.vtype).unwrap_or(VType::U32))
        ));
        out.push_str(&format!("      <Address>{addr}</Address>\n"));
        if !e.offsets.is_empty() {
            out.push_str("      <Offsets>\n");
            for o in &e.offsets {
                out.push_str(&format!("        <Offset>{o:X}</Offset>\n"));
            }
            out.push_str("      </Offsets>\n");
        }
        out.push_str(&format!("      <Value>{}</Value>\n", escape(&e.value)));
        out.push_str(&format!(
            "      <Active>{}</Active>\n",
            if e.active { 1 } else { 0 }
        ));
        out.push_str("    </CheatEntry>\n");
    }
    out.push_str("  </CheatEntries>\n</CheatTable>\n");
    std::fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Turn an entry into a real address, following the pointer chain when present.
pub fn resolve(proc: &Process, e: &Entry) -> Result<u64> {
    let base = match &e.module {
        Some(m) => {
            let module = proc
                .module_by_name(m)
                .with_context(|| format!("module '{m}' is not loaded"))?;
            module.base + e.static_offset
        }
        None => e.static_offset,
    };
    if e.offsets.is_empty() {
        return Ok(base);
    }
    let mut cur = proc
        .read_ptr(base)
        .with_context(|| format!("pointer at {base:#x} unreadable"))?;
    for (i, off) in e.offsets.iter().enumerate() {
        let last = i + 1 == e.offsets.len();
        if last {
            return Ok(cur + off);
        }
        cur = proc
            .read_ptr(cur + off)
            .with_context(|| format!("pointer at {:#x} unreadable", cur + off))?;
    }
    Ok(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<CheatTable><CheatEntries>
<CheatEntry><ID>1</ID><Description>"hp"</Description><VariableType>4 Bytes</VariableType>
<Address>MapleStory.exe+3BF8E78</Address><Offsets><Offset>360</Offset></Offsets><Active>1</Active><Value>162</Value></CheatEntry>
<CheatEntry><ID>2</ID><Description>"plain"</Description><VariableType>Float</VariableType><Address>1A2B3C</Address></CheatEntry>
<CheatEntry><GroupHeader>1</GroupHeader><Description>"group"</Description></CheatEntry>
</CheatEntries></CheatTable>"#;

    fn tmp(name: &str, body: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn parses_entries_and_skips_groups() {
        let p = tmp("neon_ce_ct_load.ct", DOC);
        let t = load(&p).unwrap();
        assert_eq!(t.entries.len(), 2);
        assert_eq!(t.entries[0].module.as_deref(), Some("MapleStory.exe"));
        assert_eq!(t.entries[0].static_offset, 0x3bf8e78);
        assert_eq!(t.entries[0].offsets, vec![0x360]);
        assert_eq!(t.entries[0].vtype, "4 bytes");
        assert!(t.entries[0].active);
        assert_eq!(t.entries[1].module, None);
        assert_eq!(t.entries[1].static_offset, 0x1a2b3c);
        assert_eq!(t.entries[1].vtype, "float");
    }

    #[test]
    fn round_trips_through_save() {
        let p = tmp("neon_ce_ct_a.ct", DOC);
        let table = load(&p).unwrap();
        let q = std::env::temp_dir().join("neon_ce_ct_b.ct");
        save(&table, &q).unwrap();
        let again = load(&q).unwrap();
        assert_eq!(again.entries.len(), 2);
        assert_eq!(again.entries[0].offsets, vec![0x360]);
        assert_eq!(again.entries[1].static_offset, 0x1a2b3c);
    }

    #[test]
    fn parses_address_forms() {
        assert_eq!(parse_address("1A2B").unwrap(), (None, 0x1a2b));
        assert_eq!(parse_address("0x10").unwrap(), (None, 0x10));
        assert_eq!(
            parse_address("MapleStory.exe+360").unwrap(),
            (Some("MapleStory.exe".into()), 0x360)
        );
        assert!(parse_address("").is_err());
    }
}
