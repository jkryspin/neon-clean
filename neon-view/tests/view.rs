use neon_protocol::Registry;
use neon_view::*;
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
    std::thread::spawn(move || server::serve(listener, w2, r2, std::time::Duration::from_millis(20)));
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
fn map_geometry_is_served_by_id_and_only_by_digits() {
    let dir = std::env::temp_dir().join(format!("neon-maps-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("1010.json"), r#"{"footholds":[{"x1":0,"y1":0,"x2":10,"y2":0}]}"#).unwrap();
    std::fs::write(dir.join("secret.json"), "no").unwrap();
    let sh = Shared { maps_dir: Some(dir.clone()), ..Shared::default() };
    let reg = Registry::builtin().unwrap();
    let body = |p: &str| match server::route(p, &sh, &reg) {
        server::Route::Body { status, body, .. } => (status, body),
        _ => panic!("not a body"),
    };
    assert_eq!(body("/map/1010").0, 200);
    assert!(body("/map/1010").1.contains("footholds"));
    assert_eq!(body("/map/9999").0, 404);
    for bad in ["/map/../1010", "/map/secret", "/map/1010.json", "/map/", "/map/%2e%2e", "/map/12345678901"] {
        assert_eq!(body(bad).0, 404, "{bad}");
    }
    let none = Shared::default();
    assert!(matches!(server::route("/map/1010", &none, &reg), server::Route::Body { status: 404, .. }));
    let _ = std::fs::remove_dir_all(dir);
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
    let st = |p: &str| match server::route(p, &sh, &reg) { server::Route::Body { status, body, .. } => (status, body), _ => panic!() };
    assert_eq!(st("/data/mob-names"), (200, r#"{"2":"Snail"}"#.to_string()));
    assert_eq!(st("/data/map-names").0, 404, "whitelisted but missing");
    for bad in ["/data/chains", "/data/../chains", "/data/mob-names.json", "/data/"] {
        assert_eq!(st(bad).0, 404, "{bad}");
    }
    let _ = std::fs::remove_dir_all(base);
}
