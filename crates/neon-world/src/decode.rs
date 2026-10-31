//! Decoders for packets whose layout is not a fixed list of offsets (so the registry cannot express them):
//! own stat updates (a bitmask walk), party lists, status messages and inventory slots.
//! Ported from `neon-stream/src/net.rs` (live-verified there); the same sanity ranges are kept because the capture's
//! decoding constant is a heuristic and noise must be rejected.

use serde::Serialize;

fn u16le(p: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(p.get(i..i + 2)?.try_into().ok()?))
}
fn u32le(p: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(p.get(i..i + 4)?.try_into().ok()?))
}

/// The fields of one own stat update (each packet carries only what changed).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
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
    pub fn is_empty(&self) -> bool {
        *self == StatUpdate::default()
    }
}

/// A job id the game has: 0 Beginner, then x00 first job and x10..x32 for the later ones.
pub fn is_job(j: u32) -> bool {
    j == 0
        || ((1..=5).contains(&(j / 100))
            && (j % 100 == 0 || ((1..=3).contains(&(j % 100 / 10)) && j % 10 <= 2)))
}

/// `0x007e`: classic stat bitmask at @5, values from @9 in ascending bit order. Sizes verified live: LEVEL/HP/MAXHP/MP/MAXMP 4,
/// STR/DEX/INT/LUK/AP/SP 2, EXP and MESO 8. JOB (0x20) is read as u16 and the walk stops after it; unknown bits stop the walk.
pub fn parse_stats(p: &[u8]) -> Option<StatUpdate> {
    if p.len() < 13 || p[2] > 1 {
        return None;
    }
    let mask = u32le(p, 5)?;
    let mut u = StatUpdate::default();
    let mut off = 9;
    for bit in (0..32).map(|b| 1u32 << b) {
        if mask & bit == 0 {
            continue;
        }
        if bit == 0x20 {
            u.job = u16le(p, off).map(u32::from).filter(|j| is_job(*j));
            break;
        }
        let n = match bit {
            0x10 | 0x400 | 0x800 | 0x1000 | 0x2000 => 4,
            0x10000 | 0x40000 => 8,
            0x40 | 0x80 | 0x100 | 0x200 | 0x4000 | 0x8000 => 2,
            _ => break,
        };
        let v = match n {
            8 => u32le(p, off)
                .zip(u32le(p, off + 4))
                .map(|(lo, hi)| u64::from(lo) | (u64::from(hi) << 32)),
            4 => u32le(p, off).map(u64::from),
            _ => u16le(p, off).map(u64::from),
        };
        let Some(v) = v else { break };
        off += n;
        let v32 = u32::try_from(v).ok();
        match bit {
            0x400 => u.hp = v32,
            0x800 => u.max_hp = v32,
            0x1000 => u.mp = v32,
            0x2000 => u.max_mp = v32,
            0x10000 => u.exp = v32,
            0x10 => u.level = v32.filter(|l| (1..=250).contains(l)),
            0x4000 => u.ap = v32,
            0x8000 => u.sp = v32,
            0x40000 => u.meso = Some(v).filter(|m| *m < 10_000_000_000),
            _ => {}
        }
    }
    (!u.is_empty()).then_some(u)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PartyMember {
    pub char_id: u32,
    pub name: String,
    pub job: u16,
    pub level: u32,
    /// Zero-based channel.
    pub channel: u32,
}

/// Members of a `0x00a7` party list: `char id u32, name length u16, name, job u16`, the level u32 at `name_end + 8` and the
/// zero-based channel u32 at `name_end + 12` (verified live with two characters). Found by scanning for that shape.
pub fn parse_party_members(p: &[u8]) -> Vec<PartyMember> {
    let mut out: Vec<PartyMember> = vec![];
    let mut i = 12;
    while i + 24 < p.len() {
        let (Some(id), Some(n)) = (u32le(p, i), u16le(p, i + 4)) else {
            break;
        };
        let n = n as usize;
        if (1..=0x00ff_ffff).contains(&id) && (3..=13).contains(&n) {
            let end = i + 6 + n;
            if let Some(name) = p
                .get(i + 6..end)
                .filter(|s| s.iter().all(|c| c.is_ascii_alphanumeric()))
            {
                if let (Some(job), Some(level), Some(channel)) =
                    (u16le(p, end), u32le(p, end + 8), u32le(p, end + 12))
                {
                    if (1..=250).contains(&level)
                        && channel < 64
                        && job < 4000
                        && !out.iter().any(|m| m.char_id == id)
                    {
                        out.push(PartyMember {
                            char_id: id,
                            name: String::from_utf8_lossy(name).into_owned(),
                            job,
                            level,
                            channel,
                        });
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

/// What a `0x00a7` party packet says.
#[derive(Debug, Clone, PartialEq)]
pub enum PartyMsg {
    /// Created (mode 0x0e) or a member joined (0x13): the party id and the member list.
    List { id: u32, members: Vec<PartyMember> },
    /// A member left (mode 0x10), or we left; the party may be disbanded.
    Left { char_id: u32 },
}

pub fn parse_party(p: &[u8]) -> Option<PartyMsg> {
    if p.len() == 8 && p[2] == 0x10 {
        return Some(PartyMsg::Left {
            char_id: u32le(p, 3)?,
        });
    }
    if p.len() > 100 && (p[2] == 0x0e || p[2] == 0x13) {
        let members = parse_party_members(p);
        // The party id sits at 3 in a create and after the joiner's name in a join.
        let id = if p[2] == 0x0e {
            u32le(p, 3)?
        } else {
            u32le(p, 5 + u16le(p, 3)? as usize)?
        };
        return (!members.is_empty()).then_some(PartyMsg::List { id, members });
    }
    None
}

/// `0x008b` status message, type 0 mode 0: an item picked up (id u32@5, quantity u32@9).
pub fn parse_pickup(p: &[u8]) -> Option<(u32, u32)> {
    if p.len() >= 13 && p[2] == 0 && p[4] == 0 {
        let (item, qty) = (u32le(p, 5)?, u32le(p, 9)?);
        return ((1_000_000..10_000_000).contains(&item) && (1..=10_000).contains(&qty))
            .then_some((item, qty));
    }
    None
}

/// `0x0072` inventory change: (tab, slot, item if a new stack, quantity or None when removed).
pub fn parse_inventory(p: &[u8]) -> Option<(u8, u16, Option<u32>, Option<u32>)> {
    if p.len() < 13 {
        return None;
    }
    let (tab, slot) = (p[10], u16le(p, 11)?);
    if !(1..=5).contains(&tab) || slot == 0 || slot > 128 {
        return None;
    }
    match p[9] {
        0 if p.len() >= 34 => {
            let item = u32le(p, 14)?;
            (1_000_000..10_000_000).contains(&item).then_some((
                tab,
                slot,
                Some(item),
                Some(u16le(p, 32)? as u32),
            ))
        }
        1 if p.len() >= 15 => Some((tab, slot, None, Some(u16le(p, 13)? as u32))),
        3 => Some((tab, slot, None, None)),
        _ => None,
    }
}

/// `0x008b` mode 3: experience gained. Flag @3, amount u32@4. Verified: the amount equals the growth of the EXP counter
/// (`0x007e`) in 26 of 26 consecutive kills.
pub fn parse_exp_gain(p: &[u8]) -> Option<u32> {
    if p.len() >= 8 && p[2] == 3 {
        let n = u32le(p, 4)?;
        return (1..=1_000_000_000).contains(&n).then_some(n);
    }
    None
}

/// The equipment list inside a `0x022d` player spawn: `[slot u8][item id u32]` entries, stride 5, ended by 0xFF, somewhere after
/// the name (its offset shifts with the name length and the fields before it). Verified on 55 of 55 real spawn packets: every slot
/// holds one item category only (1 cap 100xxxx, 5 top 104xxxx/105xxxx, 6 bottom 106xxxx, 7 shoes 107xxxx, 8 gloves 108xxxx,
/// 10 shield 109xxxx, 11 weapon 13x/14xxxxx, ...), the classic equipment slots. The cash-shop list that follows is not decoded.
pub fn parse_equips(p: &[u8]) -> Vec<(u8, u32)> {
    let Some(n) = u16le(p, 18).map(usize::from) else {
        return vec![];
    };
    let mut best: Vec<(u8, u32)> = vec![];
    for off in 20 + n..p.len().saturating_sub(6) {
        let mut run = vec![];
        let mut o = off;
        while o + 5 <= p.len() {
            let (slot, id) = (p[o], u32le(p, o + 1).unwrap_or(0));
            if !(1..=60).contains(&slot) || !(1_000_000..2_000_000).contains(&id) {
                break;
            }
            run.push((slot, id));
            o += 5;
        }
        if run.len() >= 3 && run.len() > best.len() && p.get(o) == Some(&0xff) {
            best = run;
        }
    }
    best
}

/// `0x022f` Whisper: char_id@2, an unidentified u32@6, then name, message, name again, message again (length-prefixed).
/// Returns (name, message) from the first copy. See `docs/network-protocol.md` for the evidence (60/60 real packets duplicate it).
pub fn parse_whisper(p: &[u8]) -> Option<(String, String)> {
    let mut o = 11usize;
    let nlen = u16le(p, o)? as usize;
    o += 2;
    let name = std::str::from_utf8(p.get(o..o + nlen)?).ok()?.to_string();
    o += nlen;
    let mlen = u16le(p, o)? as usize;
    o += 2;
    let msg = std::str::from_utf8(p.get(o..o + mlen)?).ok()?.to_string();
    (!name.is_empty() && !msg.is_empty()).then_some((name, msg))
}

/// `0x023d` ShopSign: char_id@2, mode@6 (3 = text follows), u32@10, constant u32@14, then the ad text. `None` for the silent (mode 0) form.
pub fn parse_shop_sign(p: &[u8]) -> Option<String> {
    if p.len() < 20 || p[6] != 3 {
        return None;
    }
    let slen = u16le(p, 18)? as usize;
    let text = std::str::from_utf8(p.get(20..20 + slen)?).ok()?.to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod whisper_shop_tests {
    use super::*;

    #[test]
    fn whisper_reads_the_first_name_and_message() {
        // a real packet from docs/network-protocol.md's evidence: "Huntra" / "cc plz"
        let p = hex("2f02af320500 00e458 4d0b 0600 48756e747261 0600 636320706c7a 0600 48756e747261 0600 636320706c7a e3b03662 e00e0000 50 af320500 000000000000000000000000000000000003 0050");
        let (name, msg) = parse_whisper(&p).expect("parses");
        assert_eq!((name.as_str(), msg.as_str()), ("Huntra", "cc plz"));
    }

    #[test]
    fn shop_sign_reads_the_ad_text_and_the_silent_form_gives_none() {
        // two real packets (docs/network-protocol.md's ShopSign evidence)
        let p = hex("3d 02 22 c5 03 00 03 00 00 00 c7 1c 00 00 02 00 00 00 13 00 64 72 6f 70 73 2c 20 65 71 75 69 70 2c 20 63 68 65 61 70 20 6e 4e 00");
        assert_eq!(parse_shop_sign(&p).as_deref(), Some("drops, equip, cheap"));
        let silent = hex("3d 02 2f f0 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00");
        assert_eq!(parse_shop_sign(&silent), None);
    }

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }
}
