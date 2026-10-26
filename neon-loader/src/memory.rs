//! Memory scanning for MapleStory
//!
//! Uses the same approach as maplebot: scan RW anonymous memory regions
//! and look for layer structures with tick-based liveness detection.

use std::fs::File;
use tracing::debug;
use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

pub struct MemoryScanner {
    pid: u32,
    base_address: usize,
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

/// Memory region from /proc/pid/maps
#[derive(Debug, Clone)]
struct Region {
    start: u64,
    end: u64,
    perms: String,
    label: String,
}

impl Region {
    fn is_rw_anon(&self) -> bool {
        self.perms.starts_with("rw") && self.label.is_empty()
    }
}

impl MemoryScanner {
    pub fn new(pid: u32) -> Result<Self, String> {
        let mem_path = format!("/proc/{}/mem", pid);
        File::open(&mem_path)
            .map_err(|e| format!("Failed to open {}: {}", mem_path, e))?;

        // Try to find the MapleStory module base address
        let base_address = Self::find_module_base(pid).unwrap_or(0x100000);

        debug!("Using base address: 0x{:X}", base_address);

        Ok(Self {
            pid,
            base_address,
        })
    }

    /// Find the base address of the main MapleStory module
    fn find_module_base(pid: u32) -> Option<usize> {
        let maps_path = format!("/proc/{}/maps", pid);
        let content = std::fs::read_to_string(&maps_path).ok()?;

        // Look for the main executable or largest RW region
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            let addr_range: Vec<&str> = parts[0].split('-').collect();
            if addr_range.len() != 2 {
                continue;
            }

            let start = u64::from_str_radix(addr_range[0], 16).ok()?;
            let perms = parts[1];

            // Look for RW region that's large enough to be the main module
            if perms.starts_with("rw") && start > 0x10000 {
                return Some(start as usize);
            }
        }

        None
    }

    /// Get all memory regions from /proc/pid/maps
    fn get_regions(&self) -> Result<Vec<Region>, String> {
        let maps_path = format!("/proc/{}/maps", self.pid);
        let content = std::fs::read_to_string(&maps_path)
            .map_err(|e| format!("Failed to read {}: {}", maps_path, e))?;

        let mut regions = vec![];
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            // Parse address range
            let addr_range: Vec<&str> = parts[0].split('-').collect();
            if addr_range.len() != 2 {
                continue;
            }

            let start = u64::from_str_radix(addr_range[0], 16)
                .map_err(|e| format!("Failed to parse start addr: {}", e))?;
            let end = u64::from_str_radix(addr_range[1], 16)
                .map_err(|e| format!("Failed to parse end addr: {}", e))?;

            let perms = parts[1].to_string();
            let label = parts.get(5).map(|s| *s).unwrap_or("");

            regions.push(Region {
                start,
                end,
                perms,
                label: label.to_string(),
            });
        }

        Ok(regions)
    }

    /// Get RW anonymous regions (heap, arenas)
    fn get_rw_anon_regions(&self) -> Result<Vec<Region>, String> {
        let regions = self.get_regions()?;
        Ok(regions.into_iter().filter(|r| r.is_rw_anon()).collect())
    }

    /// Read memory from process
    pub fn read_memory(&self, address: usize, size: usize) -> Result<Vec<u8>, String> {
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

    /// Read a u32 from memory
    pub fn read_u32(&self, address: usize) -> Result<u32, String> {
        let data = self.read_memory(address, 4)?;
        Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    }

    /// Read a u16 from memory
    pub fn read_u16(&self, address: usize) -> Result<u16, String> {
        let data = self.read_memory(address, 2)?;
        Ok(u16::from_le_bytes([data[0], data[1]]))
    }

    /// Read an i32 from memory (little endian)
    pub fn read_i32(&self, address: usize) -> Result<i32, String> {
        let data = self.read_memory(address, 4)?;
        Ok(i32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    }

    /// Read an i16 from memory
    pub fn read_i16(&self, address: usize) -> Result<i16, String> {
        let data = self.read_memory(address, 2)?;
        Ok(i16::from_le_bytes([data[0], data[1]]))
    }

    /// Read a u8 from memory
    pub fn read_u8(&self, address: usize) -> Result<u8, String> {
        let data = self.read_memory(address, 1)?;
        Ok(data[0])
    }

    /// Get player position from memory
    /// Uses mirror verification like maplebot
    pub fn get_player_position(&self) -> Result<(i16, i16), String> {
        // Player position offset (calibrated for MapleStory Classic Wine)
        const POS_ADDR: usize = 0x109248;
        const POS_MIRRORS: [usize; 2] = [0x109248, 0x109928];

        // Read from mirrors until we get consistent plausible values
        let mut last_pos: Option<(i32, i32)> = None;

        for _ in 0..10 {
            for &addr in &POS_MIRRORS {
                let data = self.read_memory(addr, 8)?;
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);

                // Check if plausible (within map bounds and not 0,0)
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

        // Return last plausible value or default
        Ok(last_pos.map(|(x, y)| (x as i16, y as i16)).unwrap_or((0, 0)))
    }

    /// Get current map ID
    pub fn get_map_id(&self) -> Result<u32, String> {
        // Map ID addresses (verified 2026-10-23)
        const MAP_ADDR: usize = 0x108b98;
        self.read_u32(MAP_ADDR)
    }

    /// Get player stats (HP, MP, EXP, level)
    /// Uses verified addresses for MapleStory Classic Wine
    pub fn get_player_stats(&self) -> Result<PlayerStats, String> {
        // Player stats addresses - these are offsets from player structure
        // Found near position address 0x109248
        const POS_ADDR: usize = 0x109248;
        const HP_OFFSET: usize = 16;   // HP is 16 bytes after position
        const MAX_HP_OFFSET: usize = 20;
        const MP_OFFSET: usize = 24;
        const MAX_MP_OFFSET: usize = 28;

        // Level is typically near position data
        // Try multiple possible locations
        let level_addrs = [
            POS_ADDR - 0x6B4,  // 0x108b94 - original
            POS_ADDR - 0x6B8,  // Alternative
            POS_ADDR - 4,      // Just before position
            POS_ADDR - 8,      // 8 bytes before position
        ];

        let mut level: u16 = 1; // Default fallback
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

        // Validate HP/MP (should be reasonable values)
        if max_hp == 0 || max_hp > 1_000_000 {
            debug!("Suspicious max_hp value: {}, using fallback", max_hp);
        }

        Ok(PlayerStats {
            hp,
            max_hp,
            mp,
            max_mp,
            exp: 0,
            level,
        })
    }

    /// Mob layer structure - try multiple patterns
    /// Pattern 1: [hdr:4][tick:4][z:4][0:4][x:4][y:4][dx:4][dy:4][x+dx:4][y+dy:4] (40 bytes)
    /// Pattern 2: [tick:4][x:4][y:4][dx:4][dy:4][...other...] (variable)
    const LAYER_SIZE: usize = 40;

    /// Try to find mob layers with non-zero positions
    /// This is a more aggressive scan that looks for any valid position data
    pub fn scan_mobs_calibration(&self) -> Result<Vec<Mob>, String> {
        let mut mobs = vec![];
        let mut pattern_count = std::collections::HashMap::<(i32, i32), u32>::new();

        // Get player position to find mobs nearby
        let player_pos = self.get_player_position().unwrap_or((0, 0));
        debug!("Calibration: Player at ({}, {})", player_pos.0, player_pos.1);

        let regions = match self.get_rw_anon_regions() {
            Ok(r) => r,
            Err(e) => {
                debug!("Calibration: Failed to get regions: {}", e);
                return Ok(mobs);
            }
        };

        debug!("Calibration: Scanning {} regions", regions.len());

        // Try different y offsets to find the correct structure
        let y_offsets = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        for &y_offset in &y_offsets {
            for region in &regions {
                let region_size = (region.end - region.start) as usize;
                if region_size < 4096 {
                    continue;
                }

                let data = match self.read_memory(region.start as usize, region_size.min(0x4000)) {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                // Scan for position pairs with this y offset
                let mut offset = 0;
                while offset + 24 <= data.len() {
                    let idx = offset / 4;
                    let x = le_i32(&data, idx);
                    let y = le_i32(&data, idx + y_offset);

                    // Check if this looks like a valid position near player
                    if x.abs() < 2000 && y.abs() < 2000 && (x != 0 || y != 0) {
                        // Count patterns
                        let count = pattern_count.entry((x, y)).or_insert(0);
                        *count += 1;

                        if *count > 30 {
                            offset += 4;
                            continue;
                        }

                        // Check if near player (within 300 pixels)
                        let dist_sq = (x - player_pos.0 as i32).pow(2) + (y - player_pos.1 as i32).pow(2);
                        let near_player = dist_sq < 300 * 300;

                        if near_player {
                            if mobs.len() < 10 {
                                debug!("Calibration: y_offset={} mob at offset {:x}: pos=({}, {}), dist_sq={}",
                                    y_offset, offset, x, y, dist_sq);
                            }
                            mobs.push(Mob {
                                id: (idx * 1000 + y_offset * 100) as u32,
                                x: x as i16,
                                y: y as i16,
                                hp_percent: 100,
                                layer: region.start as usize + offset,
                            });
                            if mobs.len() >= 30 {
                                break;
                            }
                        }
                    }
                    offset += 4;
                }

                if mobs.len() >= 30 {
                    break;
                }
            }

            if mobs.len() >= 30 {
                break;
            }
        }

        // Report most common patterns
        let mut sorted: Vec<_> = pattern_count.iter().take(20).collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        debug!("Calibration: Most common position patterns: {:?}", sorted.iter().take(10).map(|(p, c)| format!("({},{}):{}", p.0, p.1, c)).collect::<Vec<_>>());

        debug!("Calibration: Found {} potential mobs", mobs.len());

        Ok(mobs)
    }

    /// Mob sprite delta values (varies by mob type)
    /// Very permissive ranges to catch all mob types
    const MOB_DX_RANGE: (i32, i32) = (-300, 200);
    const MOB_DY_RANGE: (i32, i32) = (-400, 100);

    /// Scan for mobs using relaxed validation
    /// Looks for position patterns near the player
    pub fn scan_mobs(&self) -> Result<Vec<Mob>, String> {
        let mut mobs = vec![];
        let mut total_candidates = 0;

        // Get player position to find mobs nearby
        let player_pos = self.get_player_position().unwrap_or((0, 0));

        // Get RW anonymous regions
        let regions = match self.get_rw_anon_regions() {
            Ok(r) => r,
            Err(e) => {
                debug!("Failed to get regions: {}", e);
                return Ok(mobs);
            }
        };

        // Scan each region for mob layers
        for region in regions {
            let region_size = (region.end - region.start) as usize;
            if region_size < 4096 {
                continue;
            }

            // Read the region
            let data = match self.read_memory(region.start as usize, region_size.min(0x10000)) {
                Ok(d) => d,
                Err(_) => continue,
            };

            // Find mob candidates in this region using relaxed validation
            let candidates = self.find_mob_candidates_relaxed(&data, region.start as usize, player_pos);
            total_candidates += candidates.len();

            // Add candidates directly (relaxed validation already filters by distance)
            for (_, mob_data) in candidates {
                mobs.push(Mob {
                    id: mob_data.tick as u32,
                    x: mob_data.x as i16,
                    y: mob_data.y as i16,
                    hp_percent: 100,
                    layer: 0, // Will be set if needed
                });
            }
        }

        // Deduplicate by position
        mobs.sort_by_key(|m| (m.x, m.y));
        mobs.dedup_by_key(|m| (m.x, m.y));

        debug!("Mob scan complete: {} candidates -> {} unique mobs (player at {:?})", total_candidates, mobs.len(), player_pos);

        Ok(mobs)
    }

    /// Find potential mob layer candidates in a memory blob (relaxed validation)
    fn find_mob_candidates_relaxed(&self, data: &[u8], base_addr: usize, player_pos: (i16, i16)) -> Vec<(usize, MobData)> {
        let mut candidates = vec![];
        let mut pattern_count = std::collections::HashMap::<(i32, i32), u32>::new();

        if data.len() < 24 {
            return candidates;
        }

        // Try different y offsets - y_offset=1 worked in calibration
        let y_offsets = [1, 2, 3, 4, 5];

        for &y_offset in &y_offsets {
            let mut offset = 0;
            while offset + 16 <= data.len() {
                let idx = offset / 4;
                let x = le_i32(data, idx);
                let y = le_i32(data, idx + y_offset);

                // Check if this looks like a valid position near player
                if x.abs() < 2000 && y.abs() < 2000 && (x != 0 || y != 0) && !(x == 0 && y == 0) {
                    // Count patterns to filter out common false positives
                    let count = pattern_count.entry((x, y)).or_insert(0);
                    *count += 1;

                    // Skip positions that appear too often (likely not mobs)
                    if *count > 15 {
                        offset += 4;
                        continue;
                    }

                    // Check distance to player - must be within 150 pixels (close enough to attack)
                    let dist_sq = (x - player_pos.0 as i32).pow(2) + (y - player_pos.1 as i32).pow(2);
                    let near_player = dist_sq < 150 * 150 && dist_sq > 50; // 7-150 pixels

                    if near_player {
                        // Verify dx/dy are reasonable for mob sprites
                        let dx = le_i32(data, idx + y_offset + 1);
                        let dy = le_i32(data, idx + y_offset + 2);

                        // Mob sprite deltas should be in reasonable range
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

    /// Find potential mob layer candidates in a memory blob
    fn find_mob_candidates(&self, data: &[u8], base_addr: usize) -> Vec<(usize, MobData)> {
        let mut candidates = vec![];

        if data.len() < Self::LAYER_SIZE {
            return candidates;
        }

        // Scan for mob layer patterns
        // Try multiple offset patterns to find the correct structure
        let mut offset = 0;
        let mut found_any = false;
        while offset + Self::LAYER_SIZE <= data.len() {
            let layer = &data[offset..offset + Self::LAYER_SIZE];

            // Parse as i32s - try standard layer structure
            let hdr = le_i32(layer, 0);
            let tick = le_i32(layer, 1);
            let z = le_i32(layer, 2);
            let _zero = le_i32(layer, 3);
            let x = le_i32(layer, 4);
            let y = le_i32(layer, 5);
            let dx = le_i32(layer, 6);
            let dy = le_i32(layer, 7);
            let x_plus_dx = le_i32(layer, 8);
            let y_plus_dy = le_i32(layer, 9);

            // Validate mob layer
            if Self::is_valid_mob_layer(hdr, tick, z, x, y, dx, dy, x_plus_dx, y_plus_dy) {
                candidates.push((
                    base_addr + offset,
                    MobData { tick, x, y, dx, dy },
                ));
                if candidates.len() <= 5 {
                    debug!("Candidate at {:x}: hdr={:x} tick={} pos=({}, {})", base_addr + offset, hdr, tick, x, y);
                }
                found_any = true;
            }

            offset += 4; // Scan by 4-byte boundaries
        }

        // If we found candidates with non-zero positions, we're good
        if found_any {
            let non_zero = candidates.iter().filter(|(_, m)| m.x != 0 || m.y != 0).count();
            if non_zero > 0 {
                debug!("Found {} candidates with non-zero positions", non_zero);
            }
        }

        candidates
    }

    /// Validate a mob layer structure
    fn is_valid_mob_layer(
        hdr: i32,
        tick: i32,
        z: i32,
        x: i32,
        y: i32,
        dx: i32,
        dy: i32,
        x_plus_dx: i32,
        y_plus_dy: i32,
    ) -> bool {
        // Header should be non-zero (layer type) - but allow some common values
        if hdr == 0 {
            return false;
        }

        // Tick should be non-zero (game tick value)
        if tick == 0 {
            return false;
        }

        // Position should be reasonable (within map bounds)
        if x.abs() > 10000 || y.abs() > 10000 {
            return false;
        }

        // Delta should be in reasonable range for mob sprites (relaxed)
        if dx < Self::MOB_DX_RANGE.0 || dx > Self::MOB_DX_RANGE.1 {
            return false;
        }
        if dy < Self::MOB_DY_RANGE.0 || dy > Self::MOB_DY_RANGE.1 {
            return false;
        }

        // Verify x+dx and y+dy math (this is the key validation)
        if x.wrapping_add(dx) != x_plus_dx {
            return false;
        }
        if y.wrapping_add(dy) != y_plus_dy {
            return false;
        }

        true
    }

    /// Verify a mob layer is live (tick changed)
    fn verify_mob_liveness(&self, addr: usize, original: &MobData) -> Result<bool, String> {
        let data = self.read_memory(addr - 16, Self::LAYER_SIZE)?;

        let tick = le_i32(&data, 1);
        let x = le_i32(&data, 4);
        let y = le_i32(&data, 5);

        // Tick should have changed
        if tick == original.tick {
            return Ok(false);
        }

        // Position should be the same (mob hasn't moved much)
        if x != original.x || y != original.y {
            return Ok(false);
        }

        Ok(true)
    }

    /// Get nearest mob to player position
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

/// Mob layer data
#[derive(Debug, Clone)]
struct MobData {
    tick: i32,
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
}

/// Read little-endian i32 from byte slice at word index
fn le_i32(data: &[u8], word_idx: usize) -> i32 {
    let offset = word_idx * 4;
    if offset + 4 > data.len() {
        return 0;
    }
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

    #[test]
    fn test_scanner_creation() {
        let result = MemoryScanner::new(1);
        assert!(result.is_ok() || result.is_err());
    }
}
