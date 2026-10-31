use neon_protocol::{Dir, Registry};
use neon_view::*;
use neon_world::*;

fn plain(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn fixtures() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../neon-protocol/tests/fixtures/plain_packets.json"
    ))
    .unwrap()
}

fn fx(key: &str) -> Vec<u8> {
    plain(fixtures()[key]["plain"].as_str().unwrap())
}

#[test]
fn view_server_serves_page_state_and_404_over_a_real_socket() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    let reg = Registry::builtin().unwrap();
    let mut sh = Shared::default();
    sh.world.apply(&Event::MapEnter { map: 1010 }, 0);
    sh.world.apply(
        &Event::PlayerSpawn {
            char_id: 5,
            name: "Zed".into(),
            level: 1,
            equips: vec![],
        },
        1,
    );
    sh.feed(&reg, Dir::In, &fx("0x3c9"), 5);
    sh.feed(&reg, Dir::In, &fx("0x44d"), 6);
    sh.feed(&reg, Dir::Out, &[0xfe, 0x02, 1, 2, 3], 7);
    let app = Arc::new(App::new(reg, sh));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let a2 = app.clone();
    std::thread::spawn(move || server::serve(listener, a2, std::time::Duration::from_millis(20)));
    let req = |method: &str, path: &str| {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let get = |path: &str| req("GET", path);
    let body_of = |r: &str| r.split("\r\n\r\n").nth(1).unwrap().to_string();
    let page = get("/");
    assert!(
        page.starts_with("HTTP/1.1 200")
            && page.contains("Neon View")
            && page.contains("text/html")
    );
    let j: serde_json::Value = serde_json::from_str(&body_of(&get("/state"))).unwrap();
    assert_eq!(j["map"], 1010);
    assert_eq!(j["players"]["5"]["name"], "Zed");
    assert!(get("/nope").starts_with("HTTP/1.1 404"));
    let pj: serde_json::Value = serde_json::from_str(&body_of(&get("/packets?limit=10"))).unwrap();
    assert_eq!(pj.as_array().unwrap().len(), 3);
    assert_eq!(
        pj[0]["dir"], "out",
        "newest first, and the outgoing record is kept"
    );
    assert_eq!(
        pj[0]["status"], "structure",
        "0x02fe is registered as an outgoing structure-only opcode"
    );
    assert_eq!(pj[2]["name"], "MobSpawn");
    assert_eq!(pj[2]["status"], "verified");
    assert!(pj[2]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f[0] == "template" && f[1] == "6"));
    assert_eq!(pj[1]["name"], "MobHit");
    let oj: serde_json::Value = serde_json::from_str(&body_of(&get("/opcodes"))).unwrap();
    assert_eq!(oj.as_array().unwrap().len(), 3);
    assert!(oj
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["dir"] == "out" && o["op"] == "0x02fe"));
    // live mode: no replay loaded
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body_of(&get("/replay"))).unwrap()["mode"],
        "live"
    );
    assert!(
        req("POST", "/replay/play").starts_with("HTTP/1.1 404"),
        "no replay loaded"
    );
    // the event stream emits snapshots and keeps updating
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    write!(s, "GET /events HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut buf = [0u8; 8192];
    let n = s.read(&mut buf).unwrap();
    let first = String::from_utf8_lossy(&buf[..n]).to_string();
    assert!(first.contains("text/event-stream"));
    app.shared
        .lock()
        .unwrap()
        .world
        .apply(&Event::MapEnter { map: 2020 }, 5);
    std::thread::sleep(std::time::Duration::from_millis(120));
    let mut all = first;
    let n = s.read(&mut buf).unwrap();
    all.push_str(&String::from_utf8_lossy(&buf[..n]));
    assert!(
        all.contains("\"map\":2020"),
        "stream did not pick up the new map: {all}"
    );
    assert!(
        all.contains("event: replay"),
        "the stream also carries the transport state"
    );
}

#[test]
fn the_replay_transport_works_over_http() {
    use neon_world::record::{Replayer, Session};
    use std::io::{Read, Write};
    use std::sync::Arc;
    let session = Session::from_lines(
        include_str!("../../neon-world/tests/fixtures/exp_session.log")
            .lines()
            .map(String::from),
    );
    let dur = session.duration_ms();
    assert!(dur > 2000);
    let app = Arc::new(App::new(Registry::builtin().unwrap(), Shared::default()));
    *app.replay.lock().unwrap() = Some(Replayer::new(session));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let a2 = app.clone();
    std::thread::spawn(move || server::serve(listener, a2, std::time::Duration::from_millis(20)));
    let req = |method: &str, path: &str| {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let status = |r: &str| -> serde_json::Value {
        serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap()).unwrap()
    };
    let st = status(&req("GET", "/replay"));
    assert_eq!(
        (
            st["mode"].as_str(),
            st["playing"].as_bool(),
            st["pos_ms"].as_u64()
        ),
        (Some("replay"), Some(false), Some(0))
    );
    assert_eq!(st["duration_ms"].as_u64(), Some(dur));
    // controls are POST only
    assert!(req("GET", "/replay/play").starts_with("HTTP/1.1 405"));
    assert!(
        req("POST", "/replay/seek").starts_with("HTTP/1.1 404"),
        "missing argument"
    );
    // seek to the end: the whole session is in the world
    let st = status(&req("POST", &format!("/replay/seek?ms={dur}")));
    assert_eq!(st["pos_ms"].as_u64(), Some(dur));
    let total = app.shared.lock().unwrap().world.snapshot().exp_gained;
    assert!(total > 0);
    // seek back to 0: the world is rebuilt empty
    req("POST", "/replay/seek?ms=0");
    assert!(app.shared.lock().unwrap().world.snapshot().exp_gained < total);
    // play at speed 50, tick the ticker by hand: position advances, state reflects it
    req("POST", "/replay/speed?x=50");
    let st = status(&req("POST", "/replay/play"));
    assert_eq!(
        (st["playing"].as_bool(), st["speed"].as_f64()),
        (Some(true), Some(50.0))
    );
    for _ in 0..40 {
        app.tick_replay(50);
    }
    assert_eq!(
        app.shared.lock().unwrap().world.snapshot().exp_gained,
        total,
        "played to the end"
    );
    assert_eq!(
        status(&req("GET", "/replay"))["playing"].as_bool(),
        Some(false),
        "stops at the end"
    );
    let st = status(&req("POST", "/replay/pause"));
    assert_eq!(st["playing"].as_bool(), Some(false));
}

#[test]
fn map_geometry_is_served_by_id_and_only_by_digits() {
    let dir = std::env::temp_dir().join(format!("neon-maps-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("1010.json"),
        r#"{"footholds":[{"x1":0,"y1":0,"x2":10,"y2":0}]}"#,
    )
    .unwrap();
    std::fs::write(dir.join("secret.json"), "no").unwrap();
    let sh = Shared {
        maps_dir: Some(dir.clone()),
        ..Shared::default()
    };
    let reg = Registry::builtin().unwrap();
    let body = |p: &str| match server::route(p, &sh, &reg) {
        server::Route::Body { status, body, .. } => (status, body),
        _ => panic!("not a body"),
    };
    assert_eq!(body("/map/1010").0, 200);
    assert!(body("/map/1010").1.contains("footholds"));
    assert_eq!(body("/map/9999").0, 404);
    for bad in [
        "/map/../1010",
        "/map/secret",
        "/map/1010.json",
        "/map/",
        "/map/%2e%2e",
        "/map/12345678901",
    ] {
        assert_eq!(body(bad).0, 404, "{bad}");
    }
    let none = Shared::default();
    assert!(matches!(
        server::route("/map/1010", &none, &reg),
        server::Route::Body { status: 404, .. }
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn name_tables_are_served_only_from_the_whitelist() {
    let base = std::env::temp_dir().join(format!("neon-data-test-{}", std::process::id()));
    let maps = base.join("maps");
    std::fs::create_dir_all(&maps).unwrap();
    std::fs::write(base.join("mob-names.json"), r#"{"2":"Snail"}"#).unwrap();
    std::fs::write(base.join("chains.json"), "secret").unwrap();
    let sh = Shared {
        maps_dir: Some(maps),
        ..Shared::default()
    };
    let reg = Registry::builtin().unwrap();
    let st = |p: &str| match server::route(p, &sh, &reg) {
        server::Route::Body { status, body, .. } => (status, body),
        _ => panic!(),
    };
    assert_eq!(st("/data/mob-names"), (200, r#"{"2":"Snail"}"#.to_string()));
    assert_eq!(st("/data/map-names").0, 404, "whitelisted but missing");
    for bad in [
        "/data/chains",
        "/data/../chains",
        "/data/mob-names.json",
        "/data/",
    ] {
        assert_eq!(st(bad).0, 404, "{bad}");
    }
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn the_session_browser_lists_loads_and_refuses_bad_names() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    let dir = std::env::temp_dir().join(format!("neon-sessions-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("walk.nsr"),
        include_str!("../../neon-world/tests/fixtures/out_walk.jsonl"),
    )
    .unwrap();
    std::fs::write(
        dir.join("old.log"),
        include_str!("../../neon-world/tests/fixtures/exp_session.log"),
    )
    .unwrap();
    std::fs::write(dir.join("empty.log"), "nothing useful\n").unwrap();
    std::fs::write(dir.join("notes.txt"), "not a session").unwrap();
    let mk = |live: bool| {
        let mut app = App::new(Registry::builtin().unwrap(), Shared::default());
        app.sessions_dir = Some(dir.clone());
        app.live = live;
        let app = Arc::new(app);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let a2 = app.clone();
        std::thread::spawn(move || {
            server::serve(listener, a2, std::time::Duration::from_millis(20))
        });
        (app, addr)
    };
    let req = |addr: std::net::SocketAddr, method: &str, path: &str| {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let body = |r: &str| r.split("\r\n\r\n").nth(1).unwrap().to_string();
    let (app, addr) = mk(false);
    // before anything is loaded the server says it is a browser, not live
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body(&req(addr, "GET", "/replay"))).unwrap()
            ["mode"],
        "browser"
    );
    let list: serde_json::Value =
        serde_json::from_str(&body(&req(addr, "GET", "/sessions"))).unwrap();
    let names: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"walk.nsr") && names.contains(&"old.log") && !names.contains(&"notes.txt"),
        "{names:?}"
    );
    // load an outgoing-only session: the world takes our position from it, and the transport reports a replay
    let ok = req(addr, "POST", "/replay/load?name=walk.nsr");
    assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
    let st: serde_json::Value = serde_json::from_str(&body(&ok)).unwrap();
    assert_eq!(
        (st["mode"].as_str(), st["records"].as_u64()),
        (Some("replay"), Some(17))
    );
    assert_eq!(
        app.shared.lock().unwrap().world.snapshot().me.x,
        Some(598),
        "ended where the walk ended"
    );
    // loading another session replaces the first
    assert!(req(addr, "POST", "/replay/load?name=old.log").starts_with("HTTP/1.1 200"));
    assert!(app.shared.lock().unwrap().world.snapshot().exp_gained > 0);
    assert_eq!(
        app.shared.lock().unwrap().world.snapshot().me.x,
        None,
        "the previous session's state is gone"
    );
    // names that could escape the directory, or hold nothing, are refused
    for bad in [
        "../secret",
        "..%2Fsecret",
        "/etc/passwd",
        ".hidden",
        "a/b.log",
        "missing.log",
        "empty.log",
        "walk.nsr%00",
    ] {
        let r = req(addr, "POST", &format!("/replay/load?name={bad}"));
        assert!(
            r.starts_with("HTTP/1.1 400"),
            "{bad}: {}",
            r.lines().next().unwrap()
        );
    }
    // loading is a POST
    assert!(req(addr, "GET", "/replay/load?name=walk.nsr").starts_with("HTTP/1.1 405"));
    // a live server refuses to load (the follower owns the state)
    let (_live, laddr) = mk(true);
    assert!(req(laddr, "POST", "/replay/load?name=walk.nsr").starts_with("HTTP/1.1 400"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body(&req(laddr, "GET", "/replay"))).unwrap()
            ["mode"],
        "live"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn marks_are_stored_with_a_time_read_back_in_order_and_unsafe_labels_are_refused() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    let path = std::env::temp_dir().join(format!("neon-marks-test-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mk = |marks: Option<std::path::PathBuf>| {
        let mut app = App::new(Registry::builtin().unwrap(), Shared::default());
        app.marks_path = marks;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let a2 = Arc::new(app);
        std::thread::spawn(move || {
            server::serve(listener, a2, std::time::Duration::from_millis(20))
        });
        addr
    };
    let req = |addr: std::net::SocketAddr, method: &str, path: &str| {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let body = |r: &str| r.split("\r\n\r\n").nth(1).unwrap().to_string();
    let addr = mk(Some(path.clone()));
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(req(addr, "POST", "/mark?label=opened%20the%20shop").starts_with("HTTP/1.1 200"));
    assert!(req(
        addr,
        "POST",
        "/mark?label=bought%20a%20potion%20(2)%3A%20ok"
    )
    .starts_with("HTTP/1.1 200"));
    let marks: serde_json::Value =
        serde_json::from_str(&body(&req(addr, "GET", "/marks"))).unwrap();
    let rows = marks.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["label"], "opened the shop");
    assert_eq!(rows[1]["label"], "bought a potion (2): ok");
    assert!(
        rows[0]["t"].as_u64().unwrap() >= before && rows[1]["t"].as_u64() >= rows[0]["t"].as_u64(),
        "times are wall-clock and ordered"
    );
    // the file is what an offline tool reads: one JSON object per line
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
    for bad in [
        "%3Cscript%3E",
        "",
        "x%00y",
        "a%0Ab",
        "%2E%2E%2F%2E%2E",
        &"y".repeat(81),
    ] {
        let r = req(addr, "POST", &format!("/mark?label={bad}"));
        assert!(
            r.starts_with("HTTP/1.1 400"),
            "{bad:?}: {}",
            r.lines().next().unwrap()
        );
    }
    assert!(
        req(addr, "GET", "/mark?label=x").starts_with("HTTP/1.1 405"),
        "marking is a POST"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap().lines().count(),
        2,
        "refused labels were not written"
    );
    // a server without a marks file says so and lists nothing
    let off = mk(None);
    assert!(req(off, "POST", "/mark?label=x").starts_with("HTTP/1.1 400"));
    assert_eq!(body(&req(off, "GET", "/marks")), "[]");
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_log_and_its_wire_file_replay_together_and_the_wire_file_is_not_listed_as_a_session() {
    use std::sync::Arc;
    let dir = std::env::temp_dir().join(format!("neon-wire-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("cap.log"),
        "1793050000.000 op 0028 len 2: 28 00\n1793050002.000 op 0028 len 2: 28 00\n",
    )
    .unwrap();
    // client->server: a keepalive (wire opcode 0x8786, mapped to KeepAlive) and an unmapped wire opcode (shown raw, direction "wire")
    std::fs::write(
        dir.join("cap.log.wire"),
        "1793050001.000 op 8786 len 2: 86 87\n1793050001.500 op 1234 len 3: 34 12 00\n",
    )
    .unwrap();
    std::fs::write(dir.join("cap.log.wire.map"), r#"{"8786": "02f0"}"#).unwrap();
    let mut app = App::new(Registry::builtin().unwrap(), Shared::default());
    app.sessions_dir = Some(dir.clone());
    let app = Arc::new(app);
    let listed = app.list_sessions();
    assert!(
        listed.contains("cap.log") && !listed.contains(".wire"),
        "{listed}"
    ); // neither the wire file nor its map is a session
    app.load_session("cap.log").unwrap();
    let sh = app.shared.lock().unwrap();
    let ops = sh.packets.histogram(&app.reg);
    let text = format!("{ops:?}");
    assert!(
        text.contains("KeepAlive"),
        "the wire keepalive is shown as the registry's KeepAlive: {text}"
    );
    assert!(
        text.contains("0x1234") && text.contains("wire"),
        "an unmapped wire opcode is shown as a raw wire packet: {text}"
    );
    assert!(
        !text.contains("0x8786"),
        "a mapped wire opcode is shown under its internal number only: {text}"
    );
}
