//! Collision-rectangle discovery for mobs (and other entities).
//!
//! Live scans found each entity stores its hitbox redundantly as 12 i32 words:
//!   R T L B | W H W H | L T R B     with R-L==W and B-T==H
//! The foot position is ((L+R)/2, B). This is a signature match, not a static
//! root: addresses are transient heap. The tracker keeps CPU and memory bounded
//! by scanning a fixed number of bytes per tick and caching which 64KB blocks
//! contained hits.

use crate::chains::Mem;
use std::collections::{BTreeMap, BTreeSet};

pub const BLOCK: u64 = 0x10000;
const SIG_BYTES: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rect {
    pub addr: u64,
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

impl Rect {
    pub fn w(&self) -> i32 { self.r - self.l }
    pub fn h(&self) -> i32 { self.b - self.t }
    pub fn foot(&self) -> (i32, i32) { ((self.l + self.r) / 2, self.b) }
}

/// Check one 12-word window.
fn matches(w: &[i32]) -> Option<(i32, i32, i32, i32)> {
    let (r, t, l, b, ww, hh) = (w[0], w[1], w[2], w[3], w[4], w[5]);
    let ok = r - l == ww
        && b - t == hh
        && (8..=400).contains(&ww)
        && (8..=400).contains(&hh)
        && w[6] == ww && w[7] == hh
        && w[8] == l && w[9] == t && w[10] == r && w[11] == b
        && l.abs() < 20_000 && r.abs() < 20_000 && t.abs() < 20_000 && b.abs() < 20_000;
    ok.then_some((l, t, r, b))
}

/// Find every rect in `buf` (which starts at address `base`, 4-byte aligned).
pub fn find_in(buf: &[u8], base: u64) -> Vec<Rect> {
    let words: Vec<i32> = buf
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let mut out = vec![];
    if words.len() < 12 {
        return out;
    }
    for i in 0..=words.len() - 12 {
        if let Some((l, t, r, b)) = matches(&words[i..i + 12]) {
            out.push(Rect { addr: base + (i as u64) * 4, l, t, r, b });
        }
    }
    out
}

/// Bounded incremental scanner.
pub struct RectTracker {
    /// block base -> consecutive rescans with no hit
    known: BTreeMap<u64, u32>,
    /// regions (start, end) to sweep for new blocks
    regions: Vec<(u64, u64)>,
    cursor_region: usize,
    cursor_addr: u64,
    pub sweeps_completed: u64,
}

impl RectTracker {
    pub fn new(regions: Vec<(u64, u64)>) -> Self {
        let cursor_addr = regions.first().map(|r| r.0).unwrap_or(0);
        Self { known: BTreeMap::new(), regions, cursor_region: 0, cursor_addr, sweeps_completed: 0 }
    }

    pub fn known_blocks(&self) -> usize { self.known.len() }

    fn read_block(mem: &dyn Mem, base: u64) -> Vec<Rect> {
        mem.read(base, BLOCK as usize + SIG_BYTES)
            .or_else(|| mem.read(base, BLOCK as usize))
            .map(|b| find_in(&b, base))
            .unwrap_or_default()
    }

    /// One tick: rescan cached blocks, then advance discovery by `budget_bytes`.
    /// Returns de-duplicated live rects.
    pub fn tick(&mut self, mem: &dyn Mem, budget_bytes: u64) -> Vec<Rect> {
        let mut found: BTreeSet<Rect> = BTreeSet::new();
        let mut drop = vec![];

        for (&base, misses) in self.known.iter_mut() {
            let rs = Self::read_block(mem, base);
            if rs.is_empty() {
                *misses += 1;
                if *misses >= 20 { drop.push(base); }
            } else {
                *misses = 0;
                found.extend(rs);
            }
        }
        for b in drop { self.known.remove(&b); }

        let mut spent = 0u64;
        while spent < budget_bytes && !self.regions.is_empty() {
            let (start, end) = self.regions[self.cursor_region];
            if self.cursor_addr >= end {
                self.cursor_region += 1;
                if self.cursor_region >= self.regions.len() {
                    self.cursor_region = 0;
                    self.sweeps_completed += 1;
                    self.cursor_addr = self.regions[0].0;
                    break; // finish the sweep on a tick boundary
                }
                self.cursor_addr = self.regions[self.cursor_region].0;
                continue;
            }
            let base = self.cursor_addr.max(start) & !(BLOCK - 1);
            let base = base.max(start);
            self.cursor_addr = base + BLOCK;
            spent += BLOCK;
            if self.known.contains_key(&base) { continue; }
            let rs = Self::read_block(mem, base);
            if !rs.is_empty() {
                self.known.insert(base, 0);
                found.extend(rs);
            }
        }
        found.into_iter().collect()
    }
}

/// Turn rects into mob-like foot positions: drop the player's own rect, rects
/// outside plausible creature sizes, and duplicates of the same foot position.
///
/// When `footholds` is non-empty, a foot must also stand on one (within 6 px): UI
/// rectangles and stale copies don't. With no map geometry yet, nothing is dropped.
pub fn to_mobs(rects: &[Rect], player: (i32, i32), footholds: &[neon_state::Foothold]) -> Vec<(i32, i32)> {
    let mut seen = BTreeSet::new();
    let mut out = vec![];
    for r in rects {
        if !(10..=250).contains(&r.w()) || !(10..=250).contains(&r.h()) { continue; }
        let f = r.foot();
        if (f.0 - player.0).abs() <= 12 && (f.1 - player.1).abs() <= 12 { continue; }
        if !footholds.is_empty() && !neon_state::on_foothold(footholds, f.0, f.1, 6.0) { continue; }
        if seen.insert(f) { out.push(f); }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chains::tests::Fake;

    fn rect_words(l: i32, t: i32, r: i32, b: i32) -> Vec<u8> {
        let (w, h) = (r - l, b - t);
        [r, t, l, b, w, h, w, h, l, t, r, b].iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn finds_the_exact_live_slime_rect() {
        // Values copied from the live dump at 0x3d05b1d8.
        let mut buf = vec![0u8; 64];
        buf.extend(rect_words(986, -291, 1023, -265));
        buf.extend(vec![0u8; 64]);
        let rs = find_in(&buf, 0x1000);
        assert_eq!(rs.len(), 1);
        assert_eq!((rs[0].w(), rs[0].h()), (37, 26));
        assert_eq!(rs[0].foot(), (1004, -265));
        assert_eq!(rs[0].addr, 0x1000 + 64);
    }

    #[test]
    fn rejects_near_misses_and_zeros() {
        let mut bad = rect_words(0, 0, 40, 30);
        bad[8 * 4..8 * 4 + 4].copy_from_slice(&5i32.to_le_bytes()); // break a repeat
        assert!(find_in(&bad, 0).is_empty());
        assert!(find_in(&vec![0u8; 4096], 0).is_empty(), "all-zero memory has W=0, must not match");
        assert!(find_in(&rect_words(0, 0, 3, 3), 0).is_empty(), "too small");
        assert!(find_in(&rect_words(0, 0, 900, 900), 0).is_empty(), "too big");
    }

    #[test]
    fn tracker_discovers_then_rescans_only_known_blocks_and_respects_budget() {
        let mut m = Fake::default();
        // 4 blocks of region; rect lives in block #2
        let region = (0x100000u64, 0x100000 + 4 * BLOCK);
        m.map_zero(region.0, region.1 + 4096);
        m.put(region.0 + 2 * BLOCK + 128, &rect_words(100, -50, 140, -20));
        let mut t = RectTracker::new(vec![region]);

        // budget of 1 block per tick: needs 3 ticks to reach block #2
        assert!(t.tick(&m, BLOCK).is_empty());
        assert!(t.tick(&m, BLOCK).is_empty());
        let r = t.tick(&m, BLOCK);
        assert_eq!(r.len(), 1);
        assert_eq!(t.known_blocks(), 1);

        // moves: rect rewritten in place; cached block is rescanned with zero discovery budget
        m.put(region.0 + 2 * BLOCK + 128, &rect_words(110, -50, 150, -20));
        let r = t.tick(&m, 0);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].l, 110);
    }

    #[test]
    fn dead_block_is_dropped_after_repeated_misses() {
        let mut m = Fake::default();
        let region = (0x200000u64, 0x200000 + BLOCK);
        m.map_zero(region.0, region.1 + 4096);
        m.put(region.0, &rect_words(0, 0, 40, 30));
        let mut t = RectTracker::new(vec![region]);
        assert_eq!(t.tick(&m, BLOCK).len(), 1);
        m.put(region.0, &vec![0u8; 48]); // freed/overwritten
        for _ in 0..25 { t.tick(&m, 0); }
        assert_eq!(t.known_blocks(), 0);
    }

    #[test]
    fn to_mobs_drops_player_dupes_and_odd_sizes() {
        let mk = |l, t, r, b| Rect { addr: 0, l, t, r, b };
        let rs = [
            mk(986, -291, 1023, -265),   // slime
            mk(986, -291, 1023, -265),   // duplicate copy of the same slime
            mk(500, -490, 524, -443),    // 24x47 at player's feet -> the player itself
            mk(0, 0, 4, 4),              // too small
        ];
        let mobs = to_mobs(&rs, (512, -443), &[]);
        assert_eq!(mobs, vec![(1004, -265)]);

        // with geometry: the slime stands on the y=-265 platform and is kept;
        // a UI-rect foot at (10,66) is not on any foothold and is dropped.
        let fh = [neon_state::Foothold { x1: 900, y1: -265, x2: 1100, y2: -265 }];
        let rs2 = [mk(986, -291, 1023, -265), mk(0, 40, 24, 66)];
        assert_eq!(to_mobs(&rs2, (512, -443), &fh), vec![(1004, -265)]);
    }
}
