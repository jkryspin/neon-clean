//! Process access lives in `neon-win`; the scanner trait stays here.

pub use neon_win::process::*;

impl crate::scan::Memory for Process {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn scan_regions(&self, scope: &crate::region::Scope) -> Vec<(u64, u64)> {
        use crate::region::{Scope, HEAP_MIN};
        let allowed = |r: &Region| match scope {
            Scope::All => true,
            Scope::Private => r.kind == Kind::Private,
            Scope::Mapped => r.kind == Kind::Mapped,
            Scope::Image => r.kind == Kind::Image,
            Scope::Heap => r.kind == Kind::Private && r.writable && r.size >= HEAP_MIN,
            Scope::Module(_) => true,
            Scope::Range(b, s) => r.base < b + s && r.base + r.size > *b,
        };
        let window = match scope {
            Scope::Module(name) => self
                .module_by_name(name)
                .map(|m| (m.base, m.base + m.size))
                .unwrap_or((1, 0)),
            Scope::Range(b, s) => (*b, b.saturating_add(*s)),
            _ => (0x10000, 0x0000_7FFF_FFFF_FFFF),
        };
        let mut out = Vec::new();
        for r in self.regions(window.0, window.1, false) {
            if !allowed(&r) {
                continue;
            }
            let mut base = r.base.max(window.0);
            let end = (r.base + r.size).min(window.1);
            while base < end {
                let len = (end - base).min(MAX_REGION);
                out.push((base, len));
                base += len;
            }
        }
        out
    }

    fn read_into(&self, addr: u64, buf: &mut [u8]) -> usize {
        Process::read_into(self, addr, buf)
    }
}
