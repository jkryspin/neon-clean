//! neon-view --log <packet log> [--follow] [--port 8765] [--host 0.0.0.0] [--maps neon-calib/maps]
//!             [--mem <file with the latest neon-stream line>] [--start <ms>] [--speed 1.0]
//!
//! Two modes, one page:
//!   live    `--follow`: tail a capture that is still being written (new records appear as they arrive);
//!   replay  default: load the whole session, show it as of `--start` (default: the end) and offer play / pause / speed / seek.
//! Both read the JSON-lines session format (`.nsr`, both directions) and the older stamped incoming log.
//!
//! Serves http://<host>:<port>/ (default 0.0.0.0, so another machine on the LAN can open it). Read-only except the
//! replay controls, and there is no authentication: use `--host 127.0.0.1` anywhere untrusted.
use neon_protocol::Registry;
use neon_view::{App, Shared, ViewSink};
use neon_world::record::{Record, Replayer, Session};
use std::io::BufRead;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let Some(log) = get("--log") else {
        eprintln!("usage: neon-view --log <packet log> [--follow] [--port 8765] [--host 0.0.0.0] [--maps neon-calib/maps] [--mem <file>] [--start <ms>] [--speed 1.0]");
        std::process::exit(2);
    };
    let follow = args.iter().any(|a| a == "--follow");
    let port: u16 = get("--port").and_then(|s| s.parse().ok()).unwrap_or(8765);
    let host = get("--host").unwrap_or_else(|| "0.0.0.0".to_string());
    let maps = get("--maps").map(std::path::PathBuf::from).or_else(|| {
        let d = std::path::PathBuf::from("neon-calib/maps");
        d.is_dir().then_some(d)
    });

    let app = Arc::new(App::new(Registry::builtin().expect("registry"), Shared { maps_dir: maps, ..Shared::default() }));
    let listener = std::net::TcpListener::bind((host.as_str(), port)).expect("bind (is the port taken or the host wrong?)");
    eprintln!("neon-view on http://{host}:{port}/  (log {log}, {}; no authentication)", if follow { "live" } else { "replay" });

    if let Some(mem) = get("--mem") {
        let a = app.clone();
        std::thread::spawn(move || {
            let mut last = String::new();
            loop {
                std::thread::sleep(Duration::from_millis(200));
                let Ok(text) = std::fs::read_to_string(&mem) else { continue };
                let line = text.lines().last().unwrap_or("").to_string();
                if line.is_empty() || line == last {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    a.shared.lock().unwrap().world.apply_mem(&v, 0);
                }
                last = line;
            }
        });
    }
    {
        let a = app.clone();
        std::thread::spawn(move || neon_view::server::serve(listener, a, Duration::from_millis(250)));
    }

    if follow {
        // Live: feed every record as it is appended. Times are relative to the first record so both formats look alike.
        let mut rd = std::io::BufReader::new(std::fs::File::open(&log).expect("open log"));
        let mut t0: Option<u64> = None;
        let mut line = String::new();
        loop {
            line.clear();
            if rd.read_line(&mut line).expect("read") == 0 {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            let Some(rec) = Record::parse_line(line.trim_end()) else { continue };
            let base = *t0.get_or_insert(rec.t_ms);
            let t = if base > 1_000_000_000_000 { rec.t_ms.saturating_sub(base) } else { rec.t_ms };
            app.shared.lock().unwrap().feed(&app.reg, rec.dir, &rec.plain, t);
        }
    }

    // Replay: load, show as of --start (default the end), then tick the transport.
    let lines = std::io::BufReader::new(std::fs::File::open(&log).expect("open log")).lines().map_while(Result::ok);
    let session = Session::from_lines(lines);
    eprintln!("{} records, {} unreadable lines, {:.1} s", session.records.len(), session.bad_lines, session.duration_ms() as f64 / 1000.0);
    let mut rp = Replayer::new(session);
    rp.speed = get("--speed").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    {
        let start = get("--start").and_then(|s| s.parse().ok()).unwrap_or(u64::MAX);
        let mut sh = app.shared.lock().unwrap();
        rp.seek(&mut ViewSink { shared: &mut sh, reg: &app.reg }, start);
    }
    *app.replay.lock().unwrap() = Some(rp);
    let mut last = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let now = Instant::now();
        app.tick_replay(now.duration_since(last).as_millis() as u64);
        last = now;
    }
}
