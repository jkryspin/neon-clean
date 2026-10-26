//! Neon View: server, UI, inspector, replay and opcode explorer on top of `neon-world`.

pub mod explorer;
pub mod inspector;
pub mod server;

use neon_protocol::{Dir, Registry, WireMap};
use neon_world::record::{Record, Replayer, Sink};
use neon_world::{event_from_packet, World};
use std::sync::Mutex;

/// What the view shows: the world model plus the packet inspector.
#[derive(Debug, Default)]
pub struct Shared {
    pub world: World,
    pub packets: inspector::Inspector,
    pub explorer: explorer::Explorer,
    /// Directory of per-map geometry files (`<id>.json`), served at `/map/<id>`; its parent holds the name tables.
    pub maps_dir: Option<std::path::PathBuf>,
    /// Wire-opcode translation of the connection being shown (client->server packets use per-connection numbers).
    pub wire_map: WireMap,
}

impl Shared {
    /// Feed one plaintext packet: inspector always, world when it is an incoming packet that maps to an event.
    pub fn feed(&mut self, reg: &Registry, dir: Dir, plain: &[u8], t_ms: u64) {
        self.packets.record(reg, dir, plain, t_ms);
        self.explorer.record(dir, plain);
        let event = match dir {
            Dir::In => event_from_packet(reg, plain),
            Dir::Out => neon_world::event_from_out_packet(reg, plain),
        };
        if let Some(e) = event {
            self.explorer.learn(&e);
            self.world.apply(&e, t_ms);
        }
    }

    /// Forget everything but the configuration (used when seeking backwards).
    pub fn clear(&mut self) {
        self.world = World::new();
        self.packets = inspector::Inspector::default();
        self.explorer = explorer::Explorer::default();
    }
}

/// Adapter so a [`Replayer`] can drive [`Shared`].
pub struct ViewSink<'a> {
    pub shared: &'a mut Shared,
    pub reg: &'a Registry,
}

impl Sink for ViewSink<'_> {
    fn reset(&mut self) {
        self.shared.clear();
    }
    fn feed(&mut self, rec: &Record) {
        if rec.wire {
            // a wire packet is shown as the internal packet the registry describes; unmapped wire opcodes are not shown yet
            if let Some(p) = self.shared.wire_map.translate(&rec.plain) {
                self.shared.feed(self.reg, Dir::Out, &p, rec.t_ms);
            }
            return;
        }
        self.shared.feed(self.reg, rec.dir, &rec.plain, rec.t_ms);
    }
}

/// The whole application state, shared by the server threads, the live follower and the replay ticker.
pub struct App {
    pub shared: Mutex<Shared>,
    pub replay: Mutex<Option<Replayer>>,
    pub reg: Registry,
    /// Directory the page may load sessions from (`/sessions`, `POST /replay/load`). None: no browser.
    pub sessions_dir: Option<std::path::PathBuf>,
    /// Where marks (human labels dropped into the timeline) are appended, as JSON lines `{"t": unix ms, "label": "..."}`.
    pub marks_path: Option<std::path::PathBuf>,
    /// True when a live follower feeds `shared`: loading a replay would fight it, so it is refused.
    pub live: bool,
}

impl App {
    pub fn new(reg: Registry, shared: Shared) -> Self {
        App { shared: Mutex::new(shared), replay: Mutex::new(None), reg, sessions_dir: None, marks_path: None, live: false }
    }

    /// Advance an active replay by `dt_ms` of wall time (call about every 50 ms).
    pub fn tick_replay(&self, dt_ms: u64) {
        let mut rp = self.replay.lock().unwrap();
        if let Some(r) = rp.as_mut() {
            let mut sh = self.shared.lock().unwrap();
            let mut sink = ViewSink { shared: &mut sh, reg: &self.reg };
            r.tick(&mut sink, dt_ms);
        }
    }

    /// Replay controls. Returns false when no replay is loaded.
    pub fn replay_control(&self, action: &str, arg: Option<f64>) -> bool {
        let mut rp = self.replay.lock().unwrap();
        let Some(r) = rp.as_mut() else { return false };
        match action {
            "play" => {
                if r.at_end() {
                    let mut sh = self.shared.lock().unwrap();
                    r.seek(&mut ViewSink { shared: &mut sh, reg: &self.reg }, 0);
                }
                r.playing = true
            }
            "pause" => r.playing = false,
            "speed" => r.speed = arg.unwrap_or(1.0).clamp(0.0, 1000.0),
            "seek" => {
                let mut sh = self.shared.lock().unwrap();
                r.seek(&mut ViewSink { shared: &mut sh, reg: &self.reg }, arg.unwrap_or(0.0).max(0.0) as u64);
            }
            _ => return false,
        }
        true
    }

    /// Files the session browser offers: plain file names (no paths) of `.log`, `.nsr` and `.jsonl` files, newest first.
    pub fn list_sessions(&self) -> String {
        let Some(dir) = &self.sessions_dir else { return "[]".into() };
        let mut rows: Vec<(std::time::SystemTime, String, u64)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let ok = [".log", ".nsr", ".jsonl"].iter().any(|x| name.ends_with(x));
                let md = std::fs::metadata(e.path()).ok()?; // follows symlinks (the live logs are linked in)
                (ok && md.is_file()).then(|| (md.modified().unwrap_or(std::time::UNIX_EPOCH), name, md.len()))
            })
            .collect();
        rows.sort_by(|a, b| b.0.cmp(&a.0));
        let items: Vec<serde_json::Value> = rows.iter().map(|(_, n, sz)| serde_json::json!({"name": n, "bytes": sz})).collect();
        serde_json::to_string(&items).unwrap()
    }

    /// Load one session file from the sessions directory into the replayer (replaces the current one), showing its end.
    /// Returns an error text for a bad name, a live server, or an unreadable file.
    pub fn load_session(&self, name: &str) -> Result<(), String> {
        if self.live {
            return Err("this server follows a live capture; start neon-view without --follow to browse sessions".into());
        }
        let dir = self.sessions_dir.as_ref().ok_or("no sessions directory configured")?;
        let ok = !name.is_empty()
            && name.len() <= 120
            && name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
            && !name.starts_with('.')
            && !name.contains("..");
        if !ok {
            return Err("bad session name".into());
        }
        let mut text = std::fs::read_to_string(dir.join(name)).map_err(|e| format!("cannot read {name}: {e}"))?;
        let wire_map = std::fs::read_to_string(dir.join(format!("{name}.wire.map"))).ok().and_then(|t| WireMap::from_json(&t).ok()).unwrap_or_default();
        // a stamped incoming log may have the client->server packets of the same capture next to it (`<name>.wire`)
        if let Ok(w) = std::fs::read_to_string(dir.join(format!("{name}.wire"))) {
            text.push('\n');
            text.push_str(&w);
        }
        let session = neon_world::record::Session::from_lines(text.lines().map(String::from));
        if session.records.is_empty() {
            return Err(format!("{name} holds no readable records"));
        }
        let mut rp = Replayer::new(session);
        let mut sh = self.shared.lock().unwrap();
        sh.wire_map = wire_map;
        rp.seek(&mut ViewSink { shared: &mut sh, reg: &self.reg }, u64::MAX);
        *self.replay.lock().unwrap() = Some(rp);
        Ok(())
    }

    /// Record a label at the current wall-clock time. Labels are short plain text; anything else is refused.
    pub fn add_mark(&self, label: &str, now_unix_ms: u64) -> Result<(), String> {
        let path = self.marks_path.as_ref().ok_or("marks are not enabled on this server")?;
        let label = label.replace('+', " ");
        let ok = !label.trim().is_empty() && label.len() <= 80 && label.chars().all(|c| c.is_ascii_alphanumeric() || " -_.,:/()".contains(c));
        if !ok {
            return Err("a mark is 1-80 characters: letters, digits, spaces and - _ . , : / ( )".into());
        }
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
        writeln!(f, "{}", serde_json::json!({"t": now_unix_ms, "label": label.trim()})).map_err(|e| e.to_string())
    }

    pub fn list_marks(&self) -> String {
        let Some(path) = &self.marks_path else { return "[]".into() };
        let rows: Vec<serde_json::Value> = std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        serde_json::to_string(&rows).unwrap()
    }

    pub fn replay_status(&self) -> String {
        let rp = self.replay.lock().unwrap();
        match rp.as_ref() {
            None if self.sessions_dir.is_some() && !self.live => r#"{"mode":"browser"}"#.to_string(),
            None => r#"{"mode":"live"}"#.to_string(),
            Some(r) => format!(
                "{{\"mode\":\"replay\",\"duration_ms\":{},\"pos_ms\":{},\"playing\":{},\"speed\":{},\"records\":{},\"origin_unix_ms\":{}}}",
                r.session.duration_ms(),
                r.pos_ms(),
                r.playing,
                r.speed,
                r.session.records.len(),
                r.session.origin_unix_ms.map_or("null".to_string(), |v| v.to_string())
            ),
        }
    }
}
