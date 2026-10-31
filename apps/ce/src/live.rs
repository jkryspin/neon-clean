//! Background live reader: chains, footholds and contact rectangles.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use neon_capture::chains::Chains;
use neon_capture::rects::{self, RectTracker};
use neon_capture::scanner::MemoryScanner;
use neon_state::{Capabilities, Foothold, GameState, Mob, Player};
use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Contact {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Live {
    pub pid: u32,
    pub ts_ms: u64,
    pub hp: Option<u32>,
    pub max_hp: Option<u32>,
    pub mp: Option<u32>,
    pub level: Option<u16>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub map_id: Option<u32>,
    pub grounded: Option<bool>,
    pub camera: Option<(i32, i32)>,
    pub footholds: usize,
    pub contacts: Vec<Contact>,
    pub rects: usize,
    pub sweep_done: u64,
    pub error: Option<String>,
}

pub struct Reader {
    pub shared: Arc<RwLock<Live>>,
    pub stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    pub latest_state: Arc<Mutex<Option<GameState>>>,
}

impl Reader {
    pub fn start(pid: u32, data: PathBuf) -> Result<Reader, String> {
        let chains_text = std::fs::read_to_string(data.join("chains.json"))
            .map_err(|e| format!("chains.json: {e}"))?;
        let mut chains = Chains::from_json(&chains_text)?;
        let scanner = MemoryScanner::new(pid)?;
        let base = neon_win::process::Process::open(pid)
            .map_err(|e| e.to_string())?
            .main_base();
        chains.rebase(base);
        let shared = Arc::new(RwLock::new(Live {
            pid,
            ..Default::default()
        }));
        let latest_state = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let out = shared.clone();
        let stop2 = stop.clone();
        let state_out = latest_state.clone();
        let handle = thread::spawn(move || {
            let mut tracker: Option<RectTracker> = None;
            let mut seen = SeenMax::load(data.join("seen-max.json"));
            let mut seq = 0u64;
            while !stop2.load(Ordering::Relaxed) {
                let mut snap = Live {
                    pid,
                    ts_ms: now_ms(),
                    ..Default::default()
                };
                if !scanner.alive() {
                    snap.error = Some("process exited".into());
                    *out.write().unwrap() = snap;
                    break;
                }
                snap.hp = chains.stat(&scanner, "hp");
                snap.mp = chains.stat(&scanner, "mp");
                snap.level = chains.stat(&scanner, "level").map(|v| v as u16);
                let pos = chains
                    .position(&scanner)
                    .or_else(|| chains.position_f32_unchecked(&scanner));
                snap.x = pos.map(|p| p.0);
                snap.y = pos.map(|p| p.1);
                snap.map_id = chains.flag_stable(&scanner, "map_id");
                snap.grounded = chains.flag01(&scanner, "grounded");
                snap.camera = chains.camera(&scanner);

                let map = snap.map_id.and_then(|id| load_map(&data, id));
                let footholds = map.clone().unwrap_or_default();
                snap.footholds = footholds.len();

                if snap.hp.is_some() || snap.map_id.is_some() {
                    if tracker.is_none() {
                        if let Ok(regions) = scanner.rw_anon_regions() {
                            tracker = Some(RectTracker::new(regions));
                        }
                    }
                    if let Some(t) = tracker.as_mut() {
                        let found = t.tick(&scanner, 32 << 20);
                        snap.sweep_done = t.sweeps_completed;
                        snap.rects = found.len();
                        if let Some(p) = pos {
                            snap.contacts = rects::to_mobs(&found, p, &footholds)
                                .into_iter()
                                .map(|(x, y)| Contact { x, y })
                                .collect();
                        }
                    }
                }

                let mut state = GameState {
                    seq,
                    ts_ms: snap.ts_ms,
                    player: Player {
                        x: snap.x.unwrap_or(0),
                        y: snap.y.unwrap_or(0),
                        map_id: snap.map_id,
                        level: snap.level,
                        hp: snap.hp,
                        mp: snap.mp,
                        grounded: snap.grounded,
                        ..Default::default()
                    },
                    mobs: snap
                        .contacts
                        .iter()
                        .map(|c| Mob {
                            x: c.x,
                            y: c.y,
                            id: None,
                            hp_percent: None,
                            oid: None,
                            age_ms: Some(0),
                        })
                        .collect(),
                    footholds,
                    camera: snap.camera,
                    caps: Capabilities {
                        position: snap.x.is_some(),
                        map_id: snap.map_id.is_some(),
                        level: snap.level.is_some(),
                        hp_mp: snap.hp.is_some(),
                        mobs: !snap.contacts.is_empty(),
                        grounded: snap.grounded.is_some(),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                if let (Some(level), Some(hp)) = (snap.level, snap.hp) {
                    let max = seen.observe(level, hp);
                    state.player.max_hp = Some(max);
                    snap.max_hp = Some(max);
                    state.player.max_mp = snap.mp;
                    state.caps.max_estimated = true;
                }
                seq += 1;
                *state_out.lock().unwrap() = Some(state);
                *out.write().unwrap() = snap;
                thread::sleep(Duration::from_millis(250));
            }
        });
        Ok(Reader {
            shared,
            stop,
            handle: Some(handle),
            latest_state,
        })
    }

    pub fn snapshot(&self) -> Live {
        self.shared.read().unwrap().clone()
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop();
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn load_map(data: &Path, id: u32) -> Option<Vec<Foothold>> {
    let text = std::fs::read_to_string(data.join("maps").join(format!("{id}.json"))).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(
        v.get("footholds")?
            .as_array()?
            .iter()
            .filter_map(|f| {
                Some(Foothold {
                    x1: f.get("x1")?.as_i64()? as i32,
                    y1: f.get("y1")?.as_i64()? as i32,
                    x2: f.get("x2")?.as_i64()? as i32,
                    y2: f.get("y2")?.as_i64()? as i32,
                })
            })
            .collect(),
    )
}

struct SeenMax(BTreeMap<u16, u32>, PathBuf);

impl SeenMax {
    fn load(path: PathBuf) -> SeenMax {
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        SeenMax(map, path)
    }

    fn observe(&mut self, level: u16, hp: u32) -> u32 {
        let current = self.0.get(&level).copied().unwrap_or(0);
        if hp <= current {
            return current;
        }
        self.0.insert(level, hp);
        if let Some(dir) = self.1.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&self.0) {
            let _ = std::fs::write(&self.1, text);
        }
        hp
    }
}
