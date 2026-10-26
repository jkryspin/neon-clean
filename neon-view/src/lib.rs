//! Neon View: server, UI, inspector, replay and opcode explorer on top of `neon-world`.

pub mod inspector;
pub mod server;

use neon_protocol::Registry;
use neon_world::{event_from_packet, World};

/// Everything the view serves, behind one lock.
#[derive(Debug, Default)]
pub struct Shared {
    pub world: World,
    pub packets: inspector::Inspector,
    /// Directory of per-map geometry files (`<id>.json`), served at `/map/<id>`; its parent holds the name tables.
    pub maps_dir: Option<std::path::PathBuf>,
}

impl Shared {
    /// Feed one plaintext packet: inspector always, world when it maps to an event.
    pub fn feed(&mut self, reg: &Registry, plain: &[u8], t_ms: u64) {
        self.packets.record(reg, plain, t_ms);
        if let Some(e) = event_from_packet(reg, plain) {
            self.world.apply(&e, t_ms);
        }
    }
}
