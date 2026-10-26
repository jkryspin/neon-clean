//! Key events through the XTEST extension, in-process.
//!
//! Spawning `xdotool` per key event costs 10-20 ms each and blocks the decision
//! loop; a persistent X connection is sub-millisecond. The events are the same
//! ones `xdotool keydown/keyup` generates (XTEST fake input to the focused window).

use std::collections::HashMap;
use x11rb::connection::Connection;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask, KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
use x11rb::protocol::Event;
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

pub struct XKeys {
    conn: RustConnection,
    root: u32,
    min_keycode: u8,
    /// keysym -> keycode
    codes: HashMap<u32, u8>,
}

/// X keysym for a key name as used by the keymap config ("ctrl", "alt", "Left", "F2", "z", ...).
pub fn keysym(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    Some(match lower.as_str() {
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        "ctrl" | "control" | "control_l" => 0xffe3,
        "shift" | "shift_l" => 0xffe1,
        "alt" | "alt_l" => 0xffe9,
        "space" => 0x20,
        "return" | "enter" => 0xff0d,
        "escape" | "esc" => 0xff1b,
        "pause" | "break" => 0xff13,
        "scroll_lock" => 0xff14,
        "print" => 0xff61,
        "tab" => 0xff09,
        "home" => 0xff50,
        "end" => 0xff57,
        "insert" => 0xff63,
        "delete" => 0xffff,
        "prior" | "pageup" | "page_up" => 0xff55,
        "next" | "pagedown" | "page_down" => 0xff56,
        s if s.len() >= 2 && s.starts_with('f') && s[1..].chars().all(|c| c.is_ascii_digit()) => {
            let n: u32 = s[1..].parse().ok()?;
            if !(1..=12).contains(&n) {
                return None;
            }
            0xffbe + n - 1
        }
        s if s.chars().count() == 1 => s.chars().next()? as u32,
        _ => return None,
    })
}

impl XKeys {
    pub fn connect() -> Result<Self, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| format!("X11 connect: {e}"))?;
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let root = setup.roots[screen].root;
        let map = conn
            .get_keyboard_mapping(min, max - min + 1)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let per = map.keysyms_per_keycode as usize;
        let mut codes = HashMap::new();
        for (i, chunk) in map.keysyms.chunks(per).enumerate() {
            for &sym in chunk {
                // Keep the first (lowest) keycode that produces a keysym.
                codes.entry(sym).or_insert(min + i as u8);
            }
        }
        conn.xtest_get_version(2, 2).map_err(|e| e.to_string())?.reply().map_err(|e| format!("XTEST missing: {e}"))?;
        Ok(Self { conn, root, min_keycode: min, codes })
    }

    pub fn has(&self, key: &str) -> bool {
        keysym(key).map_or(false, |s| self.codes.contains_key(&s))
    }

    pub fn send(&self, key: &str, down: bool) -> Result<(), String> {
        let sym = keysym(key).ok_or_else(|| format!("unknown key name '{key}'"))?;
        let code = *self.codes.get(&sym).ok_or_else(|| format!("no keycode for '{key}'"))?;
        debug_assert!(code >= self.min_keycode);
        let kind = if down { KEY_PRESS_EVENT } else { KEY_RELEASE_EVENT };
        self.conn.xtest_fake_input(kind, code, 0, self.root, 0, 0, 0).map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())
    }
}

/// Global "stop everything" hotkey: grabs `key` on the X root window and raises the returned flag when it
/// is pressed. Works whatever window has focus (under XWayland: whenever an X11 window has it).
pub fn spawn_panic_listener(key: &str) -> Option<Arc<AtomicBool>> {
    let Some(sym) = keysym(key) else {
        tracing::warn!("panic key '{key}' is not a key name I know (try Pause, F12, End, Home)");
        return None;
    };
    let flag = Arc::new(AtomicBool::new(false));
    let f2 = flag.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        let setup = (|| -> Result<(RustConnection, u8), String> {
            let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
            let root = conn.setup().roots[screen].root;
            let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
            let map = conn.get_keyboard_mapping(min, max - min + 1).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
            let per = map.keysyms_per_keycode as usize;
            let code = map.keysyms.chunks(per).position(|c| c.contains(&sym)).map(|i| min + i as u8).ok_or("no keycode for the panic key")?;
            conn.grab_key(true, root, ModMask::ANY, code, GrabMode::ASYNC, GrabMode::ASYNC).map_err(|e| e.to_string())?.check().map_err(|e| format!("grab failed: {e}"))?;
            conn.flush().map_err(|e| e.to_string())?;
            Ok((conn, code))
        })();
        match setup {
            Ok((conn, _)) => {
                let _ = tx.send(Ok(()));
                while let Ok(ev) = conn.wait_for_event() {
                    if matches!(ev, Event::KeyPress(_)) {
                        f2.store(true, Ordering::SeqCst);
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(Err(e));
            }
        }
    });
    match rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(Ok(())) => Some(flag),
        Ok(Err(e)) => {
            tracing::warn!("panic key '{key}' unavailable: {e}");
            None
        }
        Err(_) => {
            tracing::warn!("panic key '{key}': X server did not answer within 3 s");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map_to_keysyms() {
        assert_eq!(keysym("Left"), Some(0xff51));
        assert_eq!(keysym("ctrl"), Some(0xffe3));
        assert_eq!(keysym("alt"), Some(0xffe9));
        assert_eq!(keysym("F2"), Some(0xffbf));
        assert_eq!(keysym("z"), Some('z' as u32));
        assert_eq!(keysym("Pause"), Some(0xff13));
        assert_eq!(keysym("f13"), None);
        assert_eq!(keysym("nonsense"), None);
    }
}
