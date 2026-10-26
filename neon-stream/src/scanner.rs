//! Read access to the game process via /proc/<pid>/mem.

use crate::chains::Mem;
use std::fs::File;
use std::os::unix::fs::FileExt;

pub struct MemoryScanner {
    pid: u32,
    mem: File,
}

impl MemoryScanner {
    pub fn new(pid: u32) -> Result<Self, String> {
        let path = format!("/proc/{}/mem", pid);
        let mem = File::open(&path).map_err(|e| format!("Failed to open {}: {}", path, e))?;
        Ok(Self { pid, mem })
    }

    /// Writable, unnamed mappings: where the game's heap objects live.
    /// Excludes Wine's own libraries and gigantic reservations.
    pub fn rw_anon_regions(&self) -> Result<Vec<(u64, u64)>, String> {
        let path = format!("/proc/{}/maps", self.pid);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path, e))?;
        let mut out = vec![];
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 || !parts[1].starts_with("rw") || parts.len() > 5 {
                continue; // named mapping (library/file) or not writable
            }
            let mut it = parts[0].split('-');
            if let (Some(a), Some(b)) = (it.next(), it.next()) {
                if let (Ok(a), Ok(b)) = (u64::from_str_radix(a, 16), u64::from_str_radix(b, 16)) {
                    if b > a && b - a <= 0x4000_0000 {
                        out.push((a, b));
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn alive(&self) -> bool {
        std::path::Path::new(&format!("/proc/{}", self.pid)).exists()
    }
}

impl Mem for MemoryScanner {
    fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.mem.read_exact_at(&mut buf, addr).ok()?;
        Some(buf)
    }
}
