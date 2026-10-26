//! Neon Inject - Native Linux reader for Wine/MapleStory
//!
//! Reads player data directly from /proc/pid/mem.

use clap::Parser;
use serde::Deserialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

#[derive(Parser, Debug)]
#[command(name = "neon-inject")]
#[command(about = "Read player data from MapleStory (Wine)")]
struct Args {
    /// PID of MapleStory process
    #[arg(long)]
    pid: u32,

    /// Enable debug logging
    #[arg(long, short)]
    verbose: bool,

    /// Stream data continuously
    #[arg(long)]
    stream: bool,

    /// Module base address (auto-detect if not provided)
    #[arg(long)]
    base: Option<String>,

    /// Config file path
    #[arg(long, default_value = "neon-offsets.json")]
    config: String,
}

#[derive(Deserialize, Debug, Clone)]
struct OffsetConfig {
    player: PlayerOffsets,
}

#[derive(Deserialize, Debug, Clone)]
struct PlayerOffsets {
    position: usize,
    map_id: usize,
    level: usize,
    hp: usize,
    max_hp: usize,
    mp: usize,
    max_mp: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct PlayerData {
    x: i16,
    y: i16,
    map_id: u32,
    level: u16,
    hp: u32,
    max_hp: u32,
    mp: u32,
    max_mp: u32,
}

fn main() {
    let args = Args::parse();

    if args.verbose {
        std::env::set_var("RUST_LOG", "debug");
    }
    tracing_subscriber::fmt::init();

    tracing::info!("Neon Inject - Memory Reader");
    tracing::info!("Target PID: {}", args.pid);

    // Load config
    let config = load_config(&args.config);
    tracing::info!("Loaded config from {}", args.config);

    // Find module base
    let base = args.base
        .map(|s| usize::from_str_radix(s.trim_start_matches("0x"), 16).unwrap())
        .unwrap_or_else(|| find_module_base(args.pid));

    tracing::info!("Module base: 0x{:X}", base);

    if args.stream {
        stream_data(args.pid, config);
    } else {
        // Read once
        let data = read_player_data(args.pid, &config);
        println!("Player data: {:?}", data);
    }
}

fn load_config(path: &str) -> OffsetConfig {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| "{\"player\":{\"position\":1086024,\"map_id\":1084312,\"level\":1081528,\"hp\":1085440,\"max_hp\":1085444,\"mp\":1085460,\"max_mp\":1085464}}".to_string());
    serde_json::from_str(&content).expect("Failed to parse config")
}

fn find_module_base(pid: u32) -> usize {
    let maps_path = format!("/proc/{}/maps", pid);
    let content = std::fs::read_to_string(&maps_path).expect("Failed to read maps");

    // Look for the main executable
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }

        let label = parts[5];
        if label.contains("MapleStory") || label.contains("wine") {
            let addr_range: Vec<&str> = parts[0].split('-').collect();
            if addr_range.len() == 2 {
                return u64::from_str_radix(addr_range[0], 16).unwrap() as usize;
            }
        }
    }

    // Fallback: use known Wine base
    0x400000
}

fn stream_data(pid: u32, config: OffsetConfig) {
    tracing::info!("Streaming player data (Ctrl+C to stop)...");

    let mut tick_count = 0;

    loop {
        let data = read_player_data(pid, &config);
        println!("Position: ({}, {}), Map: {}, Level: {}, HP: {}/{}, MP: {}/{}",
            data.x, data.y, data.map_id, data.level, data.hp, data.max_hp, data.mp, data.max_mp);

        // Randomized delay (500-800ms)
        let delay = 500 + (tick_count % 3) * 100;
        std::thread::sleep(std::time::Duration::from_millis(delay));
        tick_count += 1;
    }
}

fn read_player_data(pid: u32, config: &OffsetConfig) -> PlayerData {
    let mem_path = format!("/proc/{}/mem", pid);
    let mut file = File::open(&mem_path).expect("Failed to open /proc/pid/mem");

    // Read position at calibrated address
    let pos_addr = config.player.position;
    file.seek(SeekFrom::Start(pos_addr as u64)).unwrap();
    let mut pos_data = [0u8; 8];
    file.read_exact(&mut pos_data).unwrap();
    let x = i32::from_le_bytes([pos_data[0], pos_data[1], pos_data[2], pos_data[3]]) as i16;
    let y = i32::from_le_bytes([pos_data[4], pos_data[5], pos_data[6], pos_data[7]]) as i16;

    // Read map ID at calibrated address
    let map_addr = config.player.map_id;
    file.seek(SeekFrom::Start(map_addr as u64)).unwrap();
    let mut map_data = [0u8; 4];
    file.read_exact(&mut map_data).unwrap();
    let map_id = u32::from_le_bytes(map_data);

    // Read level at calibrated address
    let level_addr = config.player.level;
    file.seek(SeekFrom::Start(level_addr as u64)).unwrap();
    let mut level_data = [0u8; 2];
    file.read_exact(&mut level_data).unwrap();
    let level = u16::from_le_bytes(level_data);

    // Read HP/MaxHP at calibrated address (separate reads for each)
    let hp_addr = config.player.hp;
    file.seek(SeekFrom::Start(hp_addr as u64)).unwrap();
    let mut hp_data = [0u8; 4];
    file.read_exact(&mut hp_data).unwrap();
    let hp = u32::from_le_bytes(hp_data);

    let max_hp_addr = config.player.max_hp;
    file.seek(SeekFrom::Start(max_hp_addr as u64)).unwrap();
    let mut max_hp_data = [0u8; 4];
    file.read_exact(&mut max_hp_data).unwrap();
    let max_hp = u32::from_le_bytes(max_hp_data);

    // Read MP/MaxMP at calibrated address (separate reads for each)
    let mp_addr = config.player.mp;
    file.seek(SeekFrom::Start(mp_addr as u64)).unwrap();
    let mut mp_data = [0u8; 4];
    file.read_exact(&mut mp_data).unwrap();
    let mp = u32::from_le_bytes(mp_data);

    let max_mp_addr = config.player.max_mp;
    file.seek(SeekFrom::Start(max_mp_addr as u64)).unwrap();
    let mut max_mp_data = [0u8; 4];
    file.read_exact(&mut max_mp_data).unwrap();
    let max_mp = u32::from_le_bytes(max_mp_data);

    PlayerData { x, y, map_id, level, hp, max_hp, mp, max_mp }
}
