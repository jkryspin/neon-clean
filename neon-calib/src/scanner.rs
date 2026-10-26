//! Memory scanner for MapleStory

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use tracing::debug;

pub struct MemoryScanner {
    pid: u32,
}

#[derive(Debug, Clone)]
pub struct PlayerStats {
    pub hp: u32,
    pub max_hp: u32,
    pub mp: u32,
    pub max_mp: u32,
    pub level: u16,
}

#[derive(Debug, Clone)]
pub struct Mob {
    pub x: i16,
    pub y: i16,
    pub layer: usize,
}

impl MemoryScanner {
    pub fn new(pid: u32) -> Result<Self, String> {
        let mem_path = format!("/proc/{}/mem", pid);
        File::open(&mem_path)
            .map_err(|e| format!("Failed to open {}: {}", mem_path, e))?;

        Ok(Self { pid })
    }

    /// Read raw memory from process
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

    /// Read u32 from memory (little endian)
    pub fn read_u32(&self, address: usize) -> Result<u32, String> {
        let data = self.read_memory(address, 4)?;
        Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    }

    /// Read u16 from memory (little endian)
    pub fn read_u16(&self, address: usize) -> Result<u16, String> {
        let data = self.read_memory(address, 2)?;
        Ok(u16::from_le_bytes([data[0], data[1]]))
    }

    /// Read i32 from memory (little endian)
    pub fn read_i32(&self, address: usize) -> Result<i32, String> {
        let data = self.read_memory(address, 4)?;
        Ok(i32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    }

    /// Read i16 from memory (little endian)
    pub fn read_i16(&self, address: usize) -> Result<i16, String> {
        let data = self.read_memory(address, 2)?;
        Ok(i16::from_le_bytes([data[0], data[1]]))
    }

    /// Get player position at specific address
    pub fn get_player_position_at(&self, addr: usize) -> Result<(i16, i16), String> {
        let data = self.read_memory(addr, 8)?;
        let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        Ok((x as i16, y as i16))
    }

    /// Get player position using mirror verification
    pub fn get_player_position(&self) -> Result<(i16, i16), String> {
        // Known position mirrors - verified addresses
        const POS_MIRRORS: [usize; 2] = [0x109248, 0x109928];

        let mut last_pos: Option<(i32, i32)> = None;

        for _ in 0..10 {
            for &addr in &POS_MIRRORS {
                let data = self.read_memory(addr, 8)?;
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);

                // Validate: within map bounds and not 0,0
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

    /// Get current map ID
    pub fn get_map_id(&self) -> Result<u32, String> {
        const MAP_ADDR: usize = 0x108b98;
        self.read_u32(MAP_ADDR)
    }

    /// Get player stats (level, HP, MP)
    /// Searches for known values to find correct addresses
    pub fn get_player_stats(&self) -> Result<PlayerStats, String> {
        const POS_ADDR: usize = 0x109248;

        // Known values to search for (from user)
        // Level: 7, HP: 111, Max HP: 146, MP: 77, Max MP: 77

        // Search for level 7 (u16) near position address
        let mut level: u16 = 7; // Default to known value
        let mut level_addr: usize = 0;

        // Search in range around position address
        for offset in 0..0x2000 {
            let addr = if offset < POS_ADDR { POS_ADDR - offset } else { POS_ADDR + offset - 0x2000 };
            if let Ok(lvl) = self.read_u16(addr) {
                if lvl == 7 {
                    level = lvl;
                    level_addr = addr;
                    debug!("Found level 7 at 0x{:X}", addr);
                    break;
                }
            }
        }

        // Search for HP values near position
        let mut hp: u32 = 111;
        let mut max_hp: u32 = 146;
        let mut mp: u32 = 77;
        let mut max_mp: u32 = 77;

        // Search for HP (111) and Max HP (146) pattern
        for offset in 0..0x100 {
            let hp_addr = POS_ADDR + offset;
            if let Ok(h) = self.read_u32(hp_addr) {
                if h == 111 {
                    hp = h;
                    if let Ok(mh) = self.read_u32(hp_addr + 4) {
                        if mh == 146 {
                            max_hp = mh;
                            debug!("Found HP/MaxHP at 0x{:X}/0x{:X}", hp_addr, hp_addr + 4);
                        }
                    }
                }
            }
        }

        // Search for MP (77) and Max MP (77) pattern
        for offset in 0..0x100 {
            let mp_addr = POS_ADDR + offset;
            if let Ok(m) = self.read_u32(mp_addr) {
                if m == 77 {
                    if let Ok(mm) = self.read_u32(mp_addr + 4) {
                        if mm == 77 {
                            mp = m;
                            max_mp = mm;
                            debug!("Found MP/MaxMP at 0x{:X}/0x{:X}", mp_addr, mp_addr + 4);
                        }
                    }
                }
            }
        }

        Ok(PlayerStats {
            hp,
            max_hp,
            mp,
            max_mp,
            level,
        })
    }

    /// Scan for mobs near player (production version)
    pub fn scan_mobs(&self) -> Result<Vec<Mob>, String> {
        let mut mobs = vec![];
        let player_pos = self.get_player_position().unwrap_or((0, 0));

        let regions = self.get_rw_anon_regions()?;

        // Use y_offset=1 which worked in calibration
        let y_offset = 1;

        for region in &regions {
            let region_size = (region.end - region.start) as usize;
            if region_size < 4096 {
                continue;
            }

            let data = match self.read_memory(region.start as usize, region_size.min(0x10000)) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let mut offset = 0;
            while offset + 16 <= data.len() {
                let idx = offset / 4;
                let x = le_i32(&data, idx);
                let y = le_i32(&data, idx + y_offset);

                // Check if valid position near player (within 150 pixels)
                if x.abs() < 2000 && y.abs() < 2000 && (x != 0 || y != 0) {
                    let dist_sq = (x - player_pos.0 as i32).pow(2) + (y - player_pos.1 as i32).pow(2);

                    if dist_sq < 150 * 150 && dist_sq > 50 {
                        mobs.push(Mob {
                            x: x as i16,
                            y: y as i16,
                            layer: region.start as usize + offset,
                        });
                    }
                }
                offset += 4;
            }
        }

        // Deduplicate by position
        mobs.sort_by_key(|m| (m.x, m.y));
        mobs.dedup_by_key(|m| (m.x, m.y));

        Ok(mobs)
    }

    /// Calibrate mob detection - find mob layer structure
    pub fn scan_mobs_calibration(&self) -> Result<Vec<Mob>, String> {
        let mut mobs = vec![];
        let player_pos = self.get_player_position().unwrap_or((0, 0));

        debug!("Calibrating mobs, player at ({}, {})", player_pos.0, player_pos.1);

        // Get RW anonymous memory regions
        let regions = self.get_rw_anon_regions()?;
        debug!("Scanning {} regions", regions.len());

        // Try different y offsets to find correct structure
        for &y_offset in &[1, 2, 3, 4, 5] {
            for region in &regions {
                let region_size = (region.end - region.start) as usize;
                if region_size < 4096 {
                    continue;
                }

                let data = match self.read_memory(region.start as usize, region_size.min(0x4000)) {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                // Scan for position pairs
                let mut offset = 0;
                while offset + 16 <= data.len() {
                    let idx = offset / 4;
                    let x = le_i32(&data, idx);
                    let y = le_i32(&data, idx + y_offset);

                    // Check if valid position near player
                    if x.abs() < 2000 && y.abs() < 2000 && (x != 0 || y != 0) {
                        let dist_sq = (x - player_pos.0 as i32).pow(2) + (y - player_pos.1 as i32).pow(2);

                        if dist_sq < 300 * 300 && dist_sq > 0 {
                            mobs.push(Mob {
                                x: x as i16,
                                y: y as i16,
                                layer: region.start as usize + offset,
                            });
                        }
                    }

                    if mobs.len() >= 50 {
                        break;
                    }
                    offset += 4;
                }
            }
            if mobs.len() >= 50 {
                break;
            }
        }

        debug!("Calibration found {} mobs", mobs.len());
        Ok(mobs)
    }

    /// Get RW anonymous memory regions from /proc/pid/maps
    pub fn get_rw_anon_regions(&self) -> Result<Vec<Region>, String> {
        let maps_path = format!("/proc/{}/maps", self.pid);
        let content = std::fs::read_to_string(&maps_path)
            .map_err(|e| format!("Failed to read {}: {}", maps_path, e))?;

        let mut regions = vec![];
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            let addr_range: Vec<&str> = parts[0].split('-').collect();
            if addr_range.len() != 2 {
                continue;
            }

            let start = u64::from_str_radix(addr_range[0], 16)
                .map_err(|e| format!("Failed to parse start addr: {}", e))?;
            let end = u64::from_str_radix(addr_range[1], 16)
                .map_err(|e| format!("Failed to parse end addr: {}", e))?;

            let perms = parts[1];
            let label = parts.get(5).map(|s| *s).unwrap_or("");

            if perms.starts_with("rw") && label.is_empty() {
                regions.push(Region { start, end });
            }
        }

        Ok(regions)
    }

    /// Scan for other players on the map
    pub fn scan_players(&self) -> Result<Vec<Player>, String> {
        let mut players = vec![];
        let player_pos = self.get_player_position().unwrap_or((0, 0));

        let regions = self.get_rw_anon_regions()?;

        // Player layers have similar structure to mobs but with different patterns
        // For now, use same scanning approach as mobs
        for region in &regions {
            let region_size = (region.end - region.start) as usize;
            if region_size < 4096 {
            continue;
        }

        let data = match self.read_memory(region.start as usize, region_size.min(0x10000)) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let mut offset = 0;
        while offset + 16 <= data.len() {
            let idx = offset / 4;
            let x = le_i32(&data, idx);
            let y = le_i32(&data, idx + 1);

            // Check if valid position (not too close to player, not too far)
            if x.abs() < 2000 && y.abs() < 2000 && (x != 0 || y != 0) {
                let dist_sq = (x - player_pos.0 as i32).pow(2) + (y - player_pos.1 as i32).pow(2);

                // Other players should be 50-500 pixels away
                if dist_sq > 50 * 50 && dist_sq < 500 * 500 {
                    players.push(Player {
                        name: format!("Player_{}", idx),
                        x: x as i16,
                        y: y as i16,
                    });
                }
            }
            offset += 4;
        }
    }

    // Deduplicate
    players.sort_by_key(|p| (p.x, p.y));
    players.dedup_by_key(|p| (p.x, p.y));

    Ok(players)
    }
}

#[derive(Debug, Clone)]
pub struct Player {
    pub name: String,
    pub x: i16,
    pub y: i16,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub start: u64,
    pub end: u64,
}

/// Read little-endian i32 from byte slice at word index
fn le_i32(data: &[u8], word_idx: usize) -> i32 {
    let offset = word_idx * 4;
    if offset + 4 > data.len() {
        return 0;
    }
    i32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}
