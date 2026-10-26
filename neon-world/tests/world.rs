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

#[test]
fn view_server_serves_page_state_and_404_over_a_real_socket() {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    let mut w = Shared::default();
    w.world.apply(&Event::MapEnter { map: 1010 }, 0);
    w.world.apply(&Event::PlayerSpawn { char_id: 5, name: "Zed".into() }, 1);
    let reg = Arc::new(Registry::builtin().unwrap());
    w.feed(&reg, &fx("0x3c9"), 5);
    w.feed(&reg, &fx("0x44d"), 6);
    let world = Arc::new(Mutex::new(w));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let w2 = world.clone();
    let r2 = reg.clone();
    std::thread::spawn(move || view::serve(listener, w2, r2, std::time::Duration::from_millis(20)));
    let get = |path: &str| {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let page = get("/");
    assert!(page.starts_with("HTTP/1.1 200") && page.contains("Neon View") && page.contains("text/html"));
    let st = get("/state");
    let body = st.split("\r\n\r\n").nth(1).unwrap();
    let j: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(j["map"], 1010);
    assert_eq!(j["players"]["5"]["name"], "Zed");
    assert!(get("/nope").starts_with("HTTP/1.1 404"));
    let pk = get("/packets?limit=10");
    let pj: serde_json::Value = serde_json::from_str(pk.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(pj.as_array().unwrap().len(), 2);
    assert_eq!(pj[1]["name"], "MobSpawn", "oldest last: newest first");
    assert_eq!(pj[1]["status"], "verified");
    assert!(pj[1]["fields"].as_array().unwrap().iter().any(|f| f[0] == "template" && f[1] == "6"));
    assert_eq!(pj[0]["status"], "structure", "0x044d is registered as structure only");
    assert_eq!(pj[0]["name"], "MobHit");
    assert!(pj[0]["fields"].as_array().unwrap().iter().any(|f| f[0] == "factor" && f[1] == "100"), "the real fixture has factor 100");
    let oc = get("/opcodes");
    let oj: serde_json::Value = serde_json::from_str(oc.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(oj.as_array().unwrap().len(), 2);
    assert_eq!(oj[0]["count"], 1);
    // The event stream emits snapshots and keeps updating.
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
    write!(s, "GET /events HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut buf = [0u8; 4096];
    let n = s.read(&mut buf).unwrap();
    let first = String::from_utf8_lossy(&buf[..n]).to_string();
    assert!(first.contains("text/event-stream"));
    world.lock().unwrap().world.apply(&Event::MapEnter { map: 2020 }, 5);
    std::thread::sleep(std::time::Duration::from_millis(120));
    let mut all = first;
    let n = s.read(&mut buf).unwrap();
    all.push_str(&String::from_utf8_lossy(&buf[..n]));
    assert!(all.contains("\"map\":2020"), "stream did not pick up the new map: {all}");
}

fn hex(s: &str) -> Vec<u8> {
    s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
}

fn party() -> serde_json::Value {
    serde_json::from_str(include_str!("../../neon-stream/tests/fixtures/party_packets.json")).unwrap()
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
fn map_geometry_is_served_by_id_and_only_by_digits() {
    let dir = std::env::temp_dir().join(format!("neon-maps-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("1010.json"), r#"{"footholds":[{"x1":0,"y1":0,"x2":10,"y2":0}]}"#).unwrap();
    std::fs::write(dir.join("secret.json"), "no").unwrap();
    let sh = Shared { maps_dir: Some(dir.clone()), ..Shared::default() };
    let reg = Registry::builtin().unwrap();
    let body = |p: &str| match view::route(p, &sh, &reg) {
        view::Route::Body { status, body, .. } => (status, body),
        _ => panic!("not a body"),
    };
    assert_eq!(body("/map/1010").0, 200);
    assert!(body("/map/1010").1.contains("footholds"));
    assert_eq!(body("/map/9999").0, 404);
    for bad in ["/map/../1010", "/map/secret", "/map/1010.json", "/map/", "/map/%2e%2e", "/map/12345678901"] {
        assert_eq!(body(bad).0, 404, "{bad}");
    }
    let none = Shared::default();
    assert!(matches!(view::route("/map/1010", &none, &reg), view::Route::Body { status: 404, .. }));
    let _ = std::fs::remove_dir_all(dir);
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
fn name_tables_are_served_only_from_the_whitelist() {
    let base = std::env::temp_dir().join(format!("neon-data-test-{}", std::process::id()));
    let maps = base.join("maps");
    std::fs::create_dir_all(&maps).unwrap();
    std::fs::write(base.join("mob-names.json"), r#"{"2":"Snail"}"#).unwrap();
    std::fs::write(base.join("chains.json"), "secret").unwrap();
    let sh = Shared { maps_dir: Some(maps), ..Shared::default() };
    let reg = Registry::builtin().unwrap();
    let st = |p: &str| match view::route(p, &sh, &reg) { view::Route::Body { status, body, .. } => (status, body), _ => panic!() };
    assert_eq!(st("/data/mob-names"), (200, r#"{"2":"Snail"}"#.to_string()));
    assert_eq!(st("/data/map-names").0, 404, "whitelisted but missing");
    for bad in ["/data/chains", "/data/../chains", "/data/mob-names.json", "/data/"] {
        assert_eq!(st(bad).0, 404, "{bad}");
    }
    let _ = std::fs::remove_dir_all(base);
}
