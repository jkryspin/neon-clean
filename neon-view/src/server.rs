//! Minimal local view server (std only): `/` page, `/state` JSON snapshot, `/events` server-sent snapshots.
//! The caller picks the bind address (neon-view defaults to 0.0.0.0). Read-only, but there is no authentication.

use crate::Shared;
use neon_protocol::Registry;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const INDEX_HTML: &str = include_str!("../ui/index.html");

pub enum Route {
    Body { status: u16, content_type: &'static str, body: String },
    Events,
}

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
        "/packets" => json(serde_json::to_string(&sh.packets.recent(limit)).unwrap()),
        "/opcodes" => json(serde_json::to_string(&sh.packets.histogram(reg)).unwrap()),
        "/" | "/index.html" => Route::Body { status: 200, content_type: "text/html; charset=utf-8", body: INDEX_HTML.to_string() },
        "/state" => json(sh.world.to_json()),
        "/events" => Route::Events,
        _ => Route::Body { status: 404, content_type: "text/plain", body: "not found".into() },
    }
}

fn handle(mut stream: TcpStream, world: Arc<Mutex<Shared>>, reg: Arc<Registry>, tick: Duration) {
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    if method != "GET" {
        let _ = stream.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }
    let r = { route(path, &world.lock().unwrap(), &reg) };
    match r {
        Route::Body { status, content_type, body } => {
            let reason = if status == 200 { "OK" } else { "Not Found" };
            let head = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
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
                let json = world.lock().unwrap().world.to_json();
                if stream.write_all(format!("data: {json}\n\n").as_bytes()).is_err() || stream.flush().is_err() {
                    return; // client went away
                }
                std::thread::sleep(tick);
            }
        }
    }
}

/// Accept connections forever, one thread each. Returns when the listener fails.
pub fn serve(listener: TcpListener, world: Arc<Mutex<Shared>>, reg: Arc<Registry>, tick: Duration) {
    for conn in listener.incoming() {
        let Ok(stream) = conn else { return };
        let w = world.clone();
        let r = reg.clone();
        std::thread::spawn(move || handle(stream, w, r, tick));
    }
}
