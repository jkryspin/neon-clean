use neon_protocol::{Dir, Registry};
use neon_view::{explorer::Explorer, Shared};
use neon_world::Event;

fn plain(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect()
}

/// An unregistered opcode 0x7000, 40 bytes: player id @2 (varies, known), constant 5 @6, mob oid @7 (varies, known),
/// a counter @11, a sequence-ish byte @12 that is just noise, zeros after.
fn synthetic(i: u32) -> Vec<u8> {
    let mut p = vec![0u8; 40];
    p[0] = 0x00;
    p[1] = 0x70;
    p[2..6].copy_from_slice(&(500_000 + (i % 4)).to_le_bytes());
    p[6] = 5;
    p[7..11].copy_from_slice(&(41_000_000 + i % 7).to_le_bytes());
    p[11] = i as u8;
    p[12] = (i * 37 % 251) as u8;
    p
}

fn teach(ex: &mut Explorer) {
    for k in 0..4 {
        ex.learn(&Event::PlayerSpawn { char_id: 500_000 + k, name: "P".into() });
    }
    for k in 0..7 {
        ex.learn(&Event::MobSpawn { oid: 41_000_000 + k, template: 2, x: 0, y: 0 });
    }
}

#[test]
fn it_finds_constants_id_references_and_what_is_left() {
    let reg = Registry::builtin().unwrap();
    let mut ex = Explorer::default();
    teach(&mut ex);
    for i in 0..60 {
        ex.record(Dir::In, &synthetic(i));
    }
    let x = ex.explore(&reg, Dir::In, 0x7000).unwrap();
    assert_eq!((x.count, x.dominant_len, x.samples_used, x.analysed), (60, 40, 60, 40));
    assert_eq!(x.status, "unregistered");
    assert_eq!(x.offsets[6].constant, Some(5));
    assert_eq!(x.offsets[2].id_ref.as_deref(), Some("player"));
    assert_eq!(x.offsets[2].id_share, Some(1.0));
    assert_eq!(x.offsets[7].id_ref.as_deref(), Some("mob"));
    assert!(x.offsets[11].id_ref.is_none() && x.offsets[11].distinct > 20, "a counter is not an id");
    assert!(x.offsets[20].constant == Some(0), "trailing zeros are constant");
    // varying bytes that nothing explains: the counter @11 and the noise byte @12 (id windows and constants are not "left")
    assert_eq!(x.unexplained_varying, 2);
    // explained = opcode (2) + player id (4) + mob id (4) = 10 of the 40 bytes
    assert_eq!(x.explained, 10);
}

#[test]
fn registry_fields_count_as_explained() {
    let reg = Registry::builtin().unwrap();
    let mut ex = Explorer::default();
    // a real PlayerMove (0x029d: char_id@2, x@10, y@12 are registered)
    let fx: serde_json::Value = serde_json::from_str(include_str!("../../neon-protocol/tests/fixtures/plain_packets.json")).unwrap();
    let base = plain(fx["0x29d"]["plain"].as_str().unwrap());
    for i in 0..30u16 {
        let mut p = base.clone();
        p[10..12].copy_from_slice(&(i as i16 * 10).to_le_bytes());
        p[2..6].copy_from_slice(&(600_000 + u32::from(i % 3)).to_le_bytes());
        ex.record(Dir::In, &p);
    }
    let x = ex.explore(&reg, Dir::In, 0x029d).unwrap();
    assert_eq!(x.name.as_deref(), Some("PlayerMove"));
    assert_eq!(x.offsets[10].field.as_deref(), Some("x"));
    assert_eq!(x.offsets[2].field.as_deref(), Some("char_id"));
    assert_eq!(x.offsets[0].field.as_deref(), Some("opcode"));
    assert!(x.explained >= 10);
}

#[test]
fn todo_ranks_by_unexplained_traffic_and_skips_solved_opcodes() {
    let reg = Registry::builtin().unwrap();
    let mut ex = Explorer::default();
    teach(&mut ex);
    for i in 0..60 {
        ex.record(Dir::In, &synthetic(i));
    }
    // a rarer opcode with one varying byte
    for i in 0..10u8 {
        ex.record(Dir::Out, &[0x01, 0x71, i, 0, 0]);
    }
    // a bare opcode: nothing varies, nothing to map
    for _ in 0..50 {
        ex.record(Dir::In, &[0x28, 0x00]);
    }
    let t = ex.todo(&reg, 10);
    assert_eq!(t[0].op, "0x7000");
    assert_eq!(t[0].score, 60 * 2);
    assert!(t.iter().any(|x| x.dir == "out" && x.op == "0x7101" && x.score == 10));
    assert!(!t.iter().any(|x| x.op == "0x0028"), "a constant packet has nothing left to map");
}

#[test]
fn the_shared_state_feeds_the_explorer_and_clear_forgets() {
    let reg = Registry::builtin().unwrap();
    let mut sh = Shared::default();
    let fx: serde_json::Value = serde_json::from_str(include_str!("../../neon-protocol/tests/fixtures/plain_packets.json")).unwrap();
    sh.feed(&reg, Dir::In, &plain(fx["0x3c9"]["plain"].as_str().unwrap()), 1);
    assert_eq!(sh.explorer.ids.mobs.len(), 1, "a verified spawn teaches the explorer a mob id");
    assert!(sh.explorer.explore(&reg, Dir::In, 0x03c9).is_some());
    sh.clear();
    assert!(sh.explorer.explore(&reg, Dir::In, 0x03c9).is_none());
    assert!(sh.explorer.ids.mobs.is_empty());
}
