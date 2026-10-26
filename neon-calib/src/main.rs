//! Neon Calibration Tool
//!
//! Find memory addresses and save to config file for streaming.

mod scanner;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "neon-calib")]
#[command(about = "Neon MapleStory memory address calibrator")]
struct Args {
    /// PID of MapleStory process (auto-detect if not provided)
    #[arg(long)]
    pid: Option<u32>,

    /// Enable debug logging
    #[arg(long, short)]
    verbose: bool,

    /// Output config file path
    #[arg(long, default_value = "neon-calib.json")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Find all player data addresses
    FindPlayer {
        /// Known level value
        #[arg(long, default_value = "7")]
        level: u16,
        /// Known HP value
        #[arg(long, default_value = "111")]
        hp: u32,
        /// Known max HP value
        #[arg(long, default_value = "146")]
        max_hp: u32,
        /// Known MP value
        #[arg(long, default_value = "77")]
        mp: u32,
        /// Known max MP value
        #[arg(long, default_value = "77")]
        max_mp: u32,
    },

    /// Find mob layer structure
    FindMobs,

    /// Dump memory at address
    Dump {
        /// Hex address
        address: String,
        /// Bytes to dump
        #[arg(default_value = "256")]
        bytes: usize,
    },

    /// Search for value in memory
    Search {
        /// Value to search (hex or decimal)
        value: String,
        /// Type: u8, u16, u32
        #[arg(default_value = "u32")]
        r#type: String,
    },

    /// Verify current config works
    Verify,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CalibConfig {
    version: String,
    player: PlayerAddresses,
    mobs: MobConfig,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PlayerAddresses {
    position: usize,
    position_mirror: Option<usize>,
    map_id: usize,
    level: usize,
    hp: usize,
    max_hp: usize,
    mp: usize,
    max_mp: usize,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MobConfig {
    y_offset: usize,
    max_distance: i32,
    min_distance: i32,
}

fn main() {
    let args = Args::parse();

    if args.verbose {
        std::env::set_var("RUST_LOG", "debug");
    }
    tracing_subscriber::fmt::init();

    let pid = args.pid.unwrap_or_else(find_maplestory_pid);
    tracing::info!("Using PID: {}", pid);

    let scanner = scanner::MemoryScanner::new(pid).expect("Failed to create scanner");

    match args.command {
        Commands::FindPlayer { level, hp, max_hp, mp, max_mp } => {
            cmd_find_player(&scanner, level, hp, max_hp, mp, max_mp, &args.config);
        }
        Commands::FindMobs => {
            cmd_find_mobs(&scanner, &args.config);
        }
        Commands::Dump { address, bytes } => {
            cmd_dump(&scanner, &address, bytes);
        }
        Commands::Search { value, r#type: _type } => {
            cmd_search(&scanner, &value, &_type);
        }
        Commands::Verify => {
            cmd_verify(&scanner, &args.config);
        }
    }
}

fn find_maplestory_pid() -> u32 {
    let output = std::process::Command::new("pgrep")
        .args(["-f", "MapleStory.exe"])
        .output()
        .expect("Failed to run pgrep");
    let output = String::from_utf8_lossy(&output.stdout);
    for line in output.lines() {
        if let Ok(pid) = line.trim().parse() {
            return pid;
        }
    }
    eprintln!("MapleStory not found");
    std::process::exit(1);
}

fn cmd_find_player(scanner: &scanner::MemoryScanner, level: u16, hp: u32, max_hp: u32, mp: u32, max_mp: u32, config_path: &PathBuf) {
    tracing::info!("=== Finding Player Addresses ===");
    tracing::info!("Known values: level={}, hp={}, max_hp={}, mp={}, max_mp={}", level, hp, max_hp, mp, max_mp);

    // Known position address (verified working)
    let pos_addr = 0x109248;
    let pos_mirror = 0x109928;
    let map_addr = 0x108b98;

    tracing::info!("Known: position=0x{:X}, mirror=0x{:X}, map=0x{:X}", pos_addr, pos_mirror, map_addr);

    // Search for level near position
    let level_addr = search_value_u16(scanner, level, pos_addr, 0x2000);
    tracing::info!("Level {}: found at 0x{:X}", level, level_addr);

    // Search for HP/MaxHP pattern near position
    let (hp_addr, max_hp_addr) = search_pattern_u32_u32(scanner, hp, max_hp, pos_addr, 0x100);
    tracing::info!("HP {}: found at 0x{:X}", hp, hp_addr);
    tracing::info!("MaxHP {}: found at 0x{:X}", max_hp, max_hp_addr);

    // Search for MP/MaxMP pattern near position
    let (mp_addr, max_mp_addr) = search_pattern_u32_u32(scanner, mp, max_mp, pos_addr, 0x100);
    tracing::info!("MP {}: found at 0x{:X}", mp, mp_addr);
    tracing::info!("MaxMP {}: found at 0x{:X}", max_mp, max_mp_addr);

    // Create config
    let config = CalibConfig {
        version: "1.0".to_string(),
        player: PlayerAddresses {
            position: pos_addr,
            position_mirror: Some(pos_mirror),
            map_id: map_addr,
            level: level_addr,
            hp: hp_addr,
            max_hp: max_hp_addr,
            mp: mp_addr,
            max_mp: max_mp_addr,
        },
        mobs: MobConfig {
            y_offset: 1,
            max_distance: 150,
            min_distance: 7,
        },
    };

    // Save config
    let json = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(config_path, &json).expect("Failed to write config");
    tracing::info!("Config saved to {:?}", config_path);
    tracing::info!("\nConfig:\n{}", json);
}

fn cmd_find_mobs(scanner: &scanner::MemoryScanner, config_path: &PathBuf) {
    tracing::info!("=== Finding Mob Structure ===");

    let mobs = scanner.scan_mobs_calibration().expect("Failed to scan mobs");
    tracing::info!("Found {} mobs", mobs.len());

    // Show first 10
    for (i, mob) in mobs.iter().take(10).enumerate() {
        tracing::info!("  [{}] pos=({},{}) layer=0x{:X}", i, mob.x, mob.y, mob.layer);
    }

    // Update config with mob settings
    if let Ok(existing) = std::fs::read_to_string(config_path) {
        let mut config: CalibConfig = serde_json::from_str(&existing).unwrap();
        config.mobs.y_offset = 1; // y_offset=1 worked
        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(config_path, &json).expect("Failed to write config");
        tracing::info!("Updated config with mob settings");
    }
}

fn cmd_dump(scanner: &scanner::MemoryScanner, address: &str, bytes: usize) {
    let addr = usize::from_str_radix(address.trim_start_matches("0x"), 16)
        .expect("Invalid hex address");

    tracing::info!("=== Dump at 0x{:X} ===", addr);

    if let Ok(data) = scanner.read_memory(addr, bytes) {
        for (i, chunk) in data.chunks(16).enumerate() {
            let offset = addr + i * 16;
            print!("0x{:08X}: ", offset);
            for &byte in chunk {
                print!("{:02X} ", byte);
            }
            print!(" | ");
            for &byte in chunk {
                print!("{}", if byte.is_ascii_graphic() { byte as char } else { '.' });
            }
            println!();
        }
    }
}

fn cmd_search(scanner: &scanner::MemoryScanner, value: &str, type_str: &str) {
    tracing::info!("=== Searching for {} (type: {}) ===", value, type_str);

    let regions = scanner.get_rw_anon_regions().expect("Failed to get regions");
    tracing::info!("Scanning {} regions", regions.len());

    match type_str {
        "u8" => {
            let val: u8 = value.parse().unwrap_or(0);
            search_u8(scanner, val, &regions);
        }
        "u16" => {
            let val: u16 = parse_u16(value);
            search_u16(scanner, val, &regions);
        }
        "u32" => {
            let val: u32 = parse_u32(value);
            search_u32(scanner, val, &regions);
        }
        _ => {
            tracing::error!("Unknown type: {}", type_str);
        }
    }
}

fn parse_u16(value: &str) -> u16 {
    if value.starts_with("0x") || value.starts_with("0X") {
        u64::from_str_radix(value.trim_start_matches("0x"), 16).unwrap() as u16
    } else {
        value.parse().unwrap_or(0)
    }
}

fn parse_u32(value: &str) -> u32 {
    if value.starts_with("0x") || value.starts_with("0X") {
        u64::from_str_radix(value.trim_start_matches("0x"), 16).unwrap() as u32
    } else {
        value.parse().unwrap_or(0)
    }
}

fn search_u8(scanner: &scanner::MemoryScanner, val: u8, regions: &[scanner::Region]) {
    let mut count = 0;
    for region in regions {
        let size = (region.end - region.start) as usize;
        if size > 0x10000 {
            continue;
        }
        if let Ok(data) = scanner.read_memory(region.start as usize, size) {
            for (i, &byte) in data.iter().enumerate() {
                if byte == val {
                    if count < 20 {
                        tracing::info!("Found at 0x{:X}", region.start as usize + i);
                    }
                    count += 1;
                }
            }
        }
    }
    tracing::info!("Total: {} occurrences", count);
}

fn search_u16(scanner: &scanner::MemoryScanner, val: u16, regions: &[scanner::Region]) {
    let mut count = 0;
    for region in regions {
        let size = (region.end - region.start) as usize;
        if size > 0x10000 {
            continue;
        }
        if let Ok(data) = scanner.read_memory(region.start as usize, size) {
            for i in (0..data.len()).step_by(4) {
                if i + 2 > data.len() {
                    break;
                }
                let v = u16::from_le_bytes([data[i], data[i + 1]]);
                if v == val {
                    if count < 20 {
                        tracing::info!("Found at 0x{:X}", region.start as usize + i);
                    }
                    count += 1;
                }
            }
        }
    }
    tracing::info!("Total: {} occurrences", count);
}

fn search_u32(scanner: &scanner::MemoryScanner, val: u32, regions: &[scanner::Region]) {
    let mut count = 0;
    for region in regions {
        let size = (region.end - region.start) as usize;
        if size > 0x10000 {
            continue;
        }
        if let Ok(data) = scanner.read_memory(region.start as usize, size) {
            for i in (0..data.len()).step_by(4) {
                if i + 4 > data.len() {
                    break;
                }
                let v = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
                if v == val {
                    if count < 20 {
                        tracing::info!("Found at 0x{:X}", region.start as usize + i);
                    }
                    count += 1;
                }
            }
        }
    }
    tracing::info!("Total: {} occurrences", count);
}

fn cmd_verify(scanner: &scanner::MemoryScanner, config_path: &PathBuf) {
    tracing::info!("=== Verifying Config ===");

    let config: CalibConfig = match std::fs::read_to_string(config_path) {
        Ok(content) => serde_json::from_str(&content).expect("Invalid config"),
        Err(e) => {
            tracing::error!("Config not found: {}", e);
            std::process::exit(1);
        }
    };

    // Verify position
    let pos = scanner.get_player_position_at(config.player.position).ok();
    tracing::info!("Position at 0x{:X}: {:?}", config.player.position, pos);

    // Verify map
    let map = scanner.read_u32(config.player.map_id).ok();
    tracing::info!("Map ID at 0x{:X}: {:?}", config.player.map_id, map);

    // Verify level
    let level = scanner.read_u16(config.player.level).ok();
    tracing::info!("Level at 0x{:X}: {:?}", config.player.level, level);

    // Verify HP/MP
    let hp = scanner.read_u32(config.player.hp).ok();
    let max_hp = scanner.read_u32(config.player.max_hp).ok();
    let mp = scanner.read_u32(config.player.mp).ok();
    let max_mp = scanner.read_u32(config.player.max_mp).ok();

    tracing::info!("HP: {:?}/{:?}", hp, max_hp);
    tracing::info!("MP: {:?}/{:?}", mp, max_mp);
}

fn search_value_u16(scanner: &scanner::MemoryScanner, value: u16, base: usize, range: usize) -> usize {
    for offset in 0..range {
        let addr = if offset < base { base - offset } else { base + offset - range };
        if let Ok(val) = scanner.read_u16(addr) {
            if val == value {
                return addr;
            }
        }
    }
    base // Return base if not found
}

fn search_pattern_u32_u32(scanner: &scanner::MemoryScanner, val1: u32, val2: u32, base: usize, range: usize) -> (usize, usize) {
    for offset in 0..range {
        let addr = base + offset;
        if let Ok(v1) = scanner.read_u32(addr) {
            if v1 == val1 {
                if let Ok(v2) = scanner.read_u32(addr + 4) {
                    if v2 == val2 {
                        return (addr, addr + 4);
                    }
                }
            }
        }
    }
    (base, base + 4)
}
