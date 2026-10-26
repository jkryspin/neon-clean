//! Neon Calibrate - Find memory addresses in MapleStory
//!
//! Searches for known values to find correct memory addresses.

use clap::Parser;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

#[derive(Parser, Debug)]
#[command(name = "neon-calibrate")]
#[command(about = "Calibrate memory addresses for MapleStory")]
struct Args {
    /// PID of MapleStory process
    #[arg(long)]
    pid: u32,

    /// Known level value
    #[arg(long, default_value = "7")]
    level: u16,

    /// Known HP value
    #[arg(long, default_value = "99")]
    hp: u32,

    /// Known Max HP value
    #[arg(long, default_value = "146")]
    max_hp: u32,

    /// Known MP value
    #[arg(long, default_value = "77")]
    mp: u32,

    /// Known Max MP value
    #[arg(long, default_value = "77")]
    max_mp: u32,

    /// Enable debug logging
    #[arg(long, short)]
    verbose: bool,

    /// Output config file
    #[arg(long, default_value = "neon-offsets.json")]
    output: String,
}

#[derive(serde::Serialize)]
struct OffsetConfig {
    version: String,
    player: PlayerOffsets,
    mobs: MobConfig,
}

#[derive(serde::Serialize)]
struct PlayerOffsets {
    position: usize,
    map_id: usize,
    level: usize,
    hp: usize,
    max_hp: usize,
    mp: usize,
    max_mp: usize,
}

#[derive(serde::Serialize)]
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

    tracing::info!("Neon Calibrate");
    tracing::info!("Target PID: {}", args.pid);
    tracing::info!("Known values: level={}, hp={}, max_hp={}, mp={}, max_mp={}",
        args.level, args.hp, args.max_hp, args.mp, args.max_mp);

    // Open /proc/pid/mem
    let mem_path = format!("/proc/{}/mem", args.pid);
    let mut file = File::open(&mem_path).expect("Failed to open /proc/pid/mem");

    // Search for level (u16) near known addresses
    tracing::info!("Searching for level {}...", args.level);
    let level_addr = search_u16(&mut file, args.level, 0x108000, 0x2000);
    tracing::info!("Level found at: 0x{:X}", level_addr);

    // Search for HP/MaxHP pattern
    tracing::info!("Searching for HP {}/{}...", args.hp, args.max_hp);
    let (hp_addr, max_hp_addr) = search_u32_pattern(&mut file, args.hp, args.max_hp, 0x109000, 0x4000);
    tracing::info!("HP found at: 0x{:X}, MaxHP at: 0x{:X}", hp_addr, max_hp_addr);

    // Search for MP/MaxMP pattern (start after HP to avoid same address)
    tracing::info!("Searching for MP {}/{}...", args.mp, args.max_mp);
    let (mp_addr, max_mp_addr) = search_u32_pattern(&mut file, args.mp, args.max_mp, max_hp_addr + 16, 0x4000);
    tracing::info!("MP found at: 0x{:X}, MaxMP at: 0x{:X}", mp_addr, max_mp_addr);

    // Search for position and map_id as well
    tracing::info!("Searching for position and map_id...");
    // Position is typically near HP data
    let position_addr = hp_addr - 0x18;  // Approximate offset
    let map_id_addr = hp_addr - 0x658;   // Approximate offset

    // Create config
    let config = OffsetConfig {
        version: "1.0".to_string(),
        player: PlayerOffsets {
            position: position_addr,
            map_id: map_id_addr,
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
    std::fs::write(&args.output, &json).expect("Failed to write config");
    tracing::info!("Config saved to {}", args.output);
    tracing::info!("\n{}", json);
}

fn search_u16(file: &mut File, value: u16, start: usize, range: usize) -> usize {
    for offset in (0..range).step_by(2) {
        let addr = start + offset;
        file.seek(SeekFrom::Start(addr as u64)).unwrap();
        let mut data = [0u8; 2];
        file.read_exact(&mut data).unwrap();
        let val = u16::from_le_bytes(data);
        if val == value {
            return addr;
        }
    }
    start // Return start if not found
}

fn search_u32_pattern(file: &mut File, val1: u32, val2: u32, start: usize, range: usize) -> (usize, usize) {
    for offset in (0..range).step_by(4) {
        let addr = start + offset;
        file.seek(SeekFrom::Start(addr as u64)).unwrap();
        let mut data = [0u8; 8];
        file.read_exact(&mut data).unwrap();
        let v1 = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let v2 = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if v1 == val1 && v2 == val2 {
            return (addr, addr + 4);
        }
    }
    (start, start + 4) // Return start if not found
}
