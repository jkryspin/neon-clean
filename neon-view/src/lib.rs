//! Neon View: server, UI, inspector, replay and opcode explorer on top of `neon-world`.

pub mod inspector;
pub mod server;

use neon_protocol::{Dir, Registry};
use neon_world::record::{Record, Replayer, Sink};
use neon_world::{event_from_packet, World};
use std::sync::Mutex;

/// What the view shows: the world model plus the packet inspector.
#[derive(Debug, Default)]
pub struct Shared {
    pub world: World,
    pub packets: inspector::Inspector,
    /// Directory of per-map geometry files (`<id>.json`), served at `/map/<id>`; its parent holds the name tables.
    pub maps_dir: Option<std::path::PathBuf>,
}

impl Shared {
    /// Feed one plaintext packet: inspector always, world when it is an incoming packet that maps to an event.
    pub fn feed(&mut self, reg: &Registry, dir: Dir, plain: &[u8], t_ms: u64) {
        self.packets.record(reg, dir, plain, t_ms);
        if dir == Dir::In {
            if let Some(e) = event_from_packet(reg, plain) {
                self.world.apply(&e, t_ms);
            }
        }
    }

    /// Forget everything but the configuration (used when seeking backwards).
    pub fn clear(&mut self) {
        self.world = World::new();
        self.packets = inspector::Inspector::default();
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
        self.shared.feed(self.reg, rec.dir, &rec.plain, rec.t_ms);
    }
}

/// The whole application state, shared by the server threads, the live follower and the replay ticker.
pub struct App {
    pub shared: Mutex<Shared>,
    pub replay: Mutex<Option<Replayer>>,
    pub reg: Registry,
}

impl App {
    pub fn new(reg: Registry, shared: Shared) -> Self {
        App { shared: Mutex::new(shared), replay: Mutex::new(None), reg }
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

    pub fn replay_status(&self) -> String {
        let rp = self.replay.lock().unwrap();
        match rp.as_ref() {
            None => r#"{"mode":"live"}"#.to_string(),
            Some(r) => format!(
                "{{\"mode\":\"replay\",\"duration_ms\":{},\"pos_ms\":{},\"playing\":{},\"speed\":{},\"records\":{}}}",
                r.session.duration_ms(),
                r.pos_ms(),
                r.playing,
                r.speed,
                r.session.records.len()
            ),
        }
    }
}
