//! Player position read from the thread stack, by voting.
//!
//! The fixed chain (`player_position` in chains.json) reads one stack slot, `[root] + 0xF8`. That slot belongs to
//! a call frame, so it is only right while the game is at the same call depth: after some map changes it kept
//! reading the last position from a dead frame. The live position is copied into several frames at once
//! (observed: four slots 0x18..0x270 apart, all updated together while walking), at offsets that depend on the call
//! depth. So instead of one fixed slot: scan the stack for adjacent (x, y) i32 pairs that several slots agree on,
//! remember those slots, and read just them every tick (a majority vote); rescan when the vote is lost.

use crate::chains::Mem;
use std::collections::HashMap;
use std::path::Path;

const MAX_SLOTS: usize = 12;
const RESCAN_MS: u64 = 200;
/// A pair whose y is exactly 0 is usually (y, zero) of a real (x, y) next to it, so it only wins when nothing else does.
/// A new vote this close to the last position wins over a bigger one far away (the character cannot teleport).
const NEAR_PX: i64 = 600;
/// How long a position may stay off every foothold before it stops being trusted (a high jump or a fall).
const AIR_MS: u64 = 1500;
/// Between two scans a live position moves less than this (Manhattan px); anything more is a different thing.
const MOVE_PX: i64 = 300;

/// Foothold segments of the current map, to tell a standing position from noise.
#[derive(Default, Clone)]
pub struct Ground {
    segs: Vec<(i32, i32, i32, i32)>,
}

impl Ground {
    pub fn load(dir: &Path, map: u32) -> Option<Self> {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{map}.json"))).ok()?)
                .ok()?;
        let segs = v
            .get("footholds")?
            .as_array()?
            .iter()
            .filter_map(|f| {
                let g = |k: &str| f.get(k)?.as_i64().map(|n| n as i32);
                Some((g("x1")?, g("y1")?, g("x2")?, g("y2")?))
            })
            .collect::<Vec<_>>();
        (!segs.is_empty()).then_some(Self { segs })
    }

    /// Is (x, y) within a few pixels of a foothold line?
    pub fn on(&self, x: i32, y: i32) -> bool {
        self.segs.iter().any(|&(x1, y1, x2, y2)| {
            let (lo, hi) = (x1.min(x2), x1.max(x2));
            if x < lo || x > hi {
                return false;
            }
            let fy = if x2 == x1 {
                y1.min(y2)
            } else {
                y1 + (y2 - y1) * (x - x1) / (x2 - x1)
            };
            if x2 == x1 {
                (y1.min(y2)..=y1.max(y2)).contains(&y)
            } else {
                (fy - y).abs() <= 4
            }
        })
    }
}

#[derive(Default)]
pub struct StackPos {
    ground: Option<Ground>,
    /// Every slot that was part of a group of two or more at the last scan, with its value then.
    prev: HashMap<u64, (i32, i32)>,
    /// The map has geometry we expect to load: no position is trusted until it has.
    need_ground: bool,
    /// When the accepted position last stood on a foothold; a jump or fall may leave the ground for a moment.
    grounded_at: Option<u64>,
    region: Option<(u64, u64)>,
    slots: Vec<u64>,
    last_scan: Option<u64>,
}

fn plausible(x: i32, y: i32) -> bool {
    x.abs() < 30_000 && y.abs() < 30_000 && x.abs() > 1 && y.abs() < 30_000
}

/// The thread stack among the writable mappings: a low-address region of 256 KB - 4 MB.
pub fn pick_region(regions: &[(u64, u64)]) -> Option<(u64, u64)> {
    regions
        .iter()
        .copied()
        .filter(|(a, b)| *a < 0x100_0000 && (0x4_0000..=0x40_0000).contains(&(b - a)))
        .min_by_key(|(a, _)| *a)
}

impl StackPos {
    pub fn set_region(&mut self, r: Option<(u64, u64)>) {
        if self.region.is_none() {
            self.region = r;
        }
    }

    pub fn set_ground(&mut self, g: Option<Ground>, need: bool) {
        self.ground = g;
        self.need_ground = need;
        self.grounded_at = None;
        self.slots.clear();
    }

    /// True once map geometry is loaded, so the vote is checked against real footholds.
    pub fn validates(&self) -> bool {
        self.ground.is_some()
    }

    pub fn has_region(&self) -> bool {
        self.region.is_some()
    }

    fn pair(mem: &dyn Mem, addr: u64) -> Option<(i32, i32)> {
        let b = mem.read(addr, 8)?;
        let (x, y) = (
            i32::from_le_bytes(b[0..4].try_into().ok()?),
            i32::from_le_bytes(b[4..8].try_into().ok()?),
        );
        plausible(x, y).then_some((x, y))
    }

    /// The pair most remembered slots agree on, if enough of them do.
    fn vote(&self, mem: &dyn Mem) -> Option<(i32, i32)> {
        let mut tally: HashMap<(i32, i32), usize> = HashMap::new();
        for &s in &self.slots {
            if let Some(p) = Self::pair(mem, s) {
                *tally.entry(p).or_default() += 1;
            }
        }
        let (p, n) = tally.into_iter().max_by_key(|(_, n)| *n)?;
        let need = if self.slots.len() >= 4 { 3 } else { 2 };
        (n >= need).then_some(p)
    }

    /// Scan the stack for groups of slots holding the same (x, y). The live group is the one whose slots changed
    /// to a nearby value since the previous scan (a dead frame never changes); with nothing moving, keep the
    /// current slots, or on a cold start take the standing pair with the most copies.
    fn rescan(&mut self, mem: &dyn Mem, hint: Option<(i32, i32)>) {
        let Some((a, b)) = self.region else { return };
        let Some(buf) = mem.read(a, (b - a) as usize) else {
            return;
        };
        let words: Vec<i32> = buf
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let mut seen: HashMap<(i32, i32), Vec<u64>> = HashMap::new();
        for i in 0..words.len().saturating_sub(1) {
            if plausible(words[i], words[i + 1]) {
                seen.entry((words[i], words[i + 1]))
                    .or_default()
                    .push(a + 4 * i as u64);
            }
        }
        let near = |p: &(i32, i32), q: &(i32, i32)| {
            (p.0 as i64 - q.0 as i64).abs() + (p.1 as i64 - q.1 as i64).abs()
        };
        let ok_spot = |p: &(i32, i32)| {
            self.ground.as_ref().map_or(true, |g| g.on(p.0, p.1)) || self.grounded_at.is_some()
        };

        // Slots whose pair moved a little since the last scan, grouped by their new value.
        let mut movers: HashMap<(i32, i32), Vec<u64>> = HashMap::new();
        for (p, addrs) in seen.iter().filter(|(_, v)| v.len() >= 2) {
            for &ad in addrs {
                if let Some(old) = self.prev.get(&ad) {
                    if old != p && near(old, p) <= MOVE_PX {
                        movers.entry(*p).or_default().push(ad);
                    }
                }
            }
        }
        let pick = movers
            .iter()
            .filter(|(p, v)| {
                v.len() >= 2 && ok_spot(p) && hint.map_or(true, |h| near(p, &h) <= NEAR_PX)
            })
            .max_by_key(|(p, v)| (v.len(), p.0, p.1))
            .map(|(_, v)| v.clone())
            .or_else(|| {
                if !self.slots.is_empty() && self.vote(mem).is_some() {
                    return None;
                }
                // Lost (or starting): continue near the last position, else the standing pair with most copies.
                let standing =
                    |p: &(i32, i32)| self.ground.as_ref().map_or(true, |g| g.on(p.0, p.1));
                hint.and_then(|h| {
                    seen.iter()
                        .filter(|(p, v)| v.len() >= 2 && near(p, &h) <= NEAR_PX && standing(p))
                        .min_by_key(|(p, v)| (near(p, &h), std::cmp::Reverse(v.len()), p.0, p.1))
                        .map(|(_, v)| v.clone())
                })
                .or_else(|| {
                    seen.iter()
                        .filter(|(p, v)| v.len() >= 3 && p.1 != 0 && standing(p))
                        .max_by_key(|(p, v)| (v.len(), p.0, p.1))
                        .map(|(_, v)| v.clone())
                })
            });
        if let Some(v) = pick {
            self.slots = v.into_iter().take(MAX_SLOTS).collect();
        }
        self.prev = seen
            .iter()
            .filter(|(_, v)| v.len() >= 2)
            .flat_map(|(p, v)| v.iter().map(move |&ad| (ad, *p)))
            .collect();
    }

    /// Accept a voted position only if it stands on a foothold, or did a moment ago (jumping, falling, climbing).
    fn trusted(&mut self, p: (i32, i32), now_ms: u64) -> bool {
        match &self.ground {
            Some(g) => {
                if g.on(p.0, p.1) {
                    self.grounded_at = Some(now_ms);
                    true
                } else {
                    self.grounded_at
                        .map_or(false, |t| now_ms.saturating_sub(t) <= AIR_MS)
                }
            }
            None if self.need_ground => false,
            None => {
                self.grounded_at = Some(now_ms);
                true
            }
        }
    }

    /// The player's position, or None when no stack slots agree. `hint` is the last good position, if any.
    pub fn read(
        &mut self,
        mem: &dyn Mem,
        now_ms: u64,
        hint: Option<(i32, i32)>,
    ) -> Option<(i32, i32)> {
        if self
            .last_scan
            .map_or(true, |t| now_ms.saturating_sub(t) >= RESCAN_MS)
        {
            self.last_scan = Some(now_ms);
            self.rescan(mem, hint.filter(|_| self.grounded_at.is_some()));
        }
        let p = self.vote(mem)?;
        if self.trusted(p, now_ms) {
            return Some(p);
        }
        self.slots.clear();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A fake stack: base address and bytes.
    struct Fake(RefCell<Vec<u8>>, u64);
    impl Mem for Fake {
        fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
            let d = self.0.borrow();
            let o = addr.checked_sub(self.1)? as usize;
            d.get(o..o + len).map(|s| s.to_vec())
        }
    }
    fn put(f: &Fake, off: usize, x: i32, y: i32) {
        let mut d = f.0.borrow_mut();
        d[off..off + 4].copy_from_slice(&x.to_le_bytes());
        d[off + 4..off + 8].copy_from_slice(&y.to_le_bytes());
    }
    fn fake() -> Fake {
        Fake(RefCell::new(vec![0; 0x1000]), 0x10000)
    }

    #[test]
    fn finds_the_pair_several_slots_agree_on_and_follows_it() {
        let f = fake();
        for off in [0x100, 0x230, 0x2c0, 0x500] {
            put(&f, off, -801, 215);
        }
        put(&f, 0x700, 1176, 65); // a dead frame's single stale copy
        let mut s = StackPos::default();
        s.set_region(Some((0x10000, 0x11000)));
        assert_eq!(s.read(&f, 1000, None), Some((-801, 215)));
        for off in [0x100, 0x230, 0x2c0, 0x500] {
            put(&f, off, -770, 215);
        }
        assert_eq!(
            s.read(&f, 1010, Some((-801, 215))),
            Some((-770, 215)),
            "the remembered slots are read without rescanning"
        );
    }

    #[test]
    fn one_slot_out_of_step_does_not_break_the_vote() {
        let f = fake();
        for off in [0x100, 0x230, 0x2c0, 0x500] {
            put(&f, off, 50, -20);
        }
        let mut s = StackPos::default();
        s.set_region(Some((0x10000, 0x11000)));
        s.read(&f, 0, None);
        put(&f, 0x500, 66, -20); // updated a tick late
        assert_eq!(s.read(&f, 5, None), Some((50, -20)));
    }

    #[test]
    fn nothing_agreeing_means_no_position_not_a_guess() {
        let f = fake();
        put(&f, 0x100, 10, 20);
        put(&f, 0x300, 30, 40);
        let mut s = StackPos::default();
        s.set_region(Some((0x10000, 0x11000)));
        assert_eq!(s.read(&f, 0, None), None);
    }

    #[test]
    fn after_the_vote_is_lost_the_nearest_new_pair_wins() {
        let f = fake();
        for off in [0x100, 0x230, 0x2c0] {
            put(&f, off, 400, 100);
        }
        let mut s = StackPos::default();
        s.set_region(Some((0x10000, 0x11000)));
        assert_eq!(s.read(&f, 0, None), Some((400, 100)));
        // the frames move to other offsets: old slots now hold garbage, a far pair has more copies, a near pair has two
        for off in [0x100, 0x230, 0x2c0] {
            put(&f, off, 0, 0);
        }
        for off in [0x400, 0x440, 0x480, 0x4c0] {
            put(&f, off, -2000, 300);
        }
        for off in [0x600, 0x640] {
            put(&f, off, 430, 100);
        }
        assert_eq!(s.read(&f, 1000, Some((400, 100))), Some((430, 100)));
    }

    #[test]
    fn picks_the_low_stack_sized_region() {
        let regions = [
            (0x12000, 0x110000),
            (0x270000, 0x470000),
            (0x7f0000000000, 0x7f0000100000),
            (0x140000, 0x15a000),
        ];
        assert_eq!(pick_region(&regions), Some((0x12000, 0x110000)));
    }

    #[test]
    fn a_cold_start_only_accepts_a_pair_standing_on_a_foothold() {
        let f = fake();
        for off in [0x100, 0x230, 0x2c0, 0x500, 0x540] {
            put(&f, off, 100, 24); // plenty of copies, but in mid-air
        }
        for off in [0x600, 0x640, 0x680] {
            put(&f, off, -800, 215); // fewer copies, on the ground
        }
        let mut s = StackPos::default();
        s.set_region(Some((0x10000, 0x11000)));
        s.set_ground(
            Some(Ground {
                segs: vec![(-900, 215, -700, 215)],
            }),
            true,
        );
        assert_eq!(s.read(&f, 0, None), Some((-800, 215)));
    }

    #[test]
    fn ground_follows_slopes_and_ropes() {
        let g = Ground {
            segs: vec![(0, 0, 100, 50), (300, -80, 300, 20)],
        };
        assert!(g.on(50, 25) && g.on(50, 27) && !g.on(50, 40));
        assert!(g.on(300, -10) && !g.on(300, 40) && !g.on(310, -10));
    }

    #[test]
    fn a_group_that_moves_beats_a_bigger_stale_group() {
        let f = fake();
        for off in [0x100, 0x140, 0x180, 0x1c0, 0x200] {
            put(&f, off, 2417, -115); // dead frame: five copies that never change
        }
        for off in [0x600, 0x640, 0x680] {
            put(&f, off, -829, 215); // live: three copies
        }
        let mut s = StackPos::default();
        s.set_region(Some((0x10000, 0x11000)));
        assert_eq!(
            s.read(&f, 0, None),
            Some((2417, -115)),
            "standing still, the stale group looks as good"
        );
        for off in [0x600, 0x640, 0x680] {
            put(&f, off, -798, 215);
        }
        assert_eq!(
            s.read(&f, 250, None),
            Some((-798, 215)),
            "the first movement exposes the live one"
        );
        assert_eq!(s.read(&f, 500, None), Some((-798, 215)));
    }
}
