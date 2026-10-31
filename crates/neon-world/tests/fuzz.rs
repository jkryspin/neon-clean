//! The decoders read packets from a network: they must never panic, whatever arrives.
//! Deterministic (xorshift), so a failure reproduces.

use neon_protocol::{Dir, Registry};
use neon_world::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
}

fn plain(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn real_packets() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for src in [
        include_str!("../../neon-protocol/tests/fixtures/plain_packets.json"),
        include_str!("../../neon-protocol/tests/fixtures/combat_packets.json"),
        include_str!("../../neon-protocol/tests/fixtures/out_packets.json"),
        include_str!("../../neon-protocol/tests/fixtures/party_packets.json"),
    ] {
        let v: serde_json::Value = serde_json::from_str(src).unwrap();
        for e in v.as_object().unwrap().values() {
            let hex = e
                .get("plain")
                .and_then(|p| p.as_str())
                .or_else(|| e.as_str())
                .unwrap();
            out.push(plain(hex));
        }
    }
    out
}

fn exercise(reg: &Registry, p: &[u8]) {
    for dir in [Dir::In, Dir::Out] {
        let _ = reg.decode(dir, p);
        let _ = reg.layout(dir, p);
        let _ = reg.select(dir, p);
    }
    let _ = event_from_packet(reg, p);
    let _ = event_from_out_packet(reg, p);
    let _ = decode::parse_stats(p);
    let _ = decode::parse_party(p);
    let _ = decode::parse_party_members(p);
    let _ = decode::parse_pickup(p);
    let _ = decode::parse_inventory(p);
    let _ = decode::parse_exp_gain(p);
    let _ = decode::parse_equips(p);
}

#[test]
fn every_truncation_of_every_real_packet_decodes_or_is_rejected_without_panicking() {
    let reg = Registry::builtin().unwrap();
    let packets = real_packets();
    assert!(packets.len() > 30);
    let mut n = 0;
    for p in &packets {
        for len in 0..=p.len() {
            exercise(&reg, &p[..len]);
            n += 1;
        }
    }
    assert!(n > 3_000, "only {n} truncations ran");
}

#[test]
fn single_byte_corruptions_of_real_packets_never_panic() {
    let reg = Registry::builtin().unwrap();
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for p in real_packets() {
        for _ in 0..300 {
            let mut q = p.clone();
            let i = (rng.next() as usize) % q.len().max(1);
            if !q.is_empty() {
                q[i] = rng.next() as u8;
            }
            exercise(&reg, &q);
        }
    }
}

#[test]
fn random_packets_with_registered_opcodes_never_panic() {
    let reg = Registry::builtin().unwrap();
    let ids: Vec<(Dir, u16)> = reg.iter().map(|o| (o.dir, o.id)).collect();
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    for _ in 0..20_000 {
        let (dir, id) = ids[(rng.next() as usize) % ids.len()];
        let len = (rng.next() % 400) as usize;
        let mut p: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        if p.len() >= 2 {
            p[0] = id as u8;
            p[1] = (id >> 8) as u8;
        }
        let _ = reg.decode(dir, &p);
        let _ = reg.layout(dir, &p);
        exercise(&reg, &p);
    }
}

#[test]
fn replaying_garbage_lines_never_panics_and_counts_them() {
    let reg = Registry::builtin().unwrap();
    let mut rng = Rng(77);
    let mut lines: Vec<String> = Vec::new();
    for _ in 0..500 {
        let n = (rng.next() % 40) as usize;
        let junk: String = (0..n)
            .map(|_| (0x20 + (rng.next() % 90) as u8) as char)
            .collect();
        lines.push(junk);
    }
    lines.push("1.0 op 0028 len 2: 28 00".into());
    lines.push(r#"{"t":1,"d":"in","hex":"2800"}"#.into());
    lines.push(r#"{"t":"x"}"#.into());
    let (w, st) = replay(&reg, lines.clone());
    assert!(st.packets >= 1 && st.bad_lines >= 400);
    let _ = w.snapshot();
    let s = record::Session::from_lines(lines);
    assert!(s.records.len() >= 1);
}
