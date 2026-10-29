//! Read access to the game process memory.
//! Linux: /proc/<pid>/mem + /proc/<pid>/maps
//! macOS: mach_vm_read + mach_vm_region (requires task_for_pid access)
//!
//! God Mode support (Phase 4): differential scanning to find the iframe timer.

use crate::chains::Mem;

pub struct MemoryScanner {
    pid: u32,
    #[cfg(target_os = "linux")]
    mem: std::fs::File,
    #[cfg(target_os = "macos")]
    task: u32,
}

// ── God Mode: iframe timer detection and freeze ──

/// A snapshot of a single address at a point in time.
#[derive(Debug, Clone, Copy)]
pub struct MemSnapshot {
    pub addr: u64,
    pub value: u32,
    pub ts_ms: u64,
}

/// GodModeScanner performs differential memory scanning to find the iframe timer address.
///
/// Algorithm:
/// 1. On HP decrease: scan RW regions for values that changed from 0 → positive
/// 2. Between damage: filter to addresses decreasing toward 0
/// 3. The surviving address is the iframe timer — freeze it with a high value
#[derive(Debug, Default)]
pub struct GodModeScanner {
    /// Previous snapshots of candidate addresses
    prev: std::collections::HashMap<u64, u32>,
    /// Current candidates (addresses that went 0 → positive on damage)
    candidates: std::collections::HashMap<u64, u32>,
    /// How many damage cycles each candidate has survived
    survival: std::collections::HashMap<u64, u32>,
    /// The confirmed iframe timer address (once found)
    confirmed: Option<u64>,
}

impl GodModeScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a damage event (HP just decreased).
    /// This triggers a scan of RW regions for iframe timer candidates.
    pub fn on_damage(&mut self, mem: &dyn Mem, regions: &[(u64, u64)], ts_ms: u64) {
        if self.confirmed.is_some() {
            return; // Already found it
        }

        // Scan RW regions for values that changed from 0 → positive
        for &(start, end) in regions {
            // Read in 4-byte chunks (u32 values)
            let mut addr = start;
            while addr + 4 <= end {
                if let Some(data) = mem.read_bytes(addr, 4) {
                    let val = u32::from_le_bytes(data.try_into().unwrap());
                    let prev = self.prev.get(&addr).copied().unwrap_or(0);

                    // Candidate: was 0 (or near 0), now positive (iframe timer activating)
                    if prev == 0 && val > 0 && val < 0x7FFFFFFF {
                        self.candidates.insert(addr, val);
                        self.survival.entry(addr).or_insert(0);
                    }

                    self.prev.insert(addr, val);
                }
                addr += 4;
            }
        }
    }

    /// Record a non-damage tick (HP stable or increasing).
    /// Filter candidates: those that decrease toward 0 are iframe timers.
    pub fn on_stable(&mut self, mem: &dyn Mem, regions: &[(u64, u64)]) {
        if self.confirmed.is_some() {
            return;
        }

        let mut to_remove = vec![];
        for (&addr, &prev_val) in &self.candidates {
            if let Some(data) = mem.read_bytes(addr, 4) {
                let val = u32::from_le_bytes(data.try_into().unwrap());

                // If the value is decreasing toward 0, it's likely the iframe timer
                if val < prev_val {
                    let count = self.survival.get(&addr).copied().unwrap_or(0) + 1;
                    self.survival.insert(addr, count);

                    // If it's dropped to near 0 again, this is a strong candidate
                    if val < 10 {
                        to_remove.push(addr);
                    }
                } else if val == prev_val && val > 100 {
                    // Stuck at a high value — unlikely to be iframe timer
                    to_remove.push(addr);
                }
            }
        }

        for addr in to_remove {
            self.candidates.remove(&addr);
        }
    }

    /// Check if we have enough evidence to confirm an iframe timer address.
    /// Need at least 3 damage cycles survived and only 1 candidate remaining.
    pub fn try_confirm(&self) -> Option<u64> {
        if self.candidates.len() != 1 {
            return None;
        }

        let (&addr, &count) = self.candidates.iter().next()?;
        let survival = self.survival.get(&addr).copied().unwrap_or(0);

        if survival >= 3 {
            Some(addr)
        } else {
            None
        }
    }

    /// Get statistics about the scan progress.
    pub fn stats(&self) -> GodModeStats {
        GodModeStats {
            candidate_count: self.candidates.len(),
            confirmed: self.confirmed,
            survival_counts: self.survival.iter()
                .filter(|(&addr, _)| self.candidates.contains_key(&addr))
                .map(|(&addr, &count)| (addr, count))
                .collect(),
        }
    }

    /// Reset the scanner (e.g., after map change).
    pub fn reset(&mut self) {
        self.prev.clear();
        self.candidates.clear();
        self.survival.clear();
        self.confirmed = None;
    }
}

/// State for freezing the iframe timer (God Mode active).
pub struct GodModeFreezer {
    /// The confirmed iframe timer address
    timer_addr: u64,
    /// Value to write (high value = long iframe period)
    freeze_value: u32,
    /// How many ticks since last freeze
    tick_count: u32,
    /// Freeze every N ticks (to avoid detection)
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

    /// On each tick, write the freeze value if it's time to do so.
    /// Returns true if a freeze was applied.
    pub fn tick(&mut self, mem: &dyn Mem) -> bool {
        self.tick_count += 1;
        if self.tick_count % self.freeze_interval != 0 {
            return false;
        }

        #[cfg(target_os = "macos")]
        {
            // Use mach_vm_write to freeze the value
            // This requires appropriate privileges
            return false; // Placeholder — actual implementation needs task port
        }

        #[cfg(target_os = "linux")]
        {
            // Use pwrite to /proc/<pid>/mem
            // This requires appropriate permissions
            return false; // Placeholder — actual implementation needs pid
        }

        // The actual freeze implementation is in the platform-specific code
        // that has access to the process handle
    }

    pub fn timer_addr(&self) -> u64 {
        self.timer_addr
    }

    pub fn freeze_value(&self) -> u32 {
        self.freeze_value
    }
}

/// Statistics for God Mode scanning.
#[derive(Debug, Clone)]
pub struct GodModeStats {
    pub candidate_count: usize,
    pub confirmed: Option<u64>,
    pub survival_counts: std::collections::HashMap<u64, u32>,
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::os::unix::fs::FileExt;

    impl MemoryScanner {
        pub fn new(pid: u32) -> Result<Self, String> {
            let path = format!("/proc/{}/mem", pid);
            let mem = std::fs::File::open(&path).map_err(|e| format!("Failed to open {}: {}", path, e))?;
            Ok(Self { pid, mem })
        }

        pub fn rw_anon_regions(&self) -> Result<Vec<(u64, u64)>, String> {
            let path = format!("/proc/{}/maps", self.pid);
            let text = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path, e))?;
            let mut out = vec![];
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 2 || !parts[1].starts_with("rw") || parts.len() > 5 {
                    continue;
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

        pub fn read_bytes(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
            let mut buf = vec![0u8; len];
            self.mem.read_exact_at(&mut buf, addr).ok()?;
            Some(buf)
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    extern "C" {
        fn task_for_pid(target_tport: u32, pid: i32, task: *mut u32) -> i32;
    }

    impl MemoryScanner {
        pub fn new(pid: u32) -> Result<Self, String> {
            let mut task: u32 = 0;
            let kr = unsafe { task_for_pid(mach2::traps::mach_task_self(), pid as i32, &mut task) };
            if kr != 0 {
                return Err(format!(
                    "task_for_pid({}) failed (kern_return {}). \
                     Run as root, or disable SIP (csrutil disable), or sign the binary with \
                     com.apple.security.cs.debugger entitlement.",
                    pid, kr
                ));
            }
            Ok(Self { pid, task })
        }

        pub fn rw_anon_regions(&self) -> Result<Vec<(u64, u64)>, String> {
            use mach2::vm_region::*;

            let info_count_init = (std::mem::size_of::<vm_region_basic_info_data_64_t>() / std::mem::size_of::<i32>()) as u32;
            let mut out = vec![];
            let mut address: u64 = 0;
            let mut size: u64 = 0;
            let mut info: vm_region_basic_info_data_64_t = unsafe { std::mem::zeroed() };
            let mut info_count = info_count_init;
            let mut object_name: u32 = 0;

            loop {
                let kr = unsafe {
                    mach2::vm::mach_vm_region(
                        self.task,
                        &mut address,
                        &mut size,
                        VM_REGION_BASIC_INFO_64,
                        &mut info as *mut _ as *mut i32,
                        &mut info_count,
                        &mut object_name,
                    )
                };
                if kr != 0 {
                    break;
                }
                let rw = (info.protection & (mach2::vm_prot::VM_PROT_READ | mach2::vm_prot::VM_PROT_WRITE))
                    == (mach2::vm_prot::VM_PROT_READ | mach2::vm_prot::VM_PROT_WRITE);
                if rw && size <= 0x4000_0000 {
                    out.push((address, address + size));
                }
                address += size;
                info_count = info_count_init;
            }
            Ok(out)
        }

        pub fn alive(&self) -> bool {
            unsafe { libc::kill(self.pid as i32, 0) == 0 }
        }

        pub fn read_bytes(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
            let mut data_ptr: usize = 0;
            let mut data_cnt: u32 = 0;
            let kr = unsafe {
                mach2::vm::mach_vm_read(
                    self.task,
                    addr,
                    len as u64,
                    &mut data_ptr,
                    &mut data_cnt,
                )
            };
            if kr != 0 {
                return None;
            }
            let buf = unsafe { std::slice::from_raw_parts(data_ptr as *const u8, data_cnt as usize) }.to_vec();
            unsafe {
                mach2::vm::mach_vm_deallocate(
                    mach2::traps::mach_task_self(),
                    data_ptr as u64,
                    data_cnt as u64,
                );
            }
            Some(buf)
        }
    }
}

impl Mem for MemoryScanner {
    fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
        self.read_bytes(addr, len)
    }
}
