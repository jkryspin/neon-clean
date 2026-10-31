//! Foothold (platform segment) discovery by signature.
//!
//! Live scans showed each segment stored as eight i32 words
//!   x1 y1 x2 y2 | x1 y1 x2 y2 | 1 id
//! (see neon-calib/chains.json). The old static "roots" for these did not survive a
//! map change, so we rescan on every map change under a per-tick byte budget.
//! Stale copies from a previous map can linger in freed memory, so callers should
//! sanity-check that the player stands on the resulting set.

use crate::chains::Mem;
use neon_state::Foothold;
use std::collections::BTreeSet;

const CHUNK: u64 = 0x10000;
const SIG_BYTES: usize = 40;

fn matches(w: &[i32]) -> Option<Foothold> {
    let (x1, y1, x2, y2) = (w[0], w[1], w[2], w[3]);
    let ok = w[4] == x1
        && w[5] == y1
        && w[6] == x2
        && w[7] == y2
        && w[8] == 1
        && (0..5000).contains(&w[9])
        && x1 <= x2
        && (x1 != x2 || y1 != y2)
        && [x1, y1, x2, y2].iter().all(|v| v.abs() < 30_000);
    ok.then_some(Foothold { x1, y1, x2, y2 })
}

pub fn find_in(buf: &[u8]) -> Vec<Foothold> {
    let words: Vec<i32> = buf
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if words.len() < 10 {
        return vec![];
    }
    (0..=words.len() - 10)
        .filter_map(|i| matches(&words[i..i + 10]))
        .collect()
}

pub struct FootholdScanner {
    regions: Vec<(u64, u64)>,
    region: usize,
    addr: u64,
    found: BTreeSet<Foothold>,
    done: bool,
    pub map_id: Option<u32>,
}

impl FootholdScanner {
    pub fn new(regions: Vec<(u64, u64)>) -> Self {
        let addr = regions.first().map(|r| r.0).unwrap_or(0);
        Self {
            regions,
            region: 0,
            addr,
            found: BTreeSet::new(),
            done: false,
            map_id: None,
        }
    }

    /// Start over (call on a map change, with a fresh region list).
    pub fn reset(&mut self, regions: Vec<(u64, u64)>, map_id: Option<u32>) {
        *self = Self::new(regions);
        self.map_id = map_id;
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Walk up to `budget` bytes. Returns the full distinct set exactly once, when
    /// the sweep finishes.
    pub fn tick(&mut self, mem: &dyn Mem, budget: u64) -> Option<Vec<Foothold>> {
        if self.done || self.regions.is_empty() {
            return None;
        }
        let mut spent = 0u64;
        while spent < budget {
            let (start, end) = self.regions[self.region];
            if self.addr >= end {
                self.region += 1;
                if self.region >= self.regions.len() {
                    self.done = true;
                    return Some(self.found.iter().copied().collect());
                }
                self.addr = self.regions[self.region].0;
                continue;
            }
            let base = self.addr.max(start);
            self.addr = base + CHUNK;
            spent += CHUNK;
            if let Some(buf) = mem
                .read(base, CHUNK as usize + SIG_BYTES)
                .or_else(|| mem.read(base, CHUNK as usize))
            {
                self.found.extend(find_in(&buf));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chains::tests::Fake;

    fn seg_words(x1: i32, y1: i32, x2: i32, y2: i32, id: i32) -> Vec<u8> {
        [x1, y1, x2, y2, x1, y1, x2, y2, 1, id]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect()
    }

    #[test]
    fn finds_the_live_hill_segment_layout() {
        // Seen live at 0x3bf3c9d0: (810,-265,900,-265, 810,-265,900,-265, 1, 18)
        let mut buf = vec![0u8; 32];
        buf.extend(seg_words(810, -265, 900, -265, 18));
        buf.extend(vec![0u8; 32]);
        assert_eq!(
            find_in(&buf),
            vec![Foothold {
                x1: 810,
                y1: -265,
                x2: 900,
                y2: -265
            }]
        );
    }

    #[test]
    fn walls_and_slopes_are_kept_zero_length_and_reversed_are_not() {
        assert_eq!(
            find_in(&seg_words(-578, 360, -578, 300, 3)).len(),
            1,
            "a wall is a valid segment"
        );
        assert_eq!(find_in(&seg_words(0, 0, 100, -50, 4)).len(), 1);
        assert!(find_in(&seg_words(5, 5, 5, 5, 1)).is_empty(), "zero-length");
        assert!(find_in(&seg_words(100, 0, 0, 0, 1)).is_empty(), "x1 > x2");
        assert!(find_in(&vec![0u8; 400]).is_empty());
    }

    #[test]
    fn scanner_sweeps_within_budget_and_reports_once() {
        let mut m = Fake::default();
        let region = (0x300000u64, 0x300000 + 3 * CHUNK);
        m.map_zero(region.0, region.1 + 4096);
        m.put(region.0 + CHUNK + 64, &seg_words(0, 274, 500, 274, 7));
        m.put(region.0 + 2 * CHUNK + 8, &seg_words(0, 274, 500, 274, 9)); // duplicate copy
        let mut s = FootholdScanner::new(vec![region]);
        assert!(
            s.tick(&m, CHUNK).is_none(),
            "budget of one chunk cannot finish"
        );
        assert!(!s.is_done());
        let out = s.tick(&m, 10 * CHUNK).expect("sweep completes");
        assert_eq!(
            out,
            vec![Foothold {
                x1: 0,
                y1: 274,
                x2: 500,
                y2: 274
            }],
            "duplicates collapse"
        );
        assert!(s.is_done());
        assert!(s.tick(&m, 10 * CHUNK).is_none(), "reported exactly once");
    }

    #[test]
    fn reset_restarts_the_sweep_for_a_new_map() {
        let mut m = Fake::default();
        let region = (0x400000u64, 0x400000 + CHUNK);
        m.map_zero(region.0, region.1 + 4096);
        m.put(region.0, &seg_words(0, 10, 50, 10, 1));
        let mut s = FootholdScanner::new(vec![region]);
        assert!(s.tick(&m, 10 * CHUNK).is_some());
        s.reset(vec![region], Some(10001000));
        assert!(!s.is_done());
        assert_eq!(s.map_id, Some(10001000));
        assert!(s.tick(&m, 10 * CHUNK).is_some());
    }
}
