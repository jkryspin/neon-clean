//! neon-view [--log <packet log> [--log <more>]] [--sessions <dir>] [--marks <file>] [--follow] [--port 8765] [--host 0.0.0.0] [--maps data/maps]
//!             [--mem <file with the latest neon-stream line>] [--start <ms>] [--speed 1.0]
//!
//! Two modes, one page:
//!   live    `--follow`: tail a capture that is still being written (new records appear as they arrive);
//!   replay  default: load the whole session, show it as of `--start` (default: the end) and offer play / pause / speed / seek.
//! Both read the JSON-lines session format (`.nsr`, both directions) and the older stamped incoming log; `--log X --log X.wire`
//! adds the client->server packets that `neon-stream` decrypts passively (shown through the registry's `wire_opcode` map).
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
    let get = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    // --log may be given several times (an incoming capture and an outgoing one of the same period merge by time)
    let logs: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--log")
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect();
    let sessions = get("--sessions").map(std::path::PathBuf::from);
    if logs.is_empty() && sessions.is_none() {
        eprintln!("usage: neon-view [--log <packet log>] [--sessions <dir>] [--follow] [--port 8765] [--host 0.0.0.0] [--maps data/maps] [--mem <file>] [--start <ms>] [--speed 1.0]");
        std::process::exit(2);
    }
    let log = logs.first().cloned().unwrap_or_default();
    let follow = args.iter().any(|a| a == "--follow");
    let port: u16 = get("--port").and_then(|s| s.parse().ok()).unwrap_or(8765);
    let host = get("--host").unwrap_or_else(|| "0.0.0.0".to_string());
    let maps = get("--maps").map(std::path::PathBuf::from).or_else(|| {
        let d = std::path::PathBuf::from("data/maps");
        d.is_dir().then_some(d)
    });

    let mut app = App::new(
        Registry::builtin().expect("registry"),
        Shared {
            maps_dir: maps,
            ..Shared::default()
        },
    );
    app.sessions_dir = sessions;
    app.live = follow;
    app.marks_path = get("--marks").map(std::path::PathBuf::from);
    let app = Arc::new(app);
    let listener = std::net::TcpListener::bind((host.as_str(), port))
        .expect("bind (is the port taken or the host wrong?)");
    eprintln!(
        "neon-view on http://{host}:{port}/  (log {log}, {}; no authentication)",
        if follow { "live" } else { "replay" }
    );

    if let Some(mem) = get("--mem") {
        let a = app.clone();
        std::thread::spawn(move || {
            let mut last = String::new();
            loop {
                std::thread::sleep(Duration::from_millis(200));
                let Ok(text) = std::fs::read_to_string(&mem) else {
                    continue;
                };
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
        std::thread::spawn(move || {
            neon_view::server::serve(listener, a, Duration::from_millis(250))
        });
    }

    if follow {
        // Live: tail every --log (the incoming log and the `.wire` log of client->server packets) and feed each record as it is
        // appended. Times are relative to the very first record of any file, so both formats look alike.
        let base = Arc::new(std::sync::Mutex::new(None::<u64>));
        let mut tails = vec![];
        // the connection's wire-opcode map (`<first log>.wire.map`, written by neon-calib/tools/wiremap.py): re-read every few seconds
        if let Some(first) = logs.first().cloned() {
            let app = app.clone();
            std::thread::spawn(move || {
                let path = format!("{first}.wire.map");
                let mut last = String::new();
                loop {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        if text != last {
                            if let Ok(m) = neon_protocol::WireMap::from_json(&text) {
                                app.shared.lock().unwrap().wire_map = m;
                            }
                            last = text;
                        }
                    }
                    std::thread::sleep(Duration::from_secs(3));
                }
            });
        }
        for path in logs.clone() {
            let (app, base) = (app.clone(), base.clone());
            tails.push(std::thread::spawn(move || {
                let mut rd = std::io::BufReader::new(std::fs::File::open(&path).expect("open log"));
                let mut line = String::new();
                let mut last_t = 0u64;
                loop {
                    line.clear();
                    if rd.read_line(&mut line).expect("read") == 0 {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    let trimmed = line.trim_end();
                    let Some(mut rec) = Record::parse_line(trimmed) else {
                        if neon_world::is_heuristic_line(trimmed) {
                            app.shared.lock().unwrap().world.note_heuristic_skip(last_t);
                        }
                        continue;
                    };
                    if path.ends_with(".wire") {
                        // everything in a .wire file is a client->server packet with a wire opcode
                        rec.wire = true;
                        rec.dir = neon_protocol::Dir::Out;
                    }
                    let b = *base.lock().unwrap().get_or_insert(rec.t_ms);
                    let t = if b > 1_000_000_000_000 {
                        rec.t_ms.saturating_sub(b)
                    } else {
                        rec.t_ms
                    };
                    last_t = t;
                    let mut sh = app.shared.lock().unwrap();
                    if rec.wire {
                        match sh.wire_map.translate(&rec.plain) {
                            Some(p) => sh.feed(&app.reg, rec.dir, &p, t),
                            None => sh.feed(&app.reg, neon_protocol::Dir::Wire, &rec.plain, t),
                        }
                    } else {
                        sh.feed(&app.reg, rec.dir, &rec.plain, t);
                    }
                }
            }));
        }
        for t in tails {
            let _ = t.join();
        }
        return;
    }

    // Replay: load, show as of --start (default the end), then tick the transport. With only --sessions, start empty and let the page pick.
    if logs.is_empty() {
        eprintln!(
            "session browser on {:?}: pick a session in the page",
            app.sessions_dir
        );
        loop {
            std::thread::sleep(Duration::from_millis(50));
            app.tick_replay(50);
        }
    }
    let mut all: Vec<String> = Vec::new();
    for l in &logs {
        all.extend(
            std::io::BufReader::new(std::fs::File::open(l).expect("open log"))
                .lines()
                .map_while(Result::ok),
        );
    }
    let session = Session::from_lines(all);
    eprintln!(
        "{} records, {} unreadable lines, {:.1} s",
        session.records.len(),
        session.bad_lines,
        session.duration_ms() as f64 / 1000.0
    );
    let mut rp = Replayer::new(session);
    rp.speed = get("--speed").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    {
        let start = get("--start")
            .and_then(|s| s.parse().ok())
            .unwrap_or(u64::MAX);
        let mut sh = app.shared.lock().unwrap();
        rp.seek(
            &mut ViewSink {
                shared: &mut sh,
                reg: &app.reg,
            },
            start,
        );
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
