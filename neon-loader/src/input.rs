//! Input control for Wine/MapleStory via X11

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
    display: String,
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

    pub fn new() -> Result<Self, String> {
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
        let xauthority = std::env::var("XAUTHORITY")
            .unwrap_or_else(|_| "/run/user/1000/.mutter-Xwaylandauth.FGCHX3".to_string());

        // Persistent XTEST connection: key events in well under a millisecond.
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

    /// Find MapleStory window by title
    pub fn find_window(&mut self, title: &str) -> Result<(), String> {
        info!("Looking for window: {}", title);

        // Use xdotool to search for window
        let output = Command::new("xdotool")
            .args(["search", "--name", title])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .output()
            .map_err(|e| format!("xdotool search failed: {}", e))?;

        let window_ids = String::from_utf8_lossy(&output.stdout);

        if window_ids.trim().is_empty() {
            // Try partial match
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

        // Get first window ID
        let window_id = window_ids.lines().next().unwrap_or("").trim();
        if window_id.is_empty() {
            self.window_found = false;
            return Err("No window ID found".to_string());
        }

        info!("Found window ID: {}", window_id);

        // Get window geometry
        let geo_output = Command::new("xdotool")
            .args(["getwindowgeometry", "--window", window_id])
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .output()
            .map_err(|e| format!("xdotool getwindowgeometry failed: {}", e))?;

        let geo = String::from_utf8_lossy(&geo_output.stdout);
        // Format: "1280x720+100+200"
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

    /// Move mouse to screen coordinates (absolute)
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

    /// Move mouse to relative position within game window
    pub fn move_mouse_game(&self, x: i32, y: i32) -> Result<(), String> {
        let screen_x = self.window_pos.0 + x;
        let screen_y = self.window_pos.1 + y;
        self.move_mouse_screen(screen_x, screen_y)
    }

    /// Click left mouse button
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

    /// Double click
    pub fn double_click(&self) -> Result<(), String> {
        debug!("Double clicking");
        let status = Command::new("xdotool")
            .arg("click")
            .arg("double")
            .arg("1")
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool double click failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool double click failed with status: {:?}", status));
        }
        Ok(())
    }

    /// Right click
    pub fn right_click(&self) -> Result<(), String> {
        debug!("Right clicking");
        let status = Command::new("xdotool")
            .arg("click")
            .arg("3")
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool right click failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool right click failed with status: {:?}", status));
        }
        Ok(())
    }

    /// Press and release a key
    pub fn press_key(&self, key: &str) -> Result<(), String> {
        self.throttle();
        debug!("Pressing key: {}", key);
        let status = Command::new("xdotool")
            .arg("key")
            .arg(key)
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool key failed: {}", e))?;

        match status {
            _ if status.success() => Ok(()),
            _ => Err(format!("xdotool key '{}' failed with status: {:?}", key, status)),
        }
    }

    /// Click at a position given as fractions of the game window (0..1), e.g. a dialog button.
    /// Fractions survive resizing; the window is looked up fresh each time (rare, so no caching).
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

    /// Press and hold a key
    pub fn key_down(&self, key: &str) -> Result<(), String> {
        self.throttle();
        debug!("Key down: {}", key);
        if let Some(x) = self.xkeys.as_ref().filter(|x| x.has(key)) {
            return x.send(key, true);
        }
        let status = Command::new("xdotool")
            .arg("keydown")
            .arg(key)
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool keydown failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool keydown failed with status: {:?}", status));
        }
        Ok(())
    }

    /// Release a held key
    pub fn key_up(&self, key: &str) -> Result<(), String> {
        debug!("Key up: {}", key);
        if let Some(x) = self.xkeys.as_ref().filter(|x| x.has(key)) {
            return x.send(key, false);
        }
        let status = Command::new("xdotool")
            .arg("keyup")
            .arg(key)
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool keyup failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool keyup failed with status: {:?}", status));
        }
        Ok(())
    }

    /// Type text
    pub fn type_text(&self, text: &str) -> Result<(), String> {
        debug!("Typing: {}", text);
        let status = Command::new("xdotool")
            .arg("type")
            .arg("--clearmodifiers")
            .arg(text)
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .status()
            .map_err(|e| format!("xdotool type failed: {}", e))?;

        if !status.success() {
            return Err(format!("xdotool type failed with status: {:?}", status));
        }
        Ok(())
    }

    /// Press multiple keys simultaneously (e.g., "ctrl+enter")
    pub fn press_combo(&self, keys: &[&str]) -> Result<(), String> {
        debug!("Pressing combo: {:?}", keys);
        let joined = keys.join("+");
        self.press_key(&joined)
    }

    /// Move mouse in a curve (bezier-like) for more natural movement
    pub fn move_mouse_curve(&self, end_x: i32, end_y: i32, steps: usize) -> Result<(), String> {
        let start_x = self.window_pos.0 + self.window_size.0 / 2;
        let start_y = self.window_pos.1 + self.window_size.1 / 2;

        let dx = end_x - start_x;
        let _dy = end_y - start_y;

        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            // Simple quadratic curve with control point
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

    /// Get current mouse position
    pub fn get_mouse_pos(&self) -> Result<(i32, i32), String> {
        let output = Command::new("xdotool")
            .arg("getmouselocation")
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.xauthority)
            .output()
            .map_err(|e| format!("xdotool getmouselocation failed: {}", e))?;

        let output_str = String::from_utf8_lossy(&output.stdout);
        // Format: "x: 1000 y: 500 window: 0x123456 window_name: MapleStory"
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
        // never more than 5 reserved slots inside any 1 s span
        let mut c = RateCap::new(5);
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
