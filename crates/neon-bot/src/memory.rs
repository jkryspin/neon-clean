//! Memory scanning for MapleStory (legacy; the main loop reads from neon-stream).
//!
//! Linux: /proc/pid/mem + /proc/pid/maps
//! macOS: mach_vm_read + mach_vm_region (requires task_for_pid access)

use tracing::debug;
use std::time::Duration;

pub struct MemoryScanner {
    pid: u32,
    base_address: usize,
    #[cfg(target_os = "macos")]
    task: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Mob {
    pub id: u32,
    pub x: i16,
    pub y: i16,
    pub hp_percent: u8,
    pub layer: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerStats {
    pub hp: u32,
    pub max_hp: u32,
    pub mp: u32,
    pub max_mp: u32,
    pub exp: u64,
    pub level: u16,
}

#[derive(Debug, Clone)]
struct Region {
    start: u64,
    end: u64,
    #[cfg(target_os = "linux")]
    perms: String,
    #[cfg(target_os = "linux")]
    label: String,
    #[cfg(target_os = "macos")]
    protection: i32,
}

impl Region {
    fn is_rw_anon(&self) -> bool {
        #[cfg(target_os = "linux")]
        { self.perms.starts_with("rw") && self.label.is_empty() }
        #[cfg(target_os = "macos")]
        {
            let rw = (self.protection & 1 != 0) && (self.protection & 2 != 0);
            rw
        }
    }
}

// ---- Linux implementation ----

#[cfg(target_os = "linux")]
impl MemoryScanner {
    pub fn new(pid: u32) -> Result<Self, String> {
        use std::fs::File;
        let mem_path = format!("/proc/{}/mem", pid);
        File::open(&mem_path)
            .map_err(|e| format!("Failed to open {}: {}", mem_path, e))?;

        let base_address = Self::find_module_base(pid).unwrap_or(0x100000);
        debug!("Using base address: 0x{:X}", base_address);

        Ok(Self { pid, base_address })
    }

    fn find_module_base(pid: u32) -> Option<usize> {
        let maps_path = format!("/proc/{}/maps", pid);
        let content = std::fs::read_to_string(&maps_path).ok()?;

        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 { continue; }

            let addr_range: Vec<&str> = parts[0].split('-').collect();
            if addr_range.len() != 2 { continue; }

            let start = u64::from_str_radix(addr_range[0], 16).ok()?;
            let perms = parts[1];

            if perms.starts_with("rw") && start > 0x10000 {
                return Some(start as usize);
            }
        }
        None
    }

    fn get_regions(&self) -> Result<Vec<Region>, String> {
        let maps_path = format!("/proc/{}/maps", self.pid);
        let content = std::fs::read_to_string(&maps_path)
            .map_err(|e| format!("Failed to read {}: {}", maps_path, e))?;

        let mut regions = vec![];
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 { continue; }

            let addr_range: Vec<&str> = parts[0].split('-').collect();
            if addr_range.len() != 2 { continue; }

            let start = u64::from_str_radix(addr_range[0], 16)
                .map_err(|e| format!("Failed to parse start addr: {}", e))?;
            let end = u64::from_str_radix(addr_range[1], 16)
                .map_err(|e| format!("Failed to parse end addr: {}", e))?;

            let perms = parts[1].to_string();
            let label = parts.get(5).map(|s| *s).unwrap_or("");

            regions.push(Region { start, end, perms, label: label.to_string() });
        }
        Ok(regions)
    }

    fn get_rw_anon_regions(&self) -> Result<Vec<Region>, String> {
        let regions = self.get_regions()?;
        Ok(regions.into_iter().filter(|r| r.is_rw_anon()).collect())
    }

    pub fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>, String> {
        use std::fs::File;
        use std::io::{Read, Seek, SeekFrom};

        let mut buf = vec![0u8; size];
        let mem_path = format!("/proc/{}/mem", self.pid);

        let mut file = File::open(&mem_path)
            .map_err(|e| format!("Failed to open /proc/{}/mem: {}", self.pid, e))?;

        file.seek(SeekFrom::Start(address as u64))
            .map_err(|e| format!("Failed to seek to 0x{:X}: {}", address, e))?;

        file.read_exact(&mut buf)
            .map_err(|e| format!("Failed to read {} bytes: {}", size, e))?;

        Ok(buf)
    }
}

// ---- macOS implementation ----

#[cfg(target_os = "macos")]
impl MemoryScanner {
    pub fn new(pid: u32) -> Result<Self, String> {
        extern "C" {
            fn task_for_pid(target_tport: u32, pid: i32, task: *mut u32) -> i32;
        }
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

        let base_address = Self::find_module_base(task).unwrap_or(0x200000000);
        debug!("Using base address: 0x{:X}", base_address);

        Ok(Self { pid, base_address, task })
    }

    fn find_module_base(task: u32) -> Option<usize> {
        use mach2::vm_region::*;
        let info_count_init = (std::mem::size_of::<vm_region_basic_info_data_64_t>() / std::mem::size_of::<i32>()) as u32;
        let mut address: u64 = 0;

        unsafe {
            loop {
                let mut size: u64 = 0;
                let mut info: vm_region_basic_info_data_64_t = std::mem::zeroed();
                let mut info_count = info_count_init;
                let mut object_name: u32 = 0;

                let kr = mach2::vm::mach_vm_region(
                    task, &mut address, &mut size, VM_REGION_BASIC_INFO_64,
                    &mut info as *mut _ as *mut i32, &mut info_count, &mut object_name,
                );
                if kr != 0 { break; }

                let rx = (info.protection & (mach2::vm_prot::VM_PROT_READ | mach2::vm_prot::VM_PROT_EXECUTE))
                    == (mach2::vm_prot::VM_PROT_READ | mach2::vm_prot::VM_PROT_EXECUTE);
                if rx && size > 0x100000 && address > 0x100000000 {
                    return Some(address as usize);
                }
                address += size;
            }
        }
        None
    }

    fn get_rw_anon_regions(&self) -> Result<Vec<Region>, String> {
        use mach2::vm_region::*;
        let info_count_init = (std::mem::size_of::<vm_region_basic_info_data_64_t>() / std::mem::size_of::<i32>()) as u32;
        let mut out = vec![];
        let mut address: u64 = 0;

        unsafe {
            loop {
                let mut size: u64 = 0;
                let mut info: vm_region_basic_info_data_64_t = std::mem::zeroed();
                let mut info_count = info_count_init;
                let mut object_name: u32 = 0;

                let kr = mach2::vm::mach_vm_region(
                    self.task, &mut address, &mut size, VM_REGION_BASIC_INFO_64,
                    &mut info as *mut _ as *mut i32, &mut info_count, &mut object_name,
                );
                if kr != 0 { break; }

                let rw = (info.protection & (mach2::vm_prot::VM_PROT_READ | mach2::vm_prot::VM_PROT_WRITE))
                    == (mach2::vm_prot::VM_PROT_READ | mach2::vm_prot::VM_PROT_WRITE);
                if rw && size <= 0x4000_0000 {
                    out.push(Region { start: address, end: address + size, protection: info.protection });
                }
                address += size;
            }
        }
        Ok(out)
    }

    pub fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>, String> {
        let mut data_ptr: usize = 0;
        let mut data_cnt: u32 = 0;
        let kr = unsafe {
            mach2::vm::mach_vm_read(
                self.task,
                address as u64,
                size as u64,
                &mut data_ptr,
                &mut data_cnt,
            )
        };
        if kr != 0 {
            return Err(format!("mach_vm_read at 0x{:x} failed (kr={})", address, kr));
        }
        let buf = unsafe { std::slice::from_raw_parts(data_ptr as *const u8, data_cnt as usize) }.to_vec();
        unsafe {
            mach2::vm::mach_vm_deallocate(
                mach2::traps::mach_task_self(),
                data_ptr as u64,
                data_cnt as u64,
            );
        }
        Ok(buf)
    }
}

// ---- Shared implementation ----

impl MemoryScanner {
    pub fn read_u32(&self, address: usize) -> Result<u32, String> {
        let data = self.read_memory(address, 4)?;
        Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    }

    pub fn read_u16(&self, address: usize) -> Result<u16, String> {
        let data = self.read_memory(address, 2)?;
        Ok(u16::from_le_bytes([data[0], data[1]]))
    }

    pub fn read_i32(&self, address: usize) -> Result<i32, String> {
        let data = self.read_memory(address, 4)?;
        Ok(i32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    }

    pub fn read_i16(&self, address: usize) -> Result<i16, String> {
        let data = self.read_memory(address, 2)?;
        Ok(i16::from_le_bytes([data[0], data[1]]))
    }

    pub fn read_u8(&self, address: usize) -> Result<u8, String> {
        let data = self.read_memory(address, 1)?;
        Ok(data[0])
    }

    pub fn get_player_position(&self) -> Result<(i16, i16), String> {
        const POS_ADDR: usize = 0x109248;
        const POS_MIRRORS: [usize; 2] = [0x109248, 0x109928];

        let mut last_pos: Option<(i32, i32)> = None;

        for _ in 0..10 {
            for &addr in &POS_MIRRORS {
                let data = self.read_memory(addr, 8)?;
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);

                if x.abs() < 20000 && y.abs() < 20000 && (x != 0 || y != 0) {
                    if let Some(last) = last_pos {
                        if last == (x, y) {
                            return Ok((x as i16, y as i16));
                        }
                    }
                    last_pos = Some((x, y));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        Ok(last_pos.map(|(x, y)| (x as i16, y as i16)).unwrap_or((0, 0)))
    }

    pub fn get_map_id(&self) -> Result<u32, String> {
        const MAP_ADDR: usize = 0x108b98;
        self.read_u32(MAP_ADDR)
    }

    pub fn get_player_stats(&self) -> Result<PlayerStats, String> {
        const POS_ADDR: usize = 0x109248;
        const HP_OFFSET: usize = 16;
        const MAX_HP_OFFSET: usize = 20;
        const MP_OFFSET: usize = 24;
        const MAX_MP_OFFSET: usize = 28;

        let level_addrs = [
            POS_ADDR - 0x6B4,
            POS_ADDR - 0x6B8,
            POS_ADDR - 4,
            POS_ADDR - 8,
        ];

        let mut level: u16 = 1;
        for &addr in &level_addrs {
            if let Ok(lvl) = self.read_u16(addr) {
                if lvl >= 1 && lvl <= 200 {
                    level = lvl;
                    debug!("Found level {} at 0x{:X}", level, addr);
                    break;
                }
            }
        }

        let hp_addr = POS_ADDR + HP_OFFSET;
        let max_hp_addr = POS_ADDR + MAX_HP_OFFSET;
        let mp_addr = POS_ADDR + MP_OFFSET;
        let max_mp_addr = POS_ADDR + MAX_MP_OFFSET;

        let hp = self.read_u32(hp_addr)?;
        let max_hp = self.read_u32(max_hp_addr)?;
        let mp = self.read_u32(mp_addr)?;
        let max_mp = self.read_u32(max_mp_addr)?;

        if max_hp == 0 || max_hp > 1_000_000 {
            debug!("Suspicious max_hp value: {}, using fallback", max_hp);
        }

        Ok(PlayerStats { hp, max_hp, mp, max_mp, exp: 0, level })
    }

    pub fn scan_mobs(&self) -> Result<Vec<Mob>, String> {
        let mut mobs = vec![];

        let player_pos = self.get_player_position().unwrap_or((0, 0));
        let regions = match self.get_rw_anon_regions() {
            Ok(r) => r,
            Err(e) => {
                debug!("Failed to get regions: {}", e);
                return Ok(mobs);
            }
        };

        for region in regions {
            let region_size = (region.end - region.start) as usize;
            if region_size < 4096 { continue; }

            let data = match self.read_memory(region.start as usize, region_size.min(0x10000)) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let candidates = self.find_mob_candidates_relaxed(&data, region.start as usize, player_pos);

            for (_, mob_data) in candidates {
                mobs.push(Mob {
                    id: mob_data.tick as u32,
                    x: mob_data.x as i16,
                    y: mob_data.y as i16,
                    hp_percent: 100,
                    layer: 0,
                });
            }
        }

        mobs.sort_by_key(|m| (m.x, m.y));
        mobs.dedup_by_key(|m| (m.x, m.y));

        debug!("Mob scan complete: {} unique mobs (player at {:?})", mobs.len(), player_pos);
        Ok(mobs)
    }

    fn find_mob_candidates_relaxed(&self, data: &[u8], base_addr: usize, player_pos: (i16, i16)) -> Vec<(usize, MobData)> {
        let mut candidates = vec![];
        let mut pattern_count = std::collections::HashMap::<(i32, i32), u32>::new();

        if data.len() < 24 { return candidates; }

        let y_offsets = [1, 2, 3, 4, 5];

        for &y_offset in &y_offsets {
            let mut offset = 0;
            while offset + 16 <= data.len() {
                let idx = offset / 4;
                let x = le_i32(data, idx);
                let y = le_i32(data, idx + y_offset);

                if x.abs() < 2000 && y.abs() < 2000 && (x != 0 || y != 0) {
                    let count = pattern_count.entry((x, y)).or_insert(0);
                    *count += 1;

                    if *count > 15 {
                        offset += 4;
                        continue;
                    }

                    let dist_sq = (x - player_pos.0 as i32).pow(2) + (y - player_pos.1 as i32).pow(2);
                    let near_player = dist_sq < 150 * 150 && dist_sq > 50;

                    if near_player {
                        let dx = le_i32(data, idx + y_offset + 1);
                        let dy = le_i32(data, idx + y_offset + 2);

                        let valid_sprite = dx.abs() < 100 && dy.abs() < 150;

                        if valid_sprite {
                            candidates.push((
                                base_addr + offset,
                                MobData { tick: idx as i32, x, y, dx, dy },
                            ));
                        }
                    }
                }
                offset += 4;
            }
        }

        candidates
    }

    pub fn get_nearest_mob(&self) -> Result<Option<Mob>, String> {
        let mobs = self.scan_mobs()?;
        let player_pos = self.get_player_position()?;

        let mut nearest: Option<Mob> = None;
        let mut nearest_dist = f64::MAX;

        for mob in mobs {
            let dx = mob.x as i32 - player_pos.0 as i32;
            let dy = mob.y as i32 - player_pos.1 as i32;
            let dist = ((dx * dx + dy * dy) as f64).sqrt();

            if dist < nearest_dist {
                nearest_dist = dist;
                nearest = Some(mob);
            }
        }

        Ok(nearest)
    }
}

#[derive(Debug, Clone)]
struct MobData {
    tick: i32,
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
}

fn le_i32(data: &[u8], word_idx: usize) -> i32 {
    let offset = word_idx * 4;
    if offset + 4 > data.len() { return 0; }
    i32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_le_i32() {
        let data = [0x01, 0x02, 0x03, 0x04];
        assert_eq!(le_i32(&data, 0), 0x04030201);
    }
}
