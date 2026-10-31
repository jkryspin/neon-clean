//! Supervised in-process bot session used by neon-ce.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;

use neon_state::GameState;

use crate::bot::Bot;
use crate::brain::{Brain, BrainConfig};
use crate::input::InputController;
use crate::status::StatusWriter;

pub struct Session {
    tx: Option<Sender<GameState>>,
    stop: Arc<AtomicBool>,
    panic: Option<Arc<AtomicBool>>,
    handle: Option<thread::JoinHandle<()>>,
    pub status_path: PathBuf,
    pub error: Arc<std::sync::Mutex<Option<String>>>,
}

impl Session {
    pub fn start(
        status_path: PathBuf,
        control_path: PathBuf,
        maps_dir: PathBuf,
        mobs: Vec<u32>,
        home_map: Option<u32>,
        panic_key: &str,
    ) -> Result<Session, String> {
        let mut keys = InputController::new()?;
        keys.find_window("MapleStory")?;
        let mut cfg = BrainConfig::default();
        cfg.allowed_mobs = (!mobs.is_empty()).then(|| mobs.into_iter().collect());
        cfg.home_map = home_map;
        let brain = Brain::new(cfg);
        let status = StatusWriter::new(status_path.clone(), maps_dir, false, None);
        let stop = Arc::new(AtomicBool::new(false));
        let panic = crate::xkeys::spawn_panic_listener(panic_key);
        let error = Arc::new(std::sync::Mutex::new(None));
        let mut bot = Bot::new(brain, keys, status)
            .with_control(control_path)
            .with_panic(panic.clone());
        let (tx, rx) = channel();
        let err = error.clone();
        let handle = thread::spawn(move || {
            if let Err(e) = bot.run(rx) {
                *err.lock().unwrap() = Some(e);
            }
        });
        Ok(Session {
            tx: Some(tx),
            stop,
            panic,
            handle: Some(handle),
            status_path,
            error,
        })
    }

    pub fn feed(&self, state: GameState) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(state);
        }
    }

    pub fn stop(&mut self) {
        if let Some(flag) = &self.panic {
            flag.store(true, Ordering::SeqCst);
        }
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.tx = None;
    }

    pub fn running(&self) -> bool {
        self.handle.is_some()
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}
