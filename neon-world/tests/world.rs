use neon_protocol::Registry;
use neon_world::decode::{PartyMsg, StatUpdate};
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

fn hex(s: &str) -> Vec<u8> {
    s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
}

fn party() -> serde_json::Value {
    serde_json::from_str(include_str!("../../neon-stream/tests/fixtures/party_packets.json")).unwrap()
}

fn line(t: f64, p: &[u8]) -> String {
    let op = u16::from_le_bytes([p[0], p[1]]);
    format!("{t:.3} op {op:04x} len {}: {}", p.len(), p.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "))
}

#[test]
fn real_packets_become_the_expected_events() {
    let r = Registry::builtin().unwrap();
    let f = fixtures();
    // The spawn's HP is checked against an independent source: the public monster database kept in neon-calib.
    let db: serde_json::Value = serde_json::from_str(include_str!("../../neon-calib/mob-info.json")).unwrap();
    let stump_hp = db["mobs"]["6"]["hp"].as_u64().unwrap() as u32;
    assert_eq!(stump_hp, 133);
    assert_eq!(
        event_from_packet(&r, &fx("0x3c9")),
        Some(Event::MobSpawn {
            oid: f["0x3c9"]["expect"]["oid"].as_u64().unwrap() as u32,
            template: 6,
            x: 698,
            y: -565,
            hp: Some(stump_hp)
        })
    );
    assert_eq!(event_from_packet(&r, &fx("0x3d4")), Some(Event::MobKill { oid: 20598982 }));
    let Some(Event::PlayerSpawn { char_id, name, level, equips }) = event_from_packet(&r, &fx("0x22d")) else { panic!("0x022d must become a PlayerSpawn") };
    assert_eq!((char_id, name.as_str()), (329787, "alex5"));
    assert!((1..=250).contains(&level), "a real spawn carries the player's level: {level}");
    assert!(equips.len() >= 3, "a real spawn lists what the player wears: {equips:?}");
    // the packet starts at x = -185 and its one 21-byte fragment ends at x = -262, on the same ground line
    assert_eq!(event_from_packet(&r, &fx("0x29d")), Some(Event::PlayerMove { char_id: 562142, x: -262, y: 274 }));
    // the real meso drop recorded with the fixtures: mesos, not an item
    let Some(Event::DropSpawn { oid, owner, x, y, item, mesos }) = event_from_packet(&r, &fx("0x471")) else { panic!("0x0471 must become a DropSpawn") };
    assert_eq!((oid, owner, x, y), (1342746, 334426, 1677, 215));
    assert!(item.is_none() && mesos.is_some(), "the 115-byte form carries mesos: item {item:?}, mesos {mesos:?}");
    assert_eq!(event_from_packet(&r, &fx("0x472")), Some(Event::DropRemove { oid: 1342613 }));
    assert_eq!(
        event_from_packet(&r, &fx("0x7e_hp")),
        Some(Event::Stats(StatUpdate { hp: Some(141), ..Default::default() })),
        "real HP update: the value is read, not just the mask"
    );
    assert_eq!(
        event_from_packet(&r, &fx("0x7e_exp")),
        Some(Event::Stats(StatUpdate { exp: Some(59), ..Default::default() }))
    );
}

#[test]
fn unused_and_malformed_packets_are_ignored() {
    let r = Registry::builtin().unwrap();
    assert_eq!(event_from_packet(&r, &fx("0x44d")), None, "MobHit is structure only: no world event");
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
    w.apply(&Event::MobSpawn { oid: 7, template: 9, x: 5, y: 6, hp: Some(142) }, 20);
    assert_eq!(w.snapshot().mobs[&7].template, Some(9));
    w.apply(&Event::DropSpawn { oid: 1, owner: 3, x: 0, y: 0, item: Some(4000001), mesos: None }, 30);
    w.apply(&Event::DropSpawn { oid: 1, owner: 3, x: 0, y: 0, item: Some(4000001), mesos: None }, 31); // sent twice by the server
    assert_eq!(w.snapshot().drops.len(), 1);
    w.apply(&Event::PlayerMove { char_id: 4, x: 9, y: 9 }, 40);
    w.apply(&Event::PlayerSpawn { char_id: 4, name: "Ann".into(), level: 12, equips: vec![(11, 1302000)] }, 41);
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

#[test]
fn real_captured_stat_and_pickup_packets() {
    // From neon-stream's live captures (KryJib01 on Maple Island, 2026-10-25).
    let r = Registry::builtin().unwrap();
    let ev = |h: &str| event_from_packet(&r, &hex(h));
    assert_eq!(ev("7e 00 01 00 01 00 00 04 00 2b 02 00 00 00 00 00 00 00 00"), Some(Event::Stats(StatUpdate { meso: Some(555), ..Default::default() })));
    assert_eq!(ev("7e 00 00 00 01 00 00 01 00 a5 01 00 00 00 00 00 00 00 00"), Some(Event::Stats(StatUpdate { exp: Some(421), ..Default::default() })));
    assert_eq!(ev("8b 00 00 00 00 04 09 3d 00 01 00 00 00 00"), Some(Event::Pickup { item: 4000004, qty: 1 }));
    assert_eq!(ev("8b 00 00 00 01 00 0b 00 00 00 00 00 00 00 00 00"), None, "a meso pickup message is not an item");
    // JOB: read, but 101 is no job.
    let mut j = hex("7e 00 00 00 01 20 80 00 00 64 00 01 00 00 00");
    assert_eq!(event_from_packet(&r, &j), Some(Event::Stats(StatUpdate { job: Some(100), ..Default::default() })));
    j[9] = 0x65;
    assert_ne!(event_from_packet(&r, &j), Some(Event::Stats(StatUpdate { job: Some(101), ..Default::default() })));
    assert!(decode::is_job(100) && decode::is_job(132) && !decode::is_job(101) && !decode::is_job(600));
}

#[test]
fn real_party_packets_build_and_clear_the_party() {
    let r = Registry::builtin().unwrap();
    let f = party();
    let create = event_from_packet(&r, &hex(&f["create586"].as_str().unwrap().chars().collect::<Vec<_>>().chunks(2).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" "))).unwrap();
    let Event::Party(PartyMsg::List { members, .. }) = &create else { panic!("{create:?}") };
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, "shurelia2");
    assert_eq!(members[0].level, 10 /* from the 2026-10-26 capture: Swordsman 10 */);
    let mut w = World::new();
    w.apply(&create, 0);
    assert_eq!(w.snapshot().party.len(), 1);
    let plain = |k: &str| -> Vec<u8> { let s = f[k].as_str().unwrap(); (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect() };
    let join = event_from_packet(&r, &plain("join1267")).unwrap();
    w.apply(&join, 1);
    assert_eq!(w.snapshot().party.len(), 2);
    assert!(w.snapshot().party.iter().any(|m| m.name == "KryJib01"));
    let left = event_from_packet(&r, &plain("leave8")).unwrap();
    assert!(matches!(left, Event::Party(PartyMsg::Left { .. })));
    w.apply(&left, 2);
    assert!(w.snapshot().party.is_empty() && w.snapshot().party_id.is_none(), "one member left over is no party");
}

#[test]
fn me_pickups_and_inventory_accumulate() {
    let mut w = World::new();
    w.apply(&Event::Stats(StatUpdate { hp: Some(100), max_hp: Some(120), ..Default::default() }), 0);
    w.apply(&Event::Stats(StatUpdate { hp: Some(90), exp: Some(5), ..Default::default() }), 1);
    let me = &w.snapshot().me;
    assert_eq!((me.hp, me.max_hp, me.exp), (Some(90), Some(120), Some(5)), "partial updates merge");
    for i in 0..60 {
        w.apply(&Event::Pickup { item: 4000000 + i, qty: 1 }, 2);
    }
    assert_eq!(w.snapshot().pickups.len(), 50);
    assert_eq!(w.snapshot().pickups.last(), Some(&(4000059, 1)));
    w.apply(&Event::Inventory { tab: 2, slot: 3, item: Some(2000000), qty: Some(5) }, 3);
    w.apply(&Event::Inventory { tab: 2, slot: 3, item: None, qty: Some(4) }, 4);
    assert_eq!(w.snapshot().inventory["2:3"], (Some(2000000), Some(4)), "quantity update keeps the known item");
    w.apply(&Event::Inventory { tab: 2, slot: 3, item: None, qty: None }, 5);
    assert!(w.snapshot().inventory.is_empty());
}

#[test]
fn memory_state_merges_without_erasing_packet_knowledge() {
    let mut w = World::new();
    w.apply(&Event::Stats(StatUpdate { exp: Some(1455), meso: Some(900), ..Default::default() }), 0);
    // A real neon-stream line (shurelia2 in Perion, 2026-10-26): map_id and exp are null, position and HP are read.
    let line: serde_json::Value = serde_json::from_str(
        r#"{"seq":1,"player":{"x":-783,"y":1785,"map_id":null,"level":10,"hp":368,"max_hp":544,"mp":194,"max_mp":263,"exp":null,"meso":null,"dead":false,"grounded":true,"job":null}}"#,
    )
    .unwrap();
    w.apply_mem(&line, 10);
    let me = &w.snapshot().me;
    assert_eq!((me.x, me.y, me.hp, me.max_hp, me.level), (Some(-783), Some(1785), Some(368), Some(544), Some(10)));
    assert_eq!((me.grounded, me.dead), (Some(true), Some(false)));
    assert_eq!((me.exp, me.meso), (Some(1455), Some(900)), "null memory fields must not erase packet-derived values");
    assert_eq!(w.snapshot().map, None, "no map id in memory");
    let with_map: serde_json::Value = serde_json::from_str(r#"{"player":{"map_id":1010}}"#).unwrap();
    w.apply_mem(&with_map, 11);
    assert_eq!(w.snapshot().map, Some(1010));
    w.apply_mem(&serde_json::json!({"nothing": 1}), 12); // not a state line: ignored
    assert_eq!(w.snapshot().me.x, Some(-783));
}

#[test]
fn estimated_max_hp_from_memory_never_overrides_the_exact_packet_value() {
    let mut w = World::new();
    w.apply(&Event::Stats(StatUpdate { max_hp: Some(162), ..Default::default() }), 0);
    let line = serde_json::json!({"player": {"hp": 100, "max_hp": 419, "max_mp": 50}});
    w.apply_mem(&line, 1);
    assert_eq!(w.snapshot().me.max_hp, Some(162), "exact packet value kept");
    assert_eq!(w.snapshot().me.max_mp, Some(50), "no packet value for MP yet: the estimate fills in");
    assert_eq!(w.snapshot().me.hp, Some(100));
}

#[test]
fn exp_messages_match_the_exp_counter_in_a_real_session() {
    // 18 lines from shurelia2 grinding Perion Street Corner: exp-only stat updates (0x007e) and EXP messages (0x008b mode 3).
    let r = Registry::builtin().unwrap();
    let lines: Vec<String> = include_str!("fixtures/exp_session.log").lines().map(String::from).collect();
    let mut counter: Vec<u32> = Vec::new();
    let mut amounts: Vec<u32> = Vec::new();
    for l in &lines {
        let (_, p) = parse_log_line(l).unwrap();
        match event_from_packet(&r, &p) {
            Some(Event::Stats(u)) => counter.push(u.exp.unwrap()),
            Some(Event::ExpGain { amount }) => amounts.push(amount),
            other => panic!("unexpected {other:?} for {l}"),
        }
    }
    assert!(counter.len() >= 8 && counter.len().abs_diff(amounts.len()) <= 1, "{} counters, {} messages", counter.len(), amounts.len());
    // every message equals the growth of the counter at the kill it belongs to
    let deltas: Vec<u32> = counter.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(!deltas.is_empty());
    for d in &deltas {
        assert!(amounts.contains(d), "counter grew by {d} but no message said so: {amounts:?}");
    }
    let (w, st) = replay(&r, lines);
    assert_eq!(st.events as usize, counter.len() + amounts.len());
    assert_eq!(w.snapshot().exp_messages as usize, amounts.len());
    assert_eq!(w.snapshot().exp_gained, amounts.iter().map(|a| u64::from(*a)).sum::<u64>());
    assert_eq!(w.snapshot().me.exp, counter.last().copied());
}

#[test]
fn a_respawn_packet_adds_a_mob_with_its_template() {
    let r = Registry::builtin().unwrap();
    let c: serde_json::Value = serde_json::from_str(include_str!("../../neon-protocol/tests/fixtures/combat_packets.json")).unwrap();
    let p = plain(c["0x3d5"]["plain"].as_str().unwrap());
    let Some(Event::MobSpawn { template, .. }) = event_from_packet(&r, &p) else { panic!("0x03d5 must become a MobSpawn event") };
    assert!(template >= 2);
}

#[test]
fn the_activity_feed_records_what_happened_to_us() {
    let r = Registry::builtin().unwrap();
    let lines: Vec<String> = include_str!("fixtures/exp_session.log").lines().map(String::from).collect();
    let (w, _) = replay(&r, lines);
    let f = &w.snapshot().feed;
    assert!(f.iter().filter(|i| i.text.ends_with("EXP")).count() >= 8, "{f:?}");
    assert!(f.len() <= 40);
    let mut w = World::new();
    w.apply(&Event::Stats(StatUpdate { level: Some(10), ..Default::default() }), 0);
    w.apply(&Event::Stats(StatUpdate { level: Some(11), ..Default::default() }), 5);
    w.apply(&Event::MapEnter { map: 1020 }, 6);
    for i in 0..60 {
        w.apply(&Event::Pickup { item: 4000000 + i, qty: 1 }, 10 + u64::from(i));
    }
    let texts: Vec<_> = w.snapshot().feed.iter().map(|i| i.text.clone()).collect();
    assert_eq!(texts.len(), 40, "ring of 40");
    assert!(texts.iter().all(|t| t.starts_with("picked up")), "oldest entries (level up, map) were pushed out: {texts:?}");
}

#[test]
fn movement_ends_are_consistent_with_the_fragment_velocity() {
    // Independent check on the fragment layout: a fragment that ends left of where the packet started moves left, so its
    // velocity (vx) is negative; one that ends to the right has vx > 0.
    let r = Registry::builtin().unwrap();
    let d = r.decode(neon_protocol::Dir::In, &fx("0x29d")).unwrap();
    let (sx, ex) = (d.get("x").unwrap().as_i64().unwrap(), d.get("move[0].x").unwrap().as_i64().unwrap());
    let vx = d.get("move[0].vx").unwrap().as_i64().unwrap();
    assert!(ex < sx && vx < 0, "start {sx}, end {ex}, vx {vx}");
    // a mob: the same property on the real two-fragment MobMove
    let m = r.decode(neon_protocol::Dir::In, &fx("0x3dc")).unwrap();
    let mut prev = m.get("x").unwrap().as_i64().unwrap();
    for i in 0..2 {
        let x = m.get(&format!("move[{i}].x")).unwrap().as_i64().unwrap();
        let vx = m.get(&format!("move[{i}].vx")).unwrap().as_i64().unwrap();
        assert!(x == prev || (x - prev).signum() == vx.signum() || vx == 0, "fragment {i}: {prev} -> {x} with vx {vx}");
        prev = x;
    }
    // and the world takes the end of the last fragment
    let Some(Event::MobMove { x, .. }) = event_from_packet(&r, &fx("0x3dc")) else { panic!() };
    assert_eq!(i64::from(x), prev);
}

#[test]
fn npc_walks_become_npc_positions_and_a_map_change_clears_them() {
    let r = Registry::builtin().unwrap();
    let c: serde_json::Value = serde_json::from_str(include_str!("../../neon-protocol/tests/fixtures/combat_packets.json")).unwrap();
    let p = plain(c["0x456_npc2"]["plain"].as_str().unwrap()); // a real two-fragment NPC move, Amherst
    let d = r.decode(neon_protocol::Dir::In, &p).unwrap();
    let (oid, start_x, start_y) = (d.get("oid").unwrap().as_i64().unwrap(), d.get("x").unwrap().as_i64().unwrap(), d.get("y").unwrap().as_i64().unwrap());
    assert!((100..200).contains(&oid), "NPC object ids are small: {oid}");
    // Independent check: Amherst's own map file puts NPCs at (1790,154), (547,274) and (103,274); this one starts near one of them.
    let npcs = [(1790i64, 154i64), (547, 274), (103, 274)];
    assert!(npcs.iter().any(|(x, y)| (x - start_x).abs() < 120 && *y == start_y), "start ({start_x},{start_y}) is not near any NPC of the map");
    let Some(Event::NpcMove { oid: o, x, y }) = event_from_packet(&r, &p) else { panic!("0x0456 must become an NpcMove") };
    assert_eq!(i64::from(o), oid);
    assert_eq!(i64::from(y), start_y, "NPCs walk along their platform");
    let mut w = World::new();
    w.apply(&Event::NpcMove { oid: o, x, y }, 5);
    assert_eq!(w.snapshot().npcs[&o].x, x);
    w.apply(&Event::MapEnter { map: 1020 }, 6);
    assert!(w.snapshot().npcs.is_empty());
}

#[test]
fn chat_lines_become_events_and_the_world_keeps_the_latest_sixty() {
    let r = Registry::builtin().unwrap();
    let c: serde_json::Value = serde_json::from_str(include_str!("../../neon-protocol/tests/fixtures/combat_packets.json")).unwrap();
    let p = plain(c["0xae_kind3"]["plain"].as_str().unwrap());
    let Some(Event::Chat { kind, text }) = event_from_packet(&r, &p) else { panic!("0x00ae must become a Chat event") };
    assert_eq!(kind, 3);
    assert!(text.ends_with("S> 10 Int Earrings 1.6m"), "{text}");
    let mut w = World::new();
    for i in 0..70u64 {
        w.apply(&Event::Chat { kind: 3, text: format!("line {i}") }, i);
    }
    let chat = &w.snapshot().chat;
    assert_eq!(chat.len(), 60);
    assert_eq!((chat[0].text.as_str(), chat[59].text.as_str()), ("line 10", "line 69"));
}

#[test]
fn equipment_slots_hold_one_item_category_each_in_a_real_spawn() {
    // The independent check behind parse_equips: in the classic equipment scheme each slot takes one category of item
    // (id / 10000). A wrong layout would scatter them. (55 of 55 real spawn packets of a grind and a town agreed.)
    let r = Registry::builtin().unwrap();
    let eq = decode::parse_equips(&fx("0x22d"));
    assert!(!eq.is_empty());
    for (slot, id) in &eq {
        let cat = id / 10_000;
        let ok = match slot {
            1 => cat == 100,
            2 => cat == 101,
            3 => cat == 102,
            4 => cat == 103,
            5 => cat == 104 || cat == 105,
            6 => cat == 106,
            7 => cat == 107,
            8 => cat == 108,
            9 => cat == 110,
            10 => cat == 109,
            11 => (130..=149).contains(&cat),
            12..=16 => cat == 111 || cat == 180,
            _ => true,
        };
        assert!(ok, "slot {slot} holds item {id} (category {cat})");
    }
    // the world keeps them per player
    let mut w = World::new();
    let Some(e) = event_from_packet(&r, &fx("0x22d")) else { panic!() };
    w.apply(&e, 0);
    assert_eq!(w.snapshot().players[&329787].equips, eq);
    // a packet that is not a spawn has no equipment list
    assert!(decode::parse_equips(&fx("0x29d")).is_empty());
}

#[test]
fn a_real_item_drop_names_its_item() {
    let r = Registry::builtin().unwrap();
    let c: serde_json::Value = serde_json::from_str(include_str!("../../neon-protocol/tests/fixtures/combat_packets.json")).unwrap();
    let p = plain(c["0x471_item"]["plain"].as_str().unwrap());
    assert_eq!(p.len(), 123);
    let Some(Event::DropSpawn { item, mesos, owner, .. }) = event_from_packet(&r, &p) else { panic!() };
    assert!(mesos.is_none() && item.is_some_and(|i| (1_000_000..10_000_000).contains(&i)), "{item:?}");
    assert_eq!(owner, 334426, "our own drop");
    let mut w = World::new();
    w.apply(&event_from_packet(&r, &p).unwrap(), 0);
    assert_eq!(w.snapshot().drops.values().next().unwrap().item, item);
}

#[test]
fn a_real_removal_packet_removes_the_player_from_the_world() {
    let r = Registry::builtin().unwrap();
    let c: serde_json::Value = serde_json::from_str(include_str!("../../neon-protocol/tests/fixtures/combat_packets.json")).unwrap();
    let p = plain(c["0x209_leave"]["plain"].as_str().unwrap());
    assert_eq!(p.len(), 13);
    let Some(Event::PlayerLeave { char_id }) = event_from_packet(&r, &p) else { panic!("0x0209 must become a PlayerLeave") };
    let mut w = World::new();
    w.apply(&Event::PlayerSpawn { char_id, name: "Gone".into(), level: 9, equips: vec![] }, 0);
    w.apply(&Event::PlayerMove { char_id, x: 5, y: 6 }, 1);
    w.apply(&Event::PlayerSpawn { char_id: char_id + 1, name: "Stays".into(), level: 9, equips: vec![] }, 2);
    assert_eq!(w.snapshot().players.len(), 2);
    w.apply(&Event::PlayerLeave { char_id }, 3);
    let names: Vec<_> = w.snapshot().players.values().filter_map(|p| p.name.clone()).collect();
    assert_eq!(names, vec!["Stays".to_string()]);
}

#[test]
fn damage_from_other_players_accumulates_on_the_mob_until_it_is_killed() {
    use neon_world::{event_from_packet, Event, World};
    let reg = neon_protocol::Registry::builtin().unwrap();
    let f: serde_json::Value = serde_json::from_str(include_str!("fixtures/damage_kill.json")).unwrap();
    let hex = |s: &str| -> Vec<u8> { (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect() };
    let mut w = World::new();
    let spawn = event_from_packet(&reg, &hex(f["mob_spawn"]["plain"].as_str().unwrap())).expect("spawn");
    let Event::MobSpawn { oid, hp, .. } = spawn.clone() else { panic!("not a spawn") };
    w.apply(&spawn, 0);
    let mut total = 0;
    for (i, a) in f["attacks"].as_array().unwrap().iter().enumerate() {
        let e = event_from_packet(&reg, &hex(a["plain"].as_str().unwrap())).expect("attack event");
        let Event::MobHits { hits, .. } = &e else { panic!("not hits: {e:?}") };
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, oid);
        total += hits[0].1;
        w.apply(&e, 100 * (i as u64 + 1));
        assert_eq!(w.snapshot().mobs.get(&oid).map(|m| m.damage_seen), Some(total));
    }
    assert!(total >= hp.unwrap() && total <= hp.unwrap() * 12 / 10);
    w.apply(&Event::MobKill { oid }, 1000);
    assert_eq!(w.snapshot().mobs.get(&oid).map(|m| m.damage_seen), None, "a killed mob is gone");
}

#[test]
fn whisper_shop_sign_and_reactor_events_decode_from_real_packets() {
    use neon_world::{event_from_packet, Event, World};
    let r = Registry::builtin().unwrap();
    let f: serde_json::Value = serde_json::from_str(include_str!("fixtures/whisper_shop_reactor.json")).unwrap();
    let get = |k: &str| plain(f[k]["plain"].as_str().unwrap());

    let Some(Event::Whisper { name, text, .. }) = event_from_packet(&r, &get("whisper")) else { panic!("not a Whisper") };
    assert_eq!((name.as_str(), text.as_str()), ("Huntra", "cc plz"));

    let Some(Event::ShopSign { text, .. }) = event_from_packet(&r, &get("shopsign")) else { panic!("not a ShopSign") };
    assert_eq!(text, "drops, equip, cheap");

    let spawn = event_from_packet(&r, &get("reactor_spawn")).expect("reactor spawn");
    let Event::Reactor { oid, template, hit_count: None, destroyed: false } = spawn else { panic!("not a reactor spawn: {spawn:?}") };
    assert_eq!(template, 0x01030262);

    let mut w = World::new();
    w.apply(&spawn, 0);
    assert_eq!(w.snapshot().reactors.get(&oid), Some(&(template, 0)));

    let hit = event_from_packet(&r, &get("reactor_hit")).expect("reactor hit");
    let Event::Reactor { hit_count: Some(1), destroyed: false, .. } = hit else { panic!("not a reactor hit: {hit:?}") };
    w.apply(&hit, 1);
    assert_eq!(w.snapshot().reactors.get(&oid), Some(&(template, 1)));

    let gone = event_from_packet(&r, &get("reactor_destroyed")).expect("reactor destroyed");
    let Event::Reactor { destroyed: true, .. } = gone else { panic!("not destroyed: {gone:?}") };
    w.apply(&gone, 2);
    assert!(w.snapshot().reactors.get(&oid).is_none(), "a destroyed reactor is gone");
}
