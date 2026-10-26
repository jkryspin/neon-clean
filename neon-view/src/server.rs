//! Minimal local view server (std only): `/` page, `/state` JSON snapshot, `/events` server-sent snapshots.
//! The caller picks the bind address (neon-view defaults to 0.0.0.0). Read-only, but there is no authentication.

use crate::{App, Shared};
use neon_protocol::Registry;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

pub const INDEX_HTML: &str = include_str!("../ui/index.html");

pub enum Route {
    Body { status: u16, content_type: &'static str, body: String },
    Events,
}

/// Read-only routes. `sh` is locked by the caller; replay controls are handled in [`control`].
pub fn route(path: &str, sh: &Shared, reg: &Registry) -> Route {
    let (p, query) = path.split_once('?').unwrap_or((path, ""));
    let limit = query.split('&').find_map(|kv| kv.strip_prefix("limit=")).and_then(|n| n.parse().ok()).unwrap_or(100usize).min(400);
    let json = |v: String| Route::Body { status: 200, content_type: "application/json", body: v };
    match p {
        m if m.starts_with("/data/") => {
            // Only these name tables, read from the maps directory's parent (neon-calib/).
            const OK: [&str; 3] = ["mob-names", "map-names", "npc-names"];
            let name = &m[6..];
            let dir = sh.maps_dir.as_ref().and_then(|d| d.parent());
            match (OK.contains(&name), dir) {
                (true, Some(d)) => match std::fs::read_to_string(d.join(format!("{name}.json"))) {
                    Ok(body) => json(body),
                    Err(_) => Route::Body { status: 404, content_type: "text/plain", body: "missing".into() },
                },
                _ => Route::Body { status: 404, content_type: "text/plain", body: "not found".into() },
            }
        }
        m if m.starts_with("/map/") => {
            let id = &m[5..];
            // Digits only: the id becomes part of a file name, so nothing else may get through.
            let ok = !id.is_empty() && id.len() <= 10 && id.bytes().all(|c| c.is_ascii_digit());
            match (ok, &sh.maps_dir) {
                (true, Some(dir)) => match std::fs::read_to_string(dir.join(format!("{id}.json"))) {
                    Ok(body) => json(body),
                    Err(_) => Route::Body { status: 404, content_type: "text/plain", body: "no geometry for that map".into() },
                },
                _ => Route::Body { status: 404, content_type: "text/plain", body: "not found".into() },
            }
        }
        "/explore" => {
            let q = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(&format!("{k}=")));
            let dir = match q("dir") {
                Some("out") => neon_protocol::Dir::Out,
                _ => neon_protocol::Dir::In,
            };
            let id = q("op").and_then(|o| u16::from_str_radix(o.trim_start_matches("0x"), 16).ok());
            match id.and_then(|id| sh.explorer.explore(reg, dir, id)) {
                Some(x) => json(serde_json::to_string(&x).unwrap()),
                None => Route::Body { status: 404, content_type: "text/plain", body: "no such opcode seen".into() },
            }
        }
        "/todo" => json(serde_json::to_string(&sh.explorer.todo(reg, 40)).unwrap()),
        "/packets" => json(serde_json::to_string(&sh.packets.recent(limit)).unwrap()),
        "/opcodes" => json(serde_json::to_string(&sh.packets.histogram(reg)).unwrap()),
        "/" | "/index.html" => Route::Body { status: 200, content_type: "text/html; charset=utf-8", body: INDEX_HTML.to_string() },
        "/state" => json(sh.world.to_json()),
        "/events" => Route::Events,
        _ => Route::Body { status: 404, content_type: "text/plain", body: "not found".into() },
    }
}

/// Mutating routes (POST only): `/replay/play`, `/replay/pause`, `/replay/speed?x=2`, `/replay/seek?ms=1234`.
pub fn control(app: &App, path: &str) -> Route {
    let (p, query) = path.split_once('?').unwrap_or((path, ""));
    let arg = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(&format!("{k}="))).and_then(|v| v.parse::<f64>().ok());
    let ok = match p {
        "/replay/play" => app.replay_control("play", None),
        "/replay/pause" => app.replay_control("pause", None),
        "/replay/speed" => arg("x").map_or(false, |x| app.replay_control("speed", Some(x))),
        "/replay/seek" => arg("ms").map_or(false, |m| app.replay_control("seek", Some(m))),
        _ => false,
    };
    if ok {
        Route::Body { status: 200, content_type: "application/json", body: app.replay_status() }
    } else {
        Route::Body { status: 404, content_type: "text/plain", body: "no such control, bad argument, or no replay loaded".into() }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        405 => "Method Not Allowed",
        _ => "Not Found",
    }
}

fn handle(mut stream: TcpStream, app: Arc<App>, tick: Duration) {
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let r = match (method, path) {
        ("GET", p) if p == "/replay" || p.starts_with("/replay?") => Route::Body { status: 200, content_type: "application/json", body: app.replay_status() },
        ("GET", "/sessions") => Route::Body { status: 200, content_type: "application/json", body: app.list_sessions() },
        ("POST", p) if p.starts_with("/replay/load?name=") => match app.load_session(&p["/replay/load?name=".len()..]) {
            Ok(()) => Route::Body { status: 200, content_type: "application/json", body: app.replay_status() },
            Err(e) => Route::Body { status: 400, content_type: "text/plain", body: e },
        },
        ("POST", p) if p.starts_with("/replay/") => control(&app, p),
        ("GET", p) if p.starts_with("/replay/") => Route::Body { status: 405, content_type: "text/plain", body: "use POST".into() },
        ("GET", p) => route(p, &app.shared.lock().unwrap(), &app.reg),
        _ => Route::Body { status: 405, content_type: "text/plain", body: "method not allowed".into() },
    };
    match r {
        Route::Body { status, content_type, body } => {
            let head = format!(
                "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                reason(status),
                body.len()
            );
            let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body.as_bytes()));
        }
        Route::Events => {
            if stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n")
                .is_err()
            {
                return;
            }
            loop {
                let json = app.shared.lock().unwrap().world.to_json();
                let status = app.replay_status();
                if stream.write_all(format!("data: {json}\n\n").as_bytes()).is_err() || stream.write_all(format!("event: replay\ndata: {status}\n\n").as_bytes()).is_err() || stream.flush().is_err() {
                    return; // client went away
                }
                std::thread::sleep(tick);
            }
        }
    }
}

/// Accept connections forever, one thread each. Returns when the listener fails.
pub fn serve(listener: TcpListener, app: Arc<App>, tick: Duration) {
    for conn in listener.incoming() {
        let Ok(stream) = conn else { return };
        let a = app.clone();
        std::thread::spawn(move || handle(stream, a, tick));
    }
}
