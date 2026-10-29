//! Input control for Wine/MapleStory.
//!
//! Linux: X11 XTEST + xdotool fallback.
//! macOS: CoreGraphics events + cliclick/osascript fallback.

use std::collections::VecDeque;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// At most `max` key presses/holds and clicks in any sliding second ("no packet spamming", docs/authorization.md).
/// Releases are never delayed: a key must always be able to come up.
pub struct RateCap {
    max: usize,
    window: Duration,
    times: VecDeque<Instant>,
}

impl RateCap {
    pub fn new(max: usize) -> Self {
        RateCap { max, window: Duration::from_secs(1), times: VecDeque::new() }
    }

    /// How long to wait before an event at `now` is allowed; the event is recorded as happening after that wait.
    pub fn reserve(&mut self, now: Instant) -> Duration {
        while self.times.front().map_or(false, |t| now.saturating_duration_since(*t) >= self.window) {
            self.times.pop_front();
        }
        let at = if self.times.len() >= self.max { self.times[self.times.len() - self.max] + self.window } else { now };
        let at = at.max(now);
        self.times.push_back(at);
        at - now
    }
}

pub struct InputController {
    xkeys: Option<crate::xkeys::XKeys>,
    window_found: bool,
    window_pos: (i32, i32),
    window_size: (i32, i32),
    #[cfg(target_os = "linux")]
    display: String,
    #[cfg(target_os = "linux")]
    xauthority: String,
    cap: Mutex<RateCap>,
}

impl InputController {
    fn throttle(&self) {
        let wait = self.cap.lock().map(|mut c| c.reserve(Instant::now())).unwrap_or_default();
        if !wait.is_zero() {
            debug!("input rate cap: waiting {:?}", wait);
            std::thread::sleep(wait);
        }
    }
}

// ---- Linux implementation ----

#[cfg(target_os = "linux")]
impl InputController {
    pub fn new() -> Result<Self, String> {
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
        let xauthority = std::env::var("XAUTHORITY")
            .unwrap_or_else(|_| "/run/user/1000/.mutter-Xwaylandauth.FGCHX3".to_string());

        std::env::set_var("DISPLAY", &display);
        std::env::set_var("XAUTHORITY", &xauthority);
        let xkeys = match crate::xkeys::XKeys::connect() {
            Ok(x) => {
                info!("key input: in-process XTEST");
                Some(x)
            }
            Err(e) => {
                tracing::warn!("{}; falling back to spawning xdotool per key event", e);
                None
            }
        };

        Ok(Self {
            xkeys,
            window_found: false,
            window_pos: (0, 0),
            window_size: (1280, 720),
            display,
            xauthority,
            cap: Mutex::new(RateCap::new(30)),
        })
    }

    pub fn find_window(&mut self, title: &str) -> Result<(), String> {
        info!("Looking for window: {}", title);

        let output = Command::new("xdotool")
            .args(["search", "--name", title])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .output()
            .map_err(|e| format!("xdotool search failed: {}", e))?;

        let window_ids = String::from_utf8_lossy(&output.stdout);

        if window_ids.trim().is_empty() {
            let output = Command::new("xdotool")
                .args(["search", "--name", "Maple"])
                .env("DISPLAY", &self.display)
                .env("XAUTHORITY", &self.xauthority)
                .output()
                .map_err(|e| format!("xdotool search failed: {}", e))?;

            let window_ids = String::from_utf8_lossy(&output.stdout);
            if window_ids.trim().is_empty() {
                self.window_found = false;
                return Err(format!("Window '{}' not found", title));
            }
        }

        let window_id = window_ids.lines().next().unwrap_or("").trim();
        if window_id.is_empty() {
            self.window_found = false;
            return Err("No window ID found".to_string());
        }

        info!("Found window ID: {}", window_id);

        let geo_output = Command::new("xdotool")
            .args(["getwindowgeometry", "--window", window_id])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .output()
            .map_err(|e| format!("xdotool getwindowgeometry failed: {}", e))?;

        let geo = String::from_utf8_lossy(&geo_output.stdout);
        if let Some(start) = geo.find('x') {
            let width: i32 = geo[..start].parse().unwrap_or(1280);
            let rest = &geo[start + 1..];
            if let Some(height_end) = rest.find('+') {
                let height: i32 = rest[..height_end].parse().unwrap_or(720);
                let pos = &rest[height_end + 1..];
                if let Some(x_end) = pos.find('+') {
                    let x: i32 = pos[..x_end].parse().unwrap_or(0);
                    let y: i32 = pos[x_end + 1..].trim().parse().unwrap_or(0);
                    self.window_pos = (x, y);
                    self.window_size = (width, height);
                }
            }
        }

        self.window_found = true;
        info!("Window geometry: {}x{} at ({}, {})",
            self.window_size.0, self.window_size.1, self.window_pos.0, self.window_pos.1);

        Ok(())
    }

    pub fn move_mouse_screen(&self, x: i32, y: i32) -> Result<(), String> {
        debug!("Moving mouse to screen ({}, {})", x, y);
        let status = Command::new("xdotool")
            .args(["mousemove", "--window", "root", &format!("{}", x), &format!("{}", y)])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool move failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool move failed with status: {:?}", status));
        }
        Ok(())
    }

    pub fn click(&self) -> Result<(), String> {
        debug!("Clicking mouse");
        let status = Command::new("xdotool")
            .arg("click")
            .arg("1")
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool click failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool click failed with status: {:?}", status));
        }
        Ok(())
    }

    pub fn double_click(&self) -> Result<(), String> {
        debug!("Double clicking");
        let status = Command::new("xdotool")
            .args(["click", "double", "1"])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool double click failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool double click failed with status: {:?}", status));
        }
        Ok(())
    }

    pub fn right_click(&self) -> Result<(), String> {
        debug!("Right clicking");
        let status = Command::new("xdotool")
            .args(["click", "3"])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool right click failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool right click failed with status: {:?}", status));
        }
        Ok(())
    }

    pub fn press_key(&self, key: &str) -> Result<(), String> {
        self.throttle();
        debug!("Pressing key: {}", key);
        let status = Command::new("xdotool")
            .args(["key", key])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool key failed: {}", e))?;

        match status {
            _ if status.success() => Ok(()),
            _ => Err(format!("xdotool key '{}' failed with status: {:?}", key, status)),
        }
    }

    pub fn click_fraction(&self, fx: f64, fy: f64) -> Result<(), String> {
        let run = |args: &[&str]| -> Result<String, String> {
            let out = Command::new("xdotool")
                .args(args)
                .env("DISPLAY", &self.display)
                .env("XAUTHORITY", &self.xauthority)
                .output()
                .map_err(|e| format!("xdotool {:?}: {}", args, e))?;
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        };
        let ids = run(&["search", "--name", "MapleStory"])?;
        let id = ids.lines().next().map(str::trim).filter(|s| !s.is_empty()).ok_or("game window not found")?.to_string();
        let geo = run(&["getwindowgeometry", "--shell", &id])?;
        let val = |k: &str| -> Option<f64> { geo.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).and_then(|v| v.trim().parse().ok()) };
        let (w, h) = (val("WIDTH").ok_or("no window width")?, val("HEIGHT").ok_or("no window height")?);
        let (x, y) = ((w * fx).round() as i32, (h * fy).round() as i32);
        run(&["mousemove", "--window", &id, &x.to_string(), &y.to_string()])?;
        std::thread::sleep(std::time::Duration::from_millis(200));
        run(&["click", "1"])?;
        Ok(())
    }

    pub fn key_down(&self, key: &str) -> Result<(), String> {
        self.throttle();
        debug!("Key down: {}", key);
        if let Some(x) = self.xkeys.as_ref().filter(|x| x.has(key)) {
            return x.send(key, true);
        }
        let status = Command::new("xdotool")
            .args(["keydown", key])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool keydown failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool keydown failed with status: {:?}", status));
        }
        Ok(())
    }

    pub fn key_up(&self, key: &str) -> Result<(), String> {
        debug!("Key up: {}", key);
        if let Some(x) = self.xkeys.as_ref().filter(|x| x.has(key)) {
            return x.send(key, false);
        }
        let status = Command::new("xdotool")
            .args(["keyup", key])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool keyup failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool keyup failed with status: {:?}", status));
        }
        Ok(())
    }

    pub fn type_text(&self, text: &str) -> Result<(), String> {
        debug!("Typing: {}", text);
        let status = Command::new("xdotool")
            .args(["type", "--clearmodifiers", text])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool type failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool type failed with status: {:?}", status));
        }
        Ok(())
    }

    pub fn press_combo(&self, keys: &[&str]) -> Result<(), String> {
        debug!("Pressing combo: {:?}", keys);
        let joined = keys.join("+");
        self.press_key(&joined)
    }

    pub fn get_mouse_pos(&self) -> Result<(i32, i32), String> {
        let output = Command::new("xdotool")
            .arg("getmouselocation")
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .output()
            .map_err(|e| format!("xdotool getmouselocation failed: {}", e))?;

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut x: i32 = 0;
        let mut y: i32 = 0;

        for part in output_str.split_whitespace() {
            if part.starts_with("x:") {
                if let Some(val) = part.strip_prefix("x:") {
                    x = val.trim().parse().unwrap_or(0);
                }
            } else if part.starts_with("y:") {
                if let Some(val) = part.strip_prefix("y:") {
                    y = val.trim().parse().unwrap_or(0);
                }
            }
        }

        Ok((x, y))
    }
}

// ---- macOS implementation ----

#[cfg(target_os = "macos")]
impl InputController {
    /// macOS title bar height in points (for coordinate mapping).
    const TITLE_BAR: i32 = 28;

    pub fn new() -> Result<Self, String> {
        let xkeys = match crate::xkeys::XKeys::connect() {
            Ok(x) => Some(x),
            Err(e) => {
                tracing::warn!("{e}; falling back to cliclick for key events");
                None
            }
        };

        Ok(Self {
            xkeys,
            window_found: false,
            window_pos: (0, 0),
            window_size: (1280, 720),
            cap: Mutex::new(RateCap::new(30)),
        })
    }

    pub fn find_window(&mut self, title: &str) -> Result<(), String> {
        info!("Looking for window: {}", title);

        // Use osascript to find the window via CGWindowListCopyWindowInfo
        let script = format!(
            r#"
            use framework "AppKit"
            set allWindows to current application's NSWorkspace's sharedWorkspace()'s runningApplications()
            set windowList to current application's CGWindowListCopyWindowInfo(current application's kCGWindowListOptionOnScreenOnly, 0)
            set output to ""
            repeat with w in windowList as list
                set wName to (w's kCGWindowName) as text
                if wName contains "{}" then
                    set wBounds to w's kCGWindowBounds
                    set wX to (wBounds's X) as integer
                    set wY to (wBounds's Y) as integer
                    set wW to (wBounds's Width) as integer
                    set wH to (wBounds's Height) as integer
                    set output to "" & wX & "," & wY & "," & wW & "," & wH
                    exit repeat
                end if
            end repeat
            return output
            "#,
            title.replace('"', "\\\"")
        );

        let output = Command::new("osascript").args(["-l", "AppleScript", "-e", &script])
            .output()
            .map_err(|e| format!("osascript failed: {e}"))?;

        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if result.is_empty() {
            // Fallback: try "MapleStory" instead of the full title
            return self.find_window_by_process();
        }

        let parts: Vec<i32> = result.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if parts.len() == 4 {
            self.window_pos = (parts[0], parts[1]);
            self.window_size = (parts[2], parts[3]);
            self.window_found = true;
            info!("Window geometry: {}x{} at ({}, {})",
                self.window_size.0, self.window_size.1, self.window_pos.0, self.window_pos.1);
            Ok(())
        } else {
            self.window_found = false;
            Err(format!("Window '{}' not found", title))
        }
    }

    fn find_window_by_process(&mut self) -> Result<(), String> {
        // Try finding by process name
        let output = Command::new("osascript")
            .args(["-e", r#"tell application "System Events" to get {position, size} of first window of (first process whose name contains "MapleStory")"#])
            .output()
            .map_err(|e| format!("osascript failed: {e}"))?;

        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let nums: Vec<i32> = result.split(|c: char| !c.is_ascii_digit() && c != '-')
            .filter_map(|s| s.parse().ok())
            .collect();

        if nums.len() >= 4 {
            self.window_pos = (nums[0], nums[1]);
            self.window_size = (nums[2], nums[3]);
            self.window_found = true;
            info!("Window geometry: {}x{} at ({}, {})",
                self.window_size.0, self.window_size.1, self.window_pos.0, self.window_pos.1);
            Ok(())
        } else {
            self.window_found = false;
            Err("MapleStory window not found".to_string())
        }
    }

    /// Move mouse to absolute screen coordinates using cliclick.
    pub fn move_mouse_screen(&self, x: i32, y: i32) -> Result<(), String> {
        debug!("Moving mouse to screen ({}, {})", x, y);
        let status = Command::new("cliclick")
            .arg(format!("m:{},{}", x, y))
            .status()
            .map_err(|e| format!("cliclick move failed: {}", e))?;

        if !status.success() {
            return Err(format!("cliclick move failed with status: {:?}", status));
        }
        Ok(())
    }

    /// Click left mouse button using mousedown+mouseup (works for Wine/CrossOver windows).
    pub fn click(&self) -> Result<(), String> {
        debug!("Clicking mouse");
        // Wine/CrossOver windows need mousedown+mouseup, not instant click
        let output = Command::new("cliclick")
            .arg("c:.")
            .output()
            .map_err(|e| format!("cliclick click failed: {e}"))?;
        if !output.status.success() {
            return Err("cliclick click failed".to_string());
        }
        Ok(())
    }

    pub fn double_click(&self) -> Result<(), String> {
        debug!("Double clicking");
        let status = Command::new("cliclick")
            .arg("dc:.")
            .status()
            .map_err(|e| format!("cliclick double-click failed: {e}"))?;
        if !status.success() {
            return Err("cliclick double-click failed".to_string());
        }
        Ok(())
    }

    pub fn right_click(&self) -> Result<(), String> {
        debug!("Right clicking");
        let status = Command::new("cliclick")
            .arg("rc:.")
            .status()
            .map_err(|e| format!("cliclick right-click failed: {e}"))?;
        if !status.success() {
            return Err("cliclick right-click failed".to_string());
        }
        Ok(())
    }

    pub fn press_key(&self, key: &str) -> Result<(), String> {
        self.throttle();
        debug!("Pressing key: {}", key);
        // Use the in-process CGEvent path: down then up
        if let Some(x) = self.xkeys.as_ref().filter(|x| x.has(key)) {
            x.send(key, true)?;
            std::thread::sleep(Duration::from_millis(30));
            return x.send(key, false);
        }
        // Fallback: use cliclick kp: (key press) with the macOS key name
        let cliclick_key = macos_key_name(key);
        let status = Command::new("cliclick")
            .arg(format!("kp:{}", cliclick_key))
            .status()
            .map_err(|e| format!("cliclick key failed: {}", e))?;
        if !status.success() {
            return Err(format!("cliclick key '{}' failed", key));
        }
        Ok(())
    }

    pub fn click_fraction(&self, fx: f64, fy: f64) -> Result<(), String> {
        if !self.window_found {
            return Err("window not found".to_string());
        }
        let x = self.window_pos.0 + (self.window_size.0 as f64 * fx).round() as i32;
        let y = self.window_pos.1 + (self.window_size.1 as f64 * fy).round() as i32;
        self.move_mouse_screen(x, y)?;
        std::thread::sleep(Duration::from_millis(200));
        // mousedown + mouseup for Wine windows
        let _ = Command::new("cliclick").arg(format!("dd:{},{}", x, y)).status();
        std::thread::sleep(Duration::from_millis(100));
        let _ = Command::new("cliclick").arg(format!("du:{},{}", x, y)).status();
        Ok(())
    }

    pub fn key_down(&self, key: &str) -> Result<(), String> {
        self.throttle();
        debug!("Key down: {}", key);
        if let Some(x) = self.xkeys.as_ref().filter(|x| x.has(key)) {
            return x.send(key, true);
        }
        let cliclick_key = macos_key_name(key);
        let status = Command::new("cliclick")
            .arg(format!("kd:{}", cliclick_key))
            .status()
            .map_err(|e| format!("cliclick keydown failed: {}", e))?;
        if !status.success() {
            return Err(format!("cliclick keydown failed for '{}'", key));
        }
        Ok(())
    }

    pub fn key_up(&self, key: &str) -> Result<(), String> {
        debug!("Key up: {}", key);
        if let Some(x) = self.xkeys.as_ref().filter(|x| x.has(key)) {
            return x.send(key, false);
        }
        let cliclick_key = macos_key_name(key);
        let status = Command::new("cliclick")
            .arg(format!("ku:{}", cliclick_key))
            .status()
            .map_err(|e| format!("cliclick keyup failed: {}", e))?;
        if !status.success() {
            return Err(format!("cliclick keyup failed for '{}'", key));
        }
        Ok(())
    }

    pub fn type_text(&self, text: &str) -> Result<(), String> {
        debug!("Typing: {}", text);
        let status = Command::new("cliclick")
            .arg(format!("t:{}", text))
            .status()
            .map_err(|e| format!("cliclick type failed: {}", e))?;
        if !status.success() {
            return Err(format!("cliclick type failed"));
        }
        Ok(())
    }

    pub fn press_combo(&self, keys: &[&str]) -> Result<(), String> {
        debug!("Pressing combo: {:?}", keys);
        // Hold all modifier keys, press the last key, release in reverse
        if keys.is_empty() {
            return Ok(());
        }
        for key in &keys[..keys.len() - 1] {
            self.key_down(key)?;
        }
        self.press_key(keys.last().unwrap())?;
        for key in keys[..keys.len() - 1].iter().rev() {
            self.key_up(key)?;
        }
        Ok(())
    }

    pub fn get_mouse_pos(&self) -> Result<(i32, i32), String> {
        let output = Command::new("cliclick")
            .arg("p")
            .output()
            .map_err(|e| format!("cliclick get position failed: {e}"))?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let parts: Vec<i32> = text.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if parts.len() == 2 {
            Ok((parts[0], parts[1]))
        } else {
            Ok((0, 0))
        }
    }
}

/// Map xdotool key names to cliclick key names.
#[cfg(target_os = "macos")]
fn macos_key_name(key: &str) -> &str {
    match key.to_ascii_lowercase().as_str() {
        "left" => "arrow-left",
        "right" => "arrow-right",
        "up" => "arrow-up",
        "down" => "arrow-down",
        "ctrl" | "control" | "control_l" => "ctrl",
        "shift" | "shift_l" => "shift",
        "alt" | "alt_l" => "alt",
        "return" | "enter" => "return",
        "escape" | "esc" => "esc",
        "space" => "space",
        "tab" => "tab",
        "delete" => "delete",
        "home" => "home",
        "end" => "end",
        "prior" | "pageup" | "page_up" => "page-up",
        "next" | "pagedown" | "page_down" => "page-down",
        "f1" => "f1", "f2" => "f2", "f3" => "f3", "f4" => "f4",
        "f5" => "f5", "f6" => "f6", "f7" => "f7", "f8" => "f8",
        "f9" => "f9", "f10" => "f10", "f11" => "f11", "f12" => "f12",
        _ => key,
    }
}

// ---- Shared methods ----

impl InputController {
    pub fn move_mouse_game(&self, x: i32, y: i32) -> Result<(), String> {
        let screen_x = self.window_pos.0 + x;
        let screen_y = self.window_pos.1 + y;
        self.move_mouse_screen(screen_x, screen_y)
    }

    pub fn move_mouse_curve(&self, end_x: i32, end_y: i32, steps: usize) -> Result<(), String> {
        let start_x = self.window_pos.0 + self.window_size.0 / 2;
        let start_y = self.window_pos.1 + self.window_size.1 / 2;

        let dx = end_x - start_x;

        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let ctrl_x = start_x + dx / 2 + (dx as f64 * (t - 0.5) * 50.0) as i32;
            let x = (1.0 - t).powi(2) * start_x as f64 +
                    2.0 * (1.0 - t) * t * ctrl_x as f64 +
                    t.powi(2) * end_x as f64;
            let y = (1.0 - t).powi(2) * start_y as f64 +
                    2.0 * (1.0 - t) * t * (ctrl_x + 50) as f64 +
                    t.powi(2) * end_y as f64;

            self.move_mouse_screen(x as i32, y as i32)?;
        }

        Ok(())
    }

    pub fn is_window_found(&self) -> bool {
        self.window_found
    }

    pub fn get_window_pos(&self) -> (i32, i32) {
        self.window_pos
    }

    pub fn get_window_size(&self) -> (i32, i32) {
        self.window_size
    }
}

#[cfg(test)]
mod rate_tests {
    use super::*;

    #[test]
    fn the_31st_event_within_a_second_waits_for_the_first_to_leave_the_window() {
        let mut c = RateCap::new(30);
        let t0 = Instant::now();
        for i in 0..30 {
            assert_eq!(c.reserve(t0 + Duration::from_millis(i)), Duration::ZERO, "event {i}");
        }
        let w = c.reserve(t0 + Duration::from_millis(30));
        assert_eq!(w, Duration::from_millis(970), "waits until t0 + 1 s");
    }

    #[test]
    fn a_burst_is_spread_out_and_a_quiet_second_resets_it() {
        let mut c = RateCap::new(5);
        let t0 = Instant::now();
        let mut slots = vec![];
        for _ in 0..40 {
            slots.push(c.reserve(t0));
        }
        for w in slots.windows(6) {
            assert!(w[5] - w[0] >= Duration::from_secs(1), "6 events inside a second: {w:?}");
        }
        assert_eq!(c.reserve(t0 + Duration::from_secs(60)), Duration::ZERO, "a long pause clears the history");
    }
}
