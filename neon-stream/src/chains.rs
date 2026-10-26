//! Static-root pointer chains from `chains.json`.
//!
//! A chain is: read an 8-byte pointer at `root` (an address inside the exe's
//! fixed-base image), add a field offset, read the value. Roots survive restarts;
//! heap addresses do not.

use serde_json::Value;
use std::collections::HashMap;

/// Anything we can read bytes from (the live process, or a test fake).
pub trait Mem {
    fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>>;

    fn u32(&self, addr: u64) -> Option<u32> {
        Some(u32::from_le_bytes(self.read(addr, 4)?.try_into().ok()?))
    }
    fn i32(&self, addr: u64) -> Option<i32> {
        Some(i32::from_le_bytes(self.read(addr, 4)?.try_into().ok()?))
    }
    fn u64(&self, addr: u64) -> Option<u64> {
        Some(u64::from_le_bytes(self.read(addr, 8)?.try_into().ok()?))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Chains {
    /// stats object: root + named u32 field offsets (hp, mp, level, max_hp, ...)
    pub stats_root: Option<u64>,
    pub stats_fields: HashMap<String, u64>,
    /// player position sources, tried in order; each is root + x/y offsets (i32).
    /// A source is only trusted when two back-to-back reads agree: one of them
    /// was seen flickering through garbage, another can point at the wrong object.
    pub pos_sources: Vec<PosSource>,
    /// player state flags: root + SIGNED offsets (relative to the object the root points at)
    pub state_root: Option<u64>,
    pub state_fields: HashMap<String, i64>,
    /// camera sources: each is a root followed by (pointer, +offset) hops; the last sum is the camera X address, Y is
    /// 4 bytes after. Independent roots reach the same object, so the value is voted.
    pub camera: Vec<CamChain>,
    /// The player's position as the client's own f32 pair, updated every game frame (the i32 position sources are
    /// copies that lag it by a frame). Same chain format as the camera; voted.
    pub pos_f32: Vec<CamChain>,
}

/// How far the f32 position may be from the i32 copy (which lags it by a client frame) and still be trusted.
pub const F32_AGREE_PX: i32 = 40;

#[derive(Debug, Clone)]
pub struct CamChain {
    pub root: u64,
    pub offsets: Vec<u64>,
}

#[derive(Debug, Clone, Copy)]
pub struct PosSource {
    pub root: u64,
    pub x: u64,
    pub y: u64,
}

fn hex(v: &Value) -> Option<u64> {
    let s = v.as_str()?;
    u64::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}

/// Hex with an optional leading '-' ("-0x3e4").
fn shex(v: &Value) -> Option<i64> {
    let s = v.as_str()?;
    let (neg, rest) = match s.strip_prefix('-') { Some(r) => (true, r), None => (false, s) };
    let n = i64::from_str_radix(rest.trim_start_matches("0x"), 16).ok()?;
    Some(if neg { -n } else { n })
}

impl Chains {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let c = v.get("chains").ok_or("missing \"chains\"")?;
        let mut out = Chains::default();

        if let Some(s) = c.get("stats_object") {
            out.stats_root = s.get("root").and_then(hex);
            if let Some(Value::Object(fields)) = s.get("fields") {
                for (name, f) in fields {
                    if let Some(off) = f.get("offset").and_then(hex) {
                        out.stats_fields.insert(name.clone(), off);
                    }
                }
            }
        }
        if let Some(st) = c.get("player_state") {
            out.state_root = st.get("root").and_then(hex);
            if let Some(Value::Object(fields)) = st.get("fields") {
                for (name, f) in fields {
                    if let Some(off) = f.get("offset").and_then(shex) {
                        out.state_fields.insert(name.clone(), off);
                    }
                }
            }
        }
        if let Some(Value::Array(list)) = c.get("camera").and_then(|v| v.get("chains")) {
            for e in list {
                let root = e.get("root").and_then(hex);
                let offsets: Vec<u64> = e.get("offsets").and_then(|o| o.as_array()).map(|a| a.iter().filter_map(hex).collect()).unwrap_or_default();
                if let (Some(root), false) = (root, offsets.is_empty()) {
                    out.camera.push(CamChain { root, offsets });
                }
            }
        }
        if let Some(Value::Array(list)) = c.get("player_position_f32").and_then(|v| v.get("chains")) {
            for e in list {
                let root = e.get("root").and_then(hex);
                let offsets: Vec<u64> = e.get("offsets").and_then(|o| o.as_array()).map(|a| a.iter().filter_map(hex).collect()).unwrap_or_default();
                if let (Some(root), false) = (root, offsets.is_empty()) {
                    out.pos_f32.push(CamChain { root, offsets });
                }
            }
        }
        for key in ["player_position", "player_position_alt"] {
            if let Some(p) = c.get(key) {
                if let Some(root) = p.get("root").and_then(hex) {
                    out.pos_sources.push(PosSource {
                        root,
                        x: p.get("x_offset").and_then(hex).unwrap_or(0),
                        y: p.get("y_offset").and_then(hex).unwrap_or(0),
                    });
                }
            }
        }
        Ok(out)
    }

    /// Follow `root` to the object it points at; reject implausible pointers.
    pub fn deref(&self, mem: &dyn Mem, root: u64) -> Option<u64> {
        let p = mem.u64(root)?;
        (0x10000..0x0000_7fff_ffff_ffff).contains(&p).then_some(p)
    }

    fn read_pos(&self, mem: &dyn Mem, src: &PosSource) -> Option<(i32, i32)> {
        let base = self.deref(mem, src.root)?;
        let (x, y) = (mem.i32(base + src.x)?, mem.i32(base + src.y)?);
        (x.abs() < 50_000 && y.abs() < 50_000 && (x, y) != (0, 0)).then_some((x, y))
    }

    /// First source that yields two consecutive agreeing, plausible reads. The
    /// primary location flickers through garbage some of the time, so each source
    /// gets a few attempts before we move on.
    pub fn position(&self, mem: &dyn Mem) -> Option<(i32, i32)> {
        self.pos_sources.iter().find_map(|src| {
            let mut prev = None;
            for _ in 0..5 {
                let cur = self.read_pos(mem, src);
                if cur.is_some() && cur == prev {
                    return cur;
                }
                prev = cur;
            }
            None
        })
    }

    /// The primary i32 source alone: two consecutive agreeing reads out of up to ten. The alternate source is
    /// left out: it sat at (8, 0) in a headless client while the primary flickered, and won over the right value.
    pub fn position_primary(&self, mem: &dyn Mem) -> Option<(i32, i32)> {
        let src = self.pos_sources.first()?;
        let mut prev = None;
        for _ in 0..10 {
            let cur = self.read_pos(mem, src);
            if cur.is_some() && cur == prev {
                return cur;
            }
            prev = cur;
        }
        None
    }

    fn read_camera(&self, mem: &dyn Mem, c: &CamChain) -> Option<(i32, i32)> {
        let mut a = self.deref(mem, c.root)?.checked_add(*c.offsets.first()?)?;
        for k in &c.offsets[1..] {
            a = self.deref(mem, a)?.checked_add(*k)?;
        }
        let (x, y) = (mem.i32(a)?, mem.i32(a + 4)?);
        (x.abs() < 30_000 && y.abs() < 30_000).then_some((x, y))
    }

    fn read_pos_f32(&self, mem: &dyn Mem, c: &CamChain) -> Option<(i32, i32)> {
        let mut a = self.deref(mem, c.root)?.checked_add(*c.offsets.first()?)?;
        for k in &c.offsets[1..] {
            a = self.deref(mem, a)?.checked_add(*k)?;
        }
        let (x, y) = (f32::from_bits(mem.u32(a)?), f32::from_bits(mem.u32(a + 4)?));
        (x.is_finite() && y.is_finite() && x.abs() < 50_000.0 && y.abs() < 50_000.0 && (x, y) != (0.0, 0.0))
            .then(|| (x.round() as i32, y.round() as i32))
    }

    /// The player's position from the client's f32 pair, but only when it agrees with an independent read:
    /// `check` is the i32 position (one frame behind, so within ~`F32_AGREE_PX`). Chains that once matched the player
    /// were seen freezing at an old point after a map change; a vote among them picked the frozen value.
    pub fn position_f32(&self, mem: &dyn Mem, check: Option<(i32, i32)>) -> Option<(i32, i32)> {
        let (cx, cy) = check?;
        self.pos_f32
            .iter()
            .filter_map(|c| self.read_pos_f32(mem, c))
            .find(|&(x, y)| (x - cx).abs() <= F32_AGREE_PX && (y - cy).abs() <= F32_AGREE_PX)
    }

    /// The f32 position with nothing to check it against (the i32 read is missing): two back-to-back reads must agree
    /// (within a few px: the character may move between them).
    pub fn position_f32_unchecked(&self, mem: &dyn Mem) -> Option<(i32, i32)> {
        let c = self.pos_f32.first()?;
        let a = self.read_pos_f32(mem, c)?;
        let b = self.read_pos_f32(mem, c)?;
        ((a.0 - b.0).abs() <= 8 && (a.1 - b.1).abs() <= 8).then_some(b)
    }

    /// The camera (world coordinates of the view's top-left corner): the value most chains agree on, at least two.
    pub fn camera(&self, mem: &dyn Mem) -> Option<(i32, i32)> {
        let mut votes: Vec<((i32, i32), usize)> = vec![];
        for c in &self.camera {
            if let Some(v) = self.read_camera(mem, c) {
                match votes.iter_mut().find(|(p, _)| *p == v) {
                    Some(e) => e.1 += 1,
                    None => votes.push((v, 1)),
                }
            }
        }
        votes.into_iter().filter(|(_, n)| *n >= 2).max_by_key(|(_, n)| *n).map(|(p, _)| p)
    }

    pub fn stat(&self, mem: &dyn Mem, name: &str) -> Option<u32> {
        let base = self.deref(mem, self.stats_root?)?;
        mem.u32(base + *self.stats_fields.get(name)?)
    }

    /// A named u32 flag from the player-state object (signed offset from its base).
    pub fn flag(&self, mem: &dyn Mem, name: &str) -> Option<u32> {
        let base = self.deref(mem, self.state_root?)?;
        let off = *self.state_fields.get(name)?;
        mem.u32(base.checked_add_signed(off)?)
    }

    /// A 0/1 flag that is only trusted when two consecutive reads agree. These flags
    /// were seen flickering to garbage (28671, 32767) between valid values.
    pub fn flag01(&self, mem: &dyn Mem, name: &str) -> Option<bool> {
        let mut prev = None;
        for _ in 0..5 {
            let cur = self.flag(mem, name).filter(|v| *v <= 1);
            if cur.is_some() && cur == prev {
                return cur.map(|v| v == 1);
            }
            prev = cur;
        }
        None
    }

    /// Like `flag`, but only trusted when two back-to-back reads agree and are non-zero.
    pub fn flag_stable(&self, mem: &dyn Mem, name: &str) -> Option<u32> {
        let a = self.flag(mem, name)?;
        (a != 0 && self.flag(mem, name)? == a).then_some(a)
    }

    /// Human-readable dump of every chain: the first thing to run after a restart.
    pub fn verify(&self, mem: &dyn Mem) -> Vec<String> {
        let mut out = vec![];
        for (i, src) in self.pos_sources.iter().enumerate() {
            match self.deref(mem, src.root) {
                Some(b) => out.push(format!(
                    "position source {} root {:#x} -> object {:#x}, raw {:?}",
                    i, src.root, b, self.read_pos(mem, src)
                )),
                None => out.push(format!("position source {}: UNREADABLE (stale root or process not ready)", i)),
            }
        }
        out.push(format!("position (consensus): {:?}", self.position(mem)));
        let mut names: Vec<_> = self.state_fields.keys().cloned().collect();
        names.sort();
        for n in names {
            out.push(format!("  state {:<12} {:+#x} = {:?}", n, self.state_fields[&n], self.flag(mem, &n)));
        }
        match self.stats_root.and_then(|r| self.deref(mem, r)) {
            Some(b) => {
                out.push(format!("stats root -> object {:#x}", b));
                let mut names: Vec<_> = self.stats_fields.keys().cloned().collect();
                names.sort();
                for n in names {
                    out.push(format!("  {:<8} +{:#x} = {:?}", n, self.stats_fields[&n], self.stat(mem, &n)));
                }
            }
            None => out.push("stats root: UNREADABLE (stale root or process not ready)".into()),
        }
        out
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Sparse fake memory for tests.
    #[derive(Default)]
    pub struct Fake {
        pub bytes: BTreeMap<u64, u8>,
        /// Mapped regions: unset bytes inside read as zero (like real memory).
        pub zero: Vec<(u64, u64)>,
    }
    impl Fake {
        pub fn put(&mut self, addr: u64, bytes: &[u8]) {
            for (i, b) in bytes.iter().enumerate() {
                self.bytes.insert(addr + i as u64, *b);
            }
        }
        pub fn map_zero(&mut self, start: u64, end: u64) { self.zero.push((start, end)) }
        pub fn put_u32(&mut self, a: u64, v: u32) { self.put(a, &v.to_le_bytes()) }
        pub fn put_i32(&mut self, a: u64, v: i32) { self.put(a, &v.to_le_bytes()) }
        pub fn put_u64(&mut self, a: u64, v: u64) { self.put(a, &v.to_le_bytes()) }
    }
    impl Mem for Fake {
        fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
            (0..len as u64)
                .map(|i| {
                    let a = addr + i;
                    self.bytes.get(&a).copied().or_else(|| {
                        self.zero.iter().any(|&(s, e)| a >= s && a < e).then_some(0)
                    })
                })
                .collect()
        }
    }

    const JSON: &str = r#"{"chains":{
        "stats_object":{"root":"0x143bf8e78","fields":{
            "hp":{"offset":"0x360"},"mp":{"offset":"0x364"},"level":{"offset":"0x160"}}},
        "player_state":{"root":"0x143bf05a0","fields":{"grounded":{"offset":"-0x3e4"}}},
        "player_position":{"root":"0x143bf05a0","x_offset":"0xf8","y_offset":"0xfc"},
        "player_position_alt":{"root":"0x143bfdd30","x_offset":"0x308","y_offset":"0x30c"}}}"#;

    #[test]
    fn the_f32_position_is_used_only_when_it_agrees_with_the_i32_read() {
        let json = r#"{"chains":{"player_position_f32":{"chains":[{"root":"0x143bd6788","offsets":["0x48","0x98"]}]}}}"#;
        let c = Chains::from_json(json).unwrap();
        let mut m = Fake::default();
        m.put_u64(0x143bd6788, 0x50000);
        m.put_u64(0x50000 + 0x48, 0x90000);
        m.put_u32(0x90000 + 0x98, 5247.0f32.to_bits());
        m.put_u32(0x90000 + 0x9c, 587.0f32.to_bits());
        assert_eq!(c.position_f32(&m, Some((5240, 587))), Some((5247, 587)), "a frame ahead of the i32 copy");
        assert_eq!(c.position_f32(&m, Some((4185, 450))), None, "frozen elsewhere: not the player");
        assert_eq!(c.position_f32(&m, None), None, "nothing to check it against");
    }

    #[test]
    fn parses_the_real_chains_file_shape() {
        let c = Chains::from_json(JSON).unwrap();
        assert_eq!(c.stats_root, Some(0x143bf8e78));
        assert_eq!(c.stats_fields["mp"], 0x364);
        assert_eq!(c.pos_sources.len(), 2);
        assert_eq!((c.pos_sources[0].root, c.pos_sources[0].x), (0x143bf05a0, 0xf8));
        assert_eq!((c.pos_sources[1].root, c.pos_sources[1].x), (0x143bfdd30, 0x308));
    }

    #[test]
    fn reads_position_and_stats_through_pointers() {
        let c = Chains::from_json(JSON).unwrap();
        let mut m = Fake::default();
        m.put_u64(0x143bf05a0, 0x109830);
        m.put_i32(0x109830 + 0xf8, 644);
        m.put_i32(0x109830 + 0xfc, 35);
        m.put_u64(0x143bf8e78, 0x1e8d9338);
        m.put_u32(0x1e8d9338 + 0x360, 146);
        m.put_u32(0x1e8d9338 + 0x364, 77);
        assert_eq!(c.position(&m), Some((644, 35)));
        assert_eq!(c.stat(&m, "hp"), Some(146));
        assert_eq!(c.stat(&m, "mp"), Some(77));
        assert_eq!(c.stat(&m, "exp"), None, "unconfigured field is None, not 0");
    }

    #[test]
    fn reads_a_flag_at_a_negative_offset_from_the_object_base() {
        let c = Chains::from_json(JSON).unwrap();
        assert_eq!(c.state_fields["grounded"], -0x3e4);
        let mut m = Fake::default();
        m.put_u64(0x143bf05a0, 0x109830);
        m.put_u32(0x109830 - 0x3e4, 1); // = 0x10944c, the live address
        assert_eq!(c.flag(&m, "grounded"), Some(1));
        m.put_u32(0x109830 - 0x3e4, 0);
        assert_eq!(c.flag(&m, "grounded"), Some(0));
        assert_eq!(c.flag(&m, "nope"), None);
    }

    #[test]
    fn flag01_rejects_flicker_values() {
        let c = Chains::from_json(JSON).unwrap();
        let mut m = Fake::default();
        m.put_u64(0x143bf05a0, 0x109830);
        m.put_u32(0x109830 - 0x3e4, 28671); // seen live
        assert_eq!(c.flag01(&m, "grounded"), None);
        m.put_u32(0x109830 - 0x3e4, 0);
        assert_eq!(c.flag01(&m, "grounded"), Some(false));
        m.put_u32(0x109830 - 0x3e4, 1);
        assert_eq!(c.flag01(&m, "grounded"), Some(true));
    }

    #[test]
    fn flag_stable_rejects_zero_and_missing() {
        let c = Chains::from_json(JSON).unwrap();
        let mut m = Fake::default();
        m.put_u64(0x143bf05a0, 0x109830);
        m.put_u32(0x109830 - 0x3e4, 0);
        assert_eq!(c.flag_stable(&m, "grounded"), None, "zero is not a usable id");
        m.put_u32(0x109830 - 0x3e4, 10001030);
        assert_eq!(c.flag_stable(&m, "grounded"), Some(10001030));
    }

    #[test]
    fn rejects_garbage_position_and_null_roots() {
        let c = Chains::from_json(JSON).unwrap();
        let mut m = Fake::default();
        m.put_u64(0x143bf05a0, 0x109830);
        m.put_i32(0x109830 + 0xf8, 50858304); // the flicker value seen live
        m.put_i32(0x109830 + 0xfc, 0);
        assert_eq!(c.position(&m), None);
        m.put_u64(0x143bf05a0, 0); // stale/uninitialised root
        assert_eq!(c.position(&m), None);
    }

    #[test]
    fn falls_back_to_the_alt_source_when_primary_is_garbage() {
        let c = Chains::from_json(JSON).unwrap();
        let mut m = Fake::default();
        m.put_u64(0x143bf05a0, 0x109830);
        m.put_i32(0x109830 + 0xf8, 50858304);
        m.put_i32(0x109830 + 0xfc, 0);
        m.put_u64(0x143bfdd30, 0x3aff0088);
        m.put_i32(0x3aff0088 + 0x308, 644);
        m.put_i32(0x3aff0088 + 0x30c, 35);
        assert_eq!(c.position(&m), Some((644, 35)));
    }

    #[test]
    fn a_source_pointing_at_zeros_is_skipped_not_trusted() {
        // What chain B did after the restart: valid pointer, all-zero fields.
        let c = Chains::from_json(JSON).unwrap();
        let mut m = Fake::default();
        m.put_u64(0x143bfdd30, 0x3ccebcc8);
        m.put_i32(0x3ccebcc8 + 0x308, 0);
        m.put_i32(0x3ccebcc8 + 0x30c, 0);
        m.put_u64(0x143bf05a0, 0x109830);
        m.put_i32(0x109830 + 0xf8, 1088);
        m.put_i32(0x109830 + 0xfc, -25);
        assert_eq!(c.position(&m), Some((1088, -25)));
    }

    #[test]
    fn verify_reports_unreadable_roots_instead_of_panicking() {
        let c = Chains::from_json(JSON).unwrap();
        let lines = c.verify(&Fake::default());
        assert!(lines.iter().filter(|l| l.contains("UNREADABLE")).count() >= 3, "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("consensus): None")), "{lines:?}");
    }
}
