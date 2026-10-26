//! Neon Inject - Native Linux shared object for Wine/MapleStory
//!
//! This .so is injected into the Wine process and reads player data.

use std::sync::atomic::{AtomicUsize, Ordering};

// Player data structure
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PlayerData {
    pub x: i16,
    pub y: i16,
    pub map_id: u32,
    pub level: u16,
    pub hp: u32,
    pub max_hp: u32,
    pub mp: u32,
    pub max_mp: u32,
}

// Global state
static MODULE_BASE: AtomicUsize = AtomicUsize::new(0);
static mut PLAYER_DATA: PlayerData = PlayerData {
    x: 0, y: 0, map_id: 0, level: 0, hp: 0, max_hp: 0, mp: 0, max_mp: 0,
};

// Known offsets from module base (need to be calibrated)
static mut OFFSET_POSITION: usize = 0x9248;
static mut OFFSET_MAP_ID: usize = 0x8B98;
static mut OFFSET_LEVEL: usize = 0x8458;
static mut OFFSET_HP: usize = 0x9258;
static mut OFFSET_MAX_HP: usize = 0x925C;
static mut OFFSET_MP: usize = 0x9260;
static mut OFFSET_MAX_MP: usize = 0x9264;

#[no_mangle]
pub extern "C" fn neon_init(base: usize) {
    unsafe {
        MODULE_BASE.store(base, Ordering::SeqCst);
        eprintln!("Neon Inject initialized at 0x{:X}", base);
    }
}

#[no_mangle]
pub extern "C" fn get_player_data() -> *const PlayerData {
    unsafe {
        update_player_data();
        &PLAYER_DATA as *const PlayerData
    }
}

unsafe fn update_player_data() {
    let base = MODULE_BASE.load(Ordering::SeqCst);
    if base == 0 {
        return;
    }

    // Read position (8 bytes: x, y as i32)
    let pos_addr = (base + OFFSET_POSITION) as *const i32;
    let x = *pos_addr as i16;
    let y = *pos_addr.offset(1) as i16;

    // Read map ID
    let map_addr = (base + OFFSET_MAP_ID) as *const u32;
    let map_id = *map_addr;

    // Read level (u16)
    let level_addr = (base + OFFSET_LEVEL) as *const u16;
    let level = *level_addr;

    // Read HP/MaxHP
    let hp_addr = (base + OFFSET_HP) as *const u32;
    let hp = *hp_addr;
    let max_hp = *(hp_addr.offset(1));

    // Read MP/MaxMP
    let mp_addr = (base + OFFSET_MP) as *const u32;
    let mp = *mp_addr;
    let max_mp = *(mp_addr.offset(1));

    PLAYER_DATA = PlayerData { x, y, map_id, level, hp, max_hp, mp, max_mp };
}

#[no_mangle]
pub extern "C" fn set_offsets(
    position: usize,
    map_id: usize,
    level: usize,
    hp: usize,
    max_hp: usize,
    mp: usize,
    max_mp: usize,
) {
    unsafe {
        OFFSET_POSITION = position;
        OFFSET_MAP_ID = map_id;
        OFFSET_LEVEL = level;
        OFFSET_HP = hp;
        OFFSET_MAX_HP = max_hp;
        OFFSET_MP = mp;
        OFFSET_MAX_MP = max_mp;
        eprintln!("Offsets updated");
    }
}

#[no_mangle]
pub extern "C" fn get_module_base() -> usize {
    MODULE_BASE.load(Ordering::SeqCst)
}
