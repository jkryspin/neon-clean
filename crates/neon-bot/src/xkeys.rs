//! Key events through platform-native APIs, in-process.
//!
//! Linux: XTEST extension (same as xdotool keydown/keyup).
//! macOS: CoreGraphics CGEvent keyboard events.

#[cfg(target_os = "linux")]
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// X keysym / macOS virtual keycode for a key name as used by the keymap config.
/// On Linux this returns X11 keysyms; on macOS it returns CGKeyCode values.
#[cfg(target_os = "linux")]
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

#[cfg(target_os = "macos")]
pub fn keysym(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    Some(match lower.as_str() {
        "left" => 123,
        "up" => 126,
        "right" => 124,
        "down" => 125,
        "ctrl" | "control" | "control_l" => 59,
        "shift" | "shift_l" => 56,
        "alt" | "alt_l" | "option" => 58,
        "space" => 49,
        "return" | "enter" => 36,
        "escape" | "esc" => 53,
        "pause" | "break" => 113,
        "scroll_lock" => 107,
        "print" => 105,
        "tab" => 48,
        "home" => 115,
        "end" => 119,
        "insert" => 114, // Help key on Mac keyboards, mapped to Insert
        "delete" => 117, // Forward Delete
        "prior" | "pageup" | "page_up" => 116,
        "next" | "pagedown" | "page_down" => 121,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        "a" => 0,
        "b" => 11,
        "c" => 8,
        "d" => 2,
        "e" => 14,
        "f" => 3,
        "g" => 5,
        "h" => 4,
        "i" => 34,
        "j" => 38,
        "k" => 40,
        "l" => 37,
        "m" => 46,
        "n" => 45,
        "o" => 31,
        "p" => 35,
        "q" => 12,
        "r" => 15,
        "s" => 1,
        "t" => 17,
        "u" => 32,
        "v" => 9,
        "w" => 13,
        "x" => 7,
        "y" => 16,
        "z" => 6,
        "0" => 29,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "5" => 23,
        "6" => 22,
        "7" => 26,
        "8" => 28,
        "9" => 25,
        "`" => 50,
        "-" => 27,
        "=" => 24,
        "[" => 33,
        "]" => 30,
        "\\" => 42,
        ";" => 41,
        "'" => 39,
        "," => 43,
        "." => 47,
        "/" => 44,
        _ => return None,
    })
}

#[cfg(target_os = "windows")]
pub fn keysym(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    Some(match lower.as_str() {
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "ctrl" | "control" | "control_l" => 0x11,
        "shift" | "shift_l" => 0x10,
        "alt" | "alt_l" => 0x12,
        "space" => 0x20,
        "return" | "enter" => 0x0D,
        "escape" | "esc" => 0x1B,
        "pause" | "break" => 0x13,
        "scroll_lock" => 0x91,
        "print" => 0x2C,
        "tab" => 0x09,
        "home" => 0x24,
        "end" => 0x23,
        "insert" | "ins" => 0x2D,
        "delete" => 0x2E,
        "prior" | "pageup" | "page_up" => 0x21,
        "next" | "pagedown" | "page_down" => 0x22,
        s if s.len() >= 2 && s.starts_with('f') && s[1..].chars().all(|c| c.is_ascii_digit()) => {
            let n: u32 = s[1..].parse().ok()?;
            if !(1..=12).contains(&n) {
                return None;
            }
            0x70 + n - 1
        }
        s if s.chars().count() == 1 => {
            let c = s.chars().next()?.to_ascii_uppercase();
            if c.is_ascii_alphanumeric() {
                c as u32
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

// ---- Linux: XTEST ----

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        ConnectionExt, GrabMode, ModMask, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
    };
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::protocol::Event;
    use x11rb::rust_connection::RustConnection;

    pub struct XKeys {
        conn: RustConnection,
        root: u32,
        min_keycode: u8,
        codes: HashMap<u32, u8>,
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
                    codes.entry(sym).or_insert(min + i as u8);
                }
            }
            conn.xtest_get_version(2, 2)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| format!("XTEST missing: {e}"))?;
            Ok(Self {
                conn,
                root,
                min_keycode: min,
                codes,
            })
        }

        pub fn has(&self, key: &str) -> bool {
            super::keysym(key).map_or(false, |s| self.codes.contains_key(&s))
        }

        pub fn send(&self, key: &str, down: bool) -> Result<(), String> {
            let sym = super::keysym(key).ok_or_else(|| format!("unknown key name '{key}'"))?;
            let code = *self
                .codes
                .get(&sym)
                .ok_or_else(|| format!("no keycode for '{key}'"))?;
            debug_assert!(code >= self.min_keycode);
            let kind = if down {
                KEY_PRESS_EVENT
            } else {
                KEY_RELEASE_EVENT
            };
            self.conn
                .xtest_fake_input(kind, code, 0, self.root, 0, 0, 0)
                .map_err(|e| e.to_string())?;
            self.conn.flush().map_err(|e| e.to_string())
        }
    }

    pub fn spawn_panic_listener(key: &str) -> Option<Arc<AtomicBool>> {
        let Some(sym) = super::keysym(key) else {
            tracing::warn!(
                "panic key '{key}' is not a key name I know (try Pause, F12, End, Home)"
            );
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
                let map = conn
                    .get_keyboard_mapping(min, max - min + 1)
                    .map_err(|e| e.to_string())?
                    .reply()
                    .map_err(|e| e.to_string())?;
                let per = map.keysyms_per_keycode as usize;
                let code = map
                    .keysyms
                    .chunks(per)
                    .position(|c| c.contains(&sym))
                    .map(|i| min + i as u8)
                    .ok_or("no keycode for the panic key")?;
                conn.grab_key(
                    true,
                    root,
                    ModMask::ANY,
                    code,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )
                .map_err(|e| e.to_string())?
                .check()
                .map_err(|e| format!("grab failed: {e}"))?;
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
}

// ---- macOS: CoreGraphics ----

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use core_graphics::event::{CGEvent, CGEventTapLocation, CGKeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    extern "C" {
        fn CGEventSourceKeyState(stateID: u32, key: u16) -> bool;
    }

    pub struct XKeys {
        source: CGEventSource,
    }

    impl XKeys {
        pub fn connect() -> Result<Self, String> {
            let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
                .map_err(|_| "failed to create CGEventSource".to_string())?;
            tracing::info!("key input: in-process CoreGraphics events");
            Ok(Self { source })
        }

        pub fn has(&self, key: &str) -> bool {
            super::keysym(key).is_some()
        }

        pub fn send(&self, key: &str, down: bool) -> Result<(), String> {
            let code =
                super::keysym(key).ok_or_else(|| format!("unknown key name '{key}'"))? as CGKeyCode;
            let event = CGEvent::new_keyboard_event(self.source.clone(), code, down)
                .map_err(|_| format!("failed to create keyboard event for '{key}'"))?;
            event.post(CGEventTapLocation::HID);
            Ok(())
        }
    }

    pub fn spawn_panic_listener(key: &str) -> Option<Arc<AtomicBool>> {
        let Some(code) = super::keysym(key) else {
            tracing::warn!("panic key '{key}' is not a key name I know");
            return None;
        };
        let flag = Arc::new(AtomicBool::new(false));
        let f2 = flag.clone();
        let code = code as u16;
        // On macOS, we poll for the key using CGEventSource::key_state
        // rather than a global event tap (which requires accessibility permissions).
        std::thread::spawn(move || loop {
            if unsafe { CGEventSourceKeyState(CGEventSourceStateID::HIDSystemState as u32, code) } {
                f2.store(true, Ordering::SeqCst);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        });
        tracing::info!("panic key '{key}' polling active (CGEventSource::key_state)");
        Some(flag)
    }
}

// ---- Windows: SendInput ----

#[cfg(target_os = "windows")]
mod platform {
    use super::*;

    pub struct XKeys;

    impl XKeys {
        pub fn connect() -> Result<Self, String> {
            Ok(XKeys)
        }

        pub fn has(&self, key: &str) -> bool {
            keysym(key).is_some()
        }

        pub fn send(&self, key: &str, down: bool) -> Result<(), String> {
            let vk = keysym(key).ok_or_else(|| format!("unknown key '{key}'"))?;
            neon_win::input::key(vk as u16, down).map_err(|e| e.to_string())
        }
    }

    pub fn spawn_panic_listener(key: &str) -> Option<Arc<AtomicBool>> {
        let vk = keysym(key)?;
        let flag = Arc::new(AtomicBool::new(false));
        let out = flag.clone();
        std::thread::spawn(move || loop {
            if neon_win::input::is_down(vk as u16) {
                out.store(true, Ordering::SeqCst);
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        });
        Some(flag)
    }
}

pub use platform::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map_to_keycodes() {
        // These should return Some on both platforms
        assert!(keysym("Left").is_some());
        assert!(keysym("ctrl").is_some());
        assert!(keysym("alt").is_some());
        assert!(keysym("z").is_some());
        assert!(keysym("nonsense").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_keysyms() {
        assert_eq!(keysym("Left"), Some(0xff51));
        assert_eq!(keysym("ctrl"), Some(0xffe3));
        assert_eq!(keysym("alt"), Some(0xffe9));
        assert_eq!(keysym("F2"), Some(0xffbf));
        assert_eq!(keysym("z"), Some('z' as u32));
        assert_eq!(keysym("Pause"), Some(0xff13));
        assert_eq!(keysym("f13"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keycodes() {
        assert_eq!(keysym("Left"), Some(123));
        assert_eq!(keysym("ctrl"), Some(59));
        assert_eq!(keysym("alt"), Some(58));
        assert_eq!(keysym("z"), Some(6));
        assert_eq!(keysym("space"), Some(49));
        assert_eq!(keysym("return"), Some(36));
    }
}
