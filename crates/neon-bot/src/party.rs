//! Bots on one machine sharing a directory so they do not fight the same mob.
//!
//! Every bot writes `<dir>/<name>.json` a few times a second: its map and the server object id of the mob it is
//! after. Others in the same map avoid mobs claimed by a peer. Two bots can claim one mob in the same instant, so
//! the lower name wins: the higher one gives the mob up.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// A claim older than this is a bot that stopped or hung.
pub const CLAIM_TTL_MS: u64 = 3_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub name: String,
    pub map: u32,
    pub oid: Option<u32>,
    /// Wall clock (ms since the epoch) when written.
    pub ts: u64,
}

/// Write our claim atomically (temp file, then rename), so a reader never sees half a file.
pub fn publish(dir: &Path, claim: &Claim) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp", claim.name));
    std::fs::write(
        &tmp,
        serde_json::to_vec(claim).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(tmp, dir.join(format!("{}.json", claim.name)))
}

/// The other bots' fresh claims in `map`.
pub fn read_peers(dir: &Path, me: &str, map: u32, now_ms: u64) -> Vec<Claim> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "json"))
        .filter_map(|e| serde_json::from_slice::<Claim>(&std::fs::read(e.path()).ok()?).ok())
        .filter(|c| c.name != me && c.map == map && now_ms.saturating_sub(c.ts) <= CLAIM_TTL_MS)
        .collect()
}

/// Where every other fresh bot is: session name -> map (any map).
pub fn peer_maps(dir: &Path, me: &str, now_ms: u64) -> std::collections::HashMap<String, u32> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Default::default();
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "json"))
        .filter_map(|e| serde_json::from_slice::<Claim>(&std::fs::read(e.path()).ok()?).ok())
        .filter(|c| c.name != me && now_ms.saturating_sub(c.ts) <= CLAIM_TTL_MS)
        .map(|c| (c.name, c.map))
        .collect()
}

/// Object ids this bot must not target: a lower-named peer's claim always, a higher-named peer's claim unless it is
/// the mob we are already on (then they give way).
pub fn blocked_oids(me: &str, my_target: Option<u32>, peers: &[Claim]) -> HashSet<u32> {
    peers
        .iter()
        .filter_map(|c| c.oid.map(|o| (c.name.as_str(), o)))
        .filter(|(name, o)| *name < me || Some(*o) != my_target)
        .map(|(_, o)| o)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(name: &str, map: u32, oid: Option<u32>, ts: u64) -> Claim {
        Claim {
            name: name.into(),
            map,
            oid,
            ts,
        }
    }

    #[test]
    fn lower_name_keeps_a_contested_mob_and_the_higher_gives_way() {
        let a_claims = [claim("b", 1, Some(7), 0)];
        let b_claims = [claim("a", 1, Some(7), 0)];
        assert!(
            blocked_oids("a", Some(7), &a_claims).is_empty(),
            "a is lower: keeps it"
        );
        assert!(
            blocked_oids("b", Some(7), &b_claims).contains(&7),
            "b is higher: gives way"
        );
        // A peer's mob that is not ours is avoided by both.
        assert!(blocked_oids("a", Some(3), &a_claims).contains(&7));
        assert!(blocked_oids("a", None, &[claim("b", 1, None, 0)]).is_empty());
    }

    #[test]
    fn publish_and_read_filter_by_map_age_and_self() {
        let dir = std::env::temp_dir().join(format!("neon-party-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        publish(&dir, &claim("a", 1, Some(1), 10_000)).unwrap();
        publish(&dir, &claim("b", 1, Some(2), 10_000)).unwrap();
        publish(&dir, &claim("c", 2, Some(3), 10_000)).unwrap(); // other map
        publish(&dir, &claim("d", 1, Some(4), 1_000)).unwrap(); // stale
        let peers = read_peers(&dir, "a", 1, 11_000);
        assert_eq!(
            peers.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["b"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
