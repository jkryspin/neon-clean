//! Live mob positions when the network goes quiet.
//!
//! The server sends mob moves only to players who do not control the mobs. A client alone on a map controls them all
//! (its own moves go out encrypted), so the network list keeps every mob at the spot it spawned while the real mobs
//! wander off; the bot swung at empty spawn points and was hit from the side. The hitbox rects in memory follow every
//! entity, but say nothing about which entity is which. This joins the two: each stale network mob (oid, template)
//! takes the position of the rect nearest to where it was last seen, keeping that rect from tick to tick.

use std::collections::HashMap;

use neon_state::Mob;

/// A network position this fresh is used as is.
pub const FRESH_MS: u32 = 1_500;
/// A mob matched within this time follows its rect only within `NEAR`; one lost for longer may jump to any rect.
const TRACK_MS: u64 = 1_000;
const NEAR: i32 = 120;
/// A followed mob whose rect is missing stays at its last spot this long.
const HOLD_MS: u64 = 2_000;
/// Cost of a pairing: vertical distance counts double (mobs walk sideways, platforms are far apart vertically).
fn cost(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs() + 2 * (a.1 - b.1).abs()
}

#[derive(Default)]
pub struct Fuser {
    /// oid -> (position, when last matched to a rect or refreshed by the network)
    tracks: HashMap<u32, ((i32, i32), u64)>,
}

impl Fuser {
    /// Mobs with the stale ones moved onto rects. A stale mob that no rect matches is left out: its spawn point is
    /// not where it is. Rects nobody claims (NPCs, signs, effects) are dropped.
    pub fn fuse(&mut self, net: Vec<Mob>, feet: &[(i32, i32)], now_ms: u64) -> Vec<Mob> {
        self.tracks
            .retain(|oid, _| net.iter().any(|m| m.oid == Some(*oid)));
        let mut out = vec![];
        let mut stale = vec![];
        for m in net {
            let Some(oid) = m.oid else {
                out.push(m);
                continue;
            };
            if m.age_ms.map_or(false, |a| a <= FRESH_MS) {
                self.tracks.insert(oid, ((m.x, m.y), now_ms));
                out.push(m);
            } else {
                self.tracks.entry(oid).or_insert(((m.x, m.y), 0));
                stale.push(m);
            }
        }
        // Greedy assignment, cheapest pairs first: a mob followed last tick keeps its rect.
        let mut pairs = vec![];
        for (i, m) in stale.iter().enumerate() {
            let (at, seen) = self.tracks[&m.oid.unwrap()];
            let tracked = now_ms.saturating_sub(seen) <= TRACK_MS;
            for (j, &f) in feet.iter().enumerate() {
                let c = cost(at, f);
                if !tracked || c <= NEAR {
                    pairs.push((c, i, j));
                }
            }
        }
        pairs.sort_unstable();
        let (mut mob_used, mut foot_used) = (vec![false; stale.len()], vec![false; feet.len()]);
        for (_, i, j) in pairs {
            if mob_used[i] || foot_used[j] {
                continue;
            }
            mob_used[i] = true;
            foot_used[j] = true;
            self.tracks.insert(stale[i].oid.unwrap(), (feet[j], now_ms));
        }
        // A rect can drop out of a scan for a tick or two: a mob followed until just now stays where it was last seen.
        for (mut m, used) in stale.into_iter().zip(mob_used) {
            let (at, seen) = self.tracks[&m.oid.unwrap()];
            if used || (seen > 0 && now_ms.saturating_sub(seen) <= HOLD_MS) {
                (m.x, m.y) = at;
                m.age_ms = Some(now_ms.saturating_sub(seen).min(u32::MAX as u64) as u32);
                out.push(m);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mob(oid: u32, x: i32, y: i32, age: u32) -> Mob {
        Mob {
            x,
            y,
            id: Some(9),
            hp_percent: None,
            oid: Some(oid),
            age_ms: Some(age),
        }
    }

    #[test]
    fn fresh_network_mobs_pass_through() {
        let mut f = Fuser::default();
        let out = f.fuse(vec![mob(1, 100, 215, 200)], &[(400, 215)], 1000);
        assert_eq!((out[0].x, out[0].y), (100, 215));
    }

    #[test]
    fn stale_mobs_take_the_nearest_rects_and_keep_them() {
        let mut f = Fuser::default();
        // Spawned at 251 and 752; really at 300 and 900 now. A third rect far away is an NPC.
        let net = vec![mob(1, 251, 215, 60_000), mob(2, 752, 215, 60_000)];
        let out = f.fuse(net.clone(), &[(900, 215), (300, 215), (-294, 95)], 10_000);
        let pos: HashMap<u32, (i32, i32)> =
            out.iter().map(|m| (m.oid.unwrap(), (m.x, m.y))).collect();
        assert_eq!(pos[&1], (300, 215));
        assert_eq!(pos[&2], (900, 215));
        // Each follows its own rect as it moves a little per tick, even towards the other's old spot.
        let out = f.fuse(net, &[(340, 215), (860, 215), (-294, 95)], 10_020);
        let pos: HashMap<u32, (i32, i32)> =
            out.iter().map(|m| (m.oid.unwrap(), (m.x, m.y))).collect();
        assert_eq!(pos[&1], (340, 215), "{pos:?}");
        assert_eq!(pos[&2], (860, 215));
    }

    #[test]
    fn a_stale_mob_without_a_rect_is_left_out() {
        let mut f = Fuser::default();
        let out = f.fuse(
            vec![mob(1, 251, 215, 60_000), mob(2, 752, 215, 60_000)],
            &[(260, 215)],
            5_000,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].oid, Some(1));
    }

    #[test]
    fn a_tracked_mob_does_not_jump_to_a_far_rect() {
        let mut f = Fuser::default();
        f.fuse(vec![mob(1, 251, 215, 60_000)], &[(300, 215)], 1_000);
        // Its rect vanished this tick (scan gap); the only rect is across the map: it stays where it was seen.
        let out = f.fuse(vec![mob(1, 251, 215, 60_000)], &[(1500, -265)], 1_020);
        assert_eq!((out[0].x, out[0].y), (300, 215));
        // Gone for longer than the hold: no longer reported, and then free to take a far rect.
        assert!(f
            .fuse(vec![mob(1, 251, 215, 60_000)], &[], 3_500)
            .is_empty());
        let out = f.fuse(vec![mob(1, 251, 215, 60_000)], &[(1500, -265)], 3_520);
        assert_eq!((out[0].x, out[0].y), (1500, -265));
    }
}
