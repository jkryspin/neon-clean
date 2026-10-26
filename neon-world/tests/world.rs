use neon_protocol::Registry;
use neon_world::*;

fn plain(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn fixtures() -> serde_json::Value {
    serde_json::from_str(include_str!("../../neon-protocol/tests/fixtures/plain_packets.json")).unwrap()
}

fn fx(key: &str) -> Vec<u8> {
    plain(fixtures()[key]["plain"].as_str().unwrap())
}

fn line(t: f64, p: &[u8]) -> String {
    let op = u16::from_le_bytes([p[0], p[1]]);
    format!("{t:.3} op {op:04x} len {}: {}", p.len(), p.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "))
}

#[test]
fn real_packets_become_the_expected_events() {
    let r = Registry::builtin().unwrap();
    let f = fixtures();
    assert_eq!(
        event_from_packet(&r, &fx("0x3c9")),
        Some(Event::MobSpawn {
            oid: f["0x3c9"]["expect"]["oid"].as_u64().unwrap() as u32,
            template: 6,
            x: 698,
            y: -565
        })
    );
    assert_eq!(event_from_packet(&r, &fx("0x3d4")), Some(Event::MobKill { oid: 20598982 }));
    assert_eq!(event_from_packet(&r, &fx("0x22d")), Some(Event::PlayerSpawn { char_id: 329787, name: "alex5".into() }));
    assert_eq!(event_from_packet(&r, &fx("0x29d")), Some(Event::PlayerMove { char_id: 562142, x: -185, y: 274 }));
    assert_eq!(event_from_packet(&r, &fx("0x471")), Some(Event::DropSpawn { oid: 1342746, owner: 334426, x: 1677, y: 215 }));
    assert_eq!(event_from_packet(&r, &fx("0x472")), Some(Event::DropRemove { oid: 1342613 }));
    assert_eq!(event_from_packet(&r, &fx("0x7e_hp")), Some(Event::StatMask { mask: 1024 }));
}

#[test]
fn unused_and_malformed_packets_are_ignored() {
    let r = Registry::builtin().unwrap();
    assert_eq!(event_from_packet(&r, &fx("0x44d")), None, "structure-only opcode");
    assert_eq!(event_from_packet(&r, &[0x01]), None);
    assert_eq!(event_from_packet(&r, &fx("0x3c9")[..100]), None, "truncated mob spawn");
}

#[test]
fn map_enter_is_read_from_the_set_field_variant_only() {
    let r = Registry::builtin().unwrap();
    let mut p = vec![0u8; 60];
    p[0] = 0xa9;
    p[1] = 0x01;
    p[36..40].copy_from_slice(&1010u32.to_le_bytes());
    assert_eq!(event_from_packet(&r, &p), Some(Event::MapEnter { map: 1010 }));
    let mut long = p.clone();
    long.resize(1500, 0);
    assert_eq!(event_from_packet(&r, &long), None, "the long 0x01a9 is the login inventory");
    p[36..40].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(event_from_packet(&r, &p), None, "map id 0 is not a map");
}

#[test]
fn world_semantics() {
    let mut w = World::new();
    w.apply(&Event::MobMove { oid: 7, x: 1, y: 2 }, 10);
    assert_eq!(w.snapshot().mobs[&7].template, None, "move before spawn: template unknown");
    w.apply(&Event::MobSpawn { oid: 7, template: 9, x: 5, y: 6 }, 20);
    assert_eq!(w.snapshot().mobs[&7].template, Some(9));
    w.apply(&Event::DropSpawn { oid: 1, owner: 3, x: 0, y: 0 }, 30);
    w.apply(&Event::DropSpawn { oid: 1, owner: 3, x: 0, y: 0 }, 31); // sent twice by the server
    assert_eq!(w.snapshot().drops.len(), 1);
    w.apply(&Event::PlayerMove { char_id: 4, x: 9, y: 9 }, 40);
    w.apply(&Event::PlayerSpawn { char_id: 4, name: "Ann".into() }, 41);
    let p = &w.snapshot().players[&4];
    assert_eq!((p.name.as_deref(), p.x, p.y), (Some("Ann"), Some(9), Some(9)));
    w.apply(&Event::MobKill { oid: 7 }, 50);
    assert!(w.snapshot().mobs.is_empty());
    w.apply(&Event::MapEnter { map: 1020 }, 60);
    let s = w.snapshot();
    assert_eq!(s.map, Some(1020));
    assert!(s.players.is_empty() && s.drops.is_empty());
    assert_eq!(s.events, 8);
    let j: serde_json::Value = serde_json::from_str(&w.to_json()).unwrap();
    assert_eq!(j["map"], 1020);
}

#[test]
fn replaying_a_stamped_log_reproduces_the_world() {
    let r = Registry::builtin().unwrap();
    let lines = vec![
        line(100.0, &fx("0x22d")),
        line(100.5, &fx("0x3c9")),
        line(101.0, &fx("0x29d")),
        line(101.2, &fx("0x44d")), // unused
        "garbage".to_string(),
        line(102.0, &fx("0x3d4")), // kills a different mob than the spawned one
    ];
    let (w, st) = replay(&r, lines);
    assert_eq!(st, ReplayStats { packets: 5, events: 4, unused: 1, bad_lines: 1 });
    let s = w.snapshot();
    assert_eq!(s.players.len(), 2); // spawn of char 329787, move of char 562142 (no spawn seen)
    assert_eq!(s.mobs.len(), 1);
    assert_eq!(s.now_ms, 2000);
    assert_eq!(s.players[&329787].name.as_deref(), Some("alex5"));
}

#[test]
fn log_line_parser() {
    assert_eq!(parse_log_line("1.5 op 0028 len 2: 28 00"), Some((1.5, vec![0x28, 0x00])));
    assert_eq!(parse_log_line("1.5 op 0028 len 2:"), Some((1.5, vec![])));
    assert_eq!(parse_log_line("nope"), None);
    assert_eq!(parse_log_line("1.5 op 0028 len 2: zz"), None);
}
