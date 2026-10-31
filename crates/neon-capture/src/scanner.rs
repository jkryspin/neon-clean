//! Read access to the game process memory on Windows.
//!
//! God Mode support: differential scanning to find the iframe timer.

use neon_win::process::{Kind, Process};

use crate::chains::Mem;

pub struct MemoryScanner {
    pid: u32,
    process: Process,
}

#[derive(Debug, Clone, Copy)]
pub struct MemSnapshot {
    pub addr: u64,
    pub value: u32,
    pub ts_ms: u64,
}

#[derive(Debug, Default)]
pub struct GodModeScanner {
    prev: std::collections::HashMap<u64, u32>,
    candidates: std::collections::HashMap<u64, u32>,
    survival: std::collections::HashMap<u64, u32>,
    confirmed: Option<u64>,
}

impl GodModeScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_damage(&mut self, mem: &dyn Mem, regions: &[(u64, u64)], _ts_ms: u64) {
        if self.confirmed.is_some() {
            return;
        }
        for &(start, end) in regions {
            let mut addr = start;
            while addr + 4 <= end {
                if let Some(data) = mem.read_bytes(addr, 4) {
                    let val = u32::from_le_bytes(data.try_into().unwrap());
                    let prev = self.prev.get(&addr).copied().unwrap_or(0);
                    if prev == 0 && val > 0 && val < 0x7FFF_FFFF {
                        self.candidates.insert(addr, val);
                        self.survival.entry(addr).or_insert(0);
                    }
                    self.prev.insert(addr, val);
                }
                addr += 4;
            }
        }
    }

    pub fn on_stable(&mut self, mem: &dyn Mem, _regions: &[(u64, u64)]) {
        if self.confirmed.is_some() {
            return;
        }
        let mut to_remove = vec![];
        for (&addr, &prev_val) in &self.candidates {
            if let Some(data) = mem.read_bytes(addr, 4) {
                let val = u32::from_le_bytes(data.try_into().unwrap());
                if val < prev_val {
                    let count = self.survival.get(&addr).copied().unwrap_or(0) + 1;
                    self.survival.insert(addr, count);
                    if val < 10 {
                        to_remove.push(addr);
                    }
                } else if val == prev_val && val > 100 {
                    to_remove.push(addr);
                }
            }
        }
        for addr in to_remove {
            self.candidates.remove(&addr);
        }
    }

    pub fn try_confirm(&self) -> Option<u64> {
        if self.candidates.len() != 1 {
            return None;
        }
        let (&addr, _) = self.candidates.iter().next()?;
        (self.survival.get(&addr).copied().unwrap_or(0) >= 3).then_some(addr)
    }

    pub fn stats(&self) -> GodModeStats {
        GodModeStats {
            candidate_count: self.candidates.len(),
            confirmed: self.confirmed,
            survival_counts: self
                .survival
                .iter()
                .filter(|(&addr, _)| self.candidates.contains_key(&addr))
                .map(|(&addr, &count)| (addr, count))
                .collect(),
        }
    }

    pub fn reset(&mut self) {
        self.prev.clear();
        self.candidates.clear();
        self.survival.clear();
        self.confirmed = None;
    }
}

pub struct GodModeFreezer {
    timer_addr: u64,
    freeze_value: u32,
    tick_count: u32,
    freeze_interval: u32,
}

impl GodModeFreezer {
    pub fn new(timer_addr: u64, freeze_value: u32, freeze_interval: u32) -> Self {
        Self {
            timer_addr,
            freeze_value,
            tick_count: 0,
            freeze_interval,
        }
    }

    pub fn tick(&mut self, mem: &mut dyn MemWrite) -> bool {
        self.tick_count += 1;
        if self.tick_count % self.freeze_interval != 0 {
            return false;
        }
        mem.write_u32(self.timer_addr, self.freeze_value).is_some()
    }

    pub fn timer_addr(&self) -> u64 {
        self.timer_addr
    }

    pub fn freeze_value(&self) -> u32 {
        self.freeze_value
    }
}

pub trait MemWrite {
    fn write_u32(&mut self, addr: u64, value: u32) -> Option<()>;
}

#[derive(Debug, Clone)]
pub struct GodModeStats {
    pub candidate_count: usize,
    pub confirmed: Option<u64>,
    pub survival_counts: std::collections::HashMap<u64, u32>,
}

impl MemoryScanner {
    pub fn new(pid: u32) -> Result<Self, String> {
        let process = Process::open(pid).map_err(|e| e.to_string())?;
        Ok(Self { pid, process })
    }

    pub fn rw_anon_regions(&self) -> Result<Vec<(u64, u64)>, String> {
        let mut out = vec![];
        for r in self.process.all_regions(false) {
            if r.kind == Kind::Private && r.writable && r.size <= 0x4000_0000 {
                out.push((r.base, r.base + r.size));
            }
        }
        Ok(out)
    }

    pub fn alive(&self) -> bool {
        self.process.alive()
    }

    pub fn read_bytes(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
        self.process.read(addr, len)
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }
}

impl Mem for MemoryScanner {
    fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
        self.read_bytes(addr, len)
    }
}

impl MemWrite for MemoryScanner {
    fn write_u32(&mut self, addr: u64, value: u32) -> Option<()> {
        self.process.write(addr, &value.to_le_bytes()).ok()
    }
}
