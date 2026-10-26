//! neon-view --log <stamped packet log> [--follow] [--speed 1.0] [--port 8765] [--host 0.0.0.0]
//!
//! Replays (or tails) a packet log into the world model and serves it on http://<host>:<port>/ (default host 0.0.0.0,
//! so another machine on the LAN can open it). Read-only, but there is no authentication: use `--host 127.0.0.1` anywhere untrusted.
//! The log is what `NEON_DUMP_OPS=all neon-stream --net` prints with a timestamp in front of each line.
use neon_protocol::Registry;
use neon_view::{server, Shared};
use neon_world::parse_log_line;
use std::io::BufRead;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let Some(log) = get("--log") else {
        eprintln!("usage: neon-view --log <packet log> [--follow] [--speed 1.0 (0 = as fast as possible)] [--port 8765] [--host 0.0.0.0] [--maps neon-calib/maps] [--mem <file with the latest neon-stream line>]");
        std::process::exit(2);
    };
    let follow = args.iter().any(|a| a == "--follow");
    let speed: f64 = get("--speed").and_then(|s| s.parse().ok()).unwrap_or(if follow { 1.0 } else { 0.0 });
    let port: u16 = get("--port").and_then(|s| s.parse().ok()).unwrap_or(8765);
    let host = get("--host").unwrap_or_else(|| "0.0.0.0".to_string());

    let reg = Arc::new(Registry::builtin().expect("registry"));
    let maps = get("--maps").map(std::path::PathBuf::from).or_else(|| {
        let d = std::path::PathBuf::from("neon-calib/maps");
        d.is_dir().then_some(d)
    });
    let world = Arc::new(Mutex::new(Shared { maps_dir: maps, ..Shared::default() }));
    let listener = std::net::TcpListener::bind((host.as_str(), port)).expect("bind (is the port taken or the host wrong?)");
    eprintln!("neon-view on http://{host}:{port}/  (log {log}, follow={follow}, speed={speed}; no authentication)");

    if let Some(mem) = get("--mem") {
        let w = world.clone();
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
                    w.lock().unwrap().world.apply_mem(&v, 0);
                }
                last = line;
            }
        });
    }
    let (w2, r2) = (world.clone(), reg.clone());
    std::thread::spawn(move || server::serve(listener, w2, r2, Duration::from_millis(250)));

    let mut rd = std::io::BufReader::new(std::fs::File::open(&log).expect("open log"));
    let (mut t0, mut prev): (Option<f64>, Option<f64>) = (None, None);
    let mut line = String::new();
    loop {
        line.clear();
        if rd.read_line(&mut line).expect("read") == 0 {
            if follow {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            eprintln!("end of log; serving the final state (Ctrl-C to stop)");
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
        let Some((t, plain)) = parse_log_line(line.trim_end()) else { continue };
        if speed > 0.0 {
            if let Some(p) = prev {
                std::thread::sleep(Duration::from_secs_f64(((t - p).max(0.0) / speed).min(2.0)));
            }
        }
        prev = Some(t);
        let t0 = *t0.get_or_insert(t);
        world.lock().unwrap().feed(&reg, &plain, ((t - t0) * 1000.0) as u64);
    }
}
