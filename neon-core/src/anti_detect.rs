//! Anti-detection module: randomized timing, human-like behavior, pattern breaking

use rand::Rng;
use rand::SeedableRng;
use std::time::{Duration, Instant};

/// Configuration for anti-detection behavior
#[derive(Debug, Clone)]
pub struct AntiDetectConfig {
    /// Base timing jitter (0.0-1.0, higher = more variance)
    pub timing_jitter: f64,
    /// Minimum idle period in seconds
    pub min_idle_seconds: u64,
    /// Maximum idle period in seconds
    pub max_idle_seconds: u64,
    /// How often to take idle breaks (in actions)
    pub idle_frequency: u32,
    /// Chance to make a "mistake" (0.0-1.0)
    pub mistake_chance: f64,
    /// Maximum session duration in hours
    pub max_session_hours: u64,
}

impl Default for AntiDetectConfig {
    fn default() -> Self {
        Self {
            timing_jitter: 0.25,
            min_idle_seconds: 30,
            max_idle_seconds: 180,
            idle_frequency: 50,
            mistake_chance: 0.05,
            max_session_hours: 5,
        }
    }
}

/// State tracking for anti-detection
#[derive(Debug)]
pub struct AntiDetectState {
    pub actions_since_idle: u32,
    pub session_start: Instant,
    pub last_action: Instant,
    pub is_idling: bool,
    pub idle_end_time: Option<Instant>,
}

impl Default for AntiDetectState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            actions_since_idle: 0,
            session_start: now,
            last_action: now,
            is_idling: false,
            idle_end_time: None,
        }
    }
}

pub struct AntiDetect {
    config: AntiDetectConfig,
    state: AntiDetectState,
    rng: rand::rngs::StdRng,
}

impl AntiDetect {
    pub fn new() -> Self {
        Self::with_config(AntiDetectConfig::default())
    }

    pub fn with_config(config: AntiDetectConfig) -> Self {
        let rng = rand::rngs::StdRng::from_entropy();
        Self {
            config,
            state: AntiDetectState::default(),
            rng,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new()
    }

    /// Get a jittered delay based on base duration
    pub fn jittered_delay(&mut self, base_ms: u64) -> Duration {
        let jitter = self.config.timing_jitter;
        let factor = 1.0 - jitter + self.rng.gen_range(0.0..=jitter * 2.0);
        let ms = (base_ms as f64 * factor).max(10.0) as u64;
        Duration::from_millis(ms)
    }

    /// Get a jittered Duration
    pub fn jittered_duration(&mut self, base: Duration) -> Duration {
        self.jittered_delay(base.as_millis() as u64)
    }

    /// Get a human-like click delay (100-300ms with jitter)
    pub fn human_click_delay(&mut self) -> Duration {
        let base = 100 + self.rng.gen_range(0..=200);
        self.jittered_delay(base)
    }

    /// Get a human-like key hold duration (50-150ms)
    pub fn human_key_hold(&mut self) -> Duration {
        let base = 50 + self.rng.gen_range(0..=100);
        self.jittered_delay(base)
    }

    /// Get a human-like reaction time (150-350ms)
    pub fn human_reaction_time(&mut self) -> Duration {
        let base = 150 + self.rng.gen_range(0..=200);
        self.jittered_delay(base)
    }

    /// Check if we should take an idle break
    pub fn should_idle(&self) -> bool {
        if self.state.is_idling {
            return false;
        }

        self.state.actions_since_idle >= self.config.idle_frequency
    }

    /// Start an idle break
    pub fn start_idle(&mut self) -> Duration {
        let duration_secs = self.rng.gen_range(
            self.config.min_idle_seconds..=self.config.max_idle_seconds
        );
        let duration = Duration::from_secs(duration_secs);
        self.state.is_idling = true;
        self.state.idle_end_time = Some(Instant::now() + duration);
        self.state.actions_since_idle = 0;
        duration
    }

    /// Check if idle period is over
    pub fn idle_complete(&mut self) -> bool {
        if !self.state.is_idling {
            return true;
        }

        if let Some(end_time) = self.state.idle_end_time {
            if Instant::now() >= end_time {
                self.state.is_idling = false;
                self.state.idle_end_time = None;
                return true;
            }
        }
        false
    }

    /// Record an action and update state
    pub fn record_action(&mut self) {
        self.state.last_action = Instant::now();
        self.state.actions_since_idle += 1;
    }

    /// Check if we should make a "mistake" this action
    pub fn should_mistake(&mut self) -> bool {
        self.rng.gen_range(0.0..1.0) < self.config.mistake_chance
    }

    /// Get session duration
    pub fn session_duration(&self) -> Duration {
        self.state.session_start.elapsed()
    }

    /// Check if session should end
    pub fn session_should_end(&self) -> bool {
        self.session_duration().as_secs() >= self.config.max_session_hours * 3600
    }

    /// Reset for new session
    pub fn new_session(&mut self) {
        self.state = AntiDetectState {
            session_start: Instant::now(),
            last_action: Instant::now(),
            ..Default::default()
        };
    }

    /// Get mouse movement path with slight curve (bezier-like)
    pub fn mouse_path(&mut self, start: (i32, i32), end: (i32, i32)) -> Vec<(i32, i32)> {
        let mut path = vec![start];
        let dx = end.0 - start.0;
        let dy = end.1 - start.1;
        let steps = (dx.abs().max(dy.abs()) / 10).max(3) as usize;

        // Add slight curve using a control point offset
        let curve_x = self.rng.gen_range(-10..=10);
        let curve_y = self.rng.gen_range(-10..=10);

        for i in 1..steps {
            let t = i as f64 / steps as f64;
            // Quadratic bezier with random control point
            let bx = (1.0 - t).powi(2) * start.0 as f64
                + 2.0 * (1.0 - t) * t * (start.0 as f64 + dx as f64 * 0.5 + curve_x as f64)
                + t.powi(2) * end.0 as f64;
            let by = (1.0 - t).powi(2) * start.1 as f64
                + 2.0 * (1.0 - t) * t * (start.1 as f64 + dy as f64 * 0.5 + curve_y as f64)
                + t.powi(2) * end.1 as f64;

            path.push((bx as i32, by as i32));
        }

        path.push(end);
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jittered_delay() {
        let mut anti = AntiDetect::new();
        let base = 1000u64; // 1 second

        // Should be within 75%-125% of base
        for _ in 0..100 {
            let delay = anti.jittered_delay(base);
            let ms = delay.as_millis() as u64;
            assert!(ms >= 750, "Delay too short: {}", ms);
            assert!(ms <= 1250, "Delay too long: {}", ms);
        }
    }

    #[test]
    fn test_mouse_path() {
        let mut anti = AntiDetect::new();
        let start = (0, 0);
        let end = (100, 100);

        let path = anti.mouse_path(start, end);
        assert!(path.len() >= 3, "Path too short");
        assert_eq!(path[0], start, "Path should start at start");
        assert_eq!(path[path.len() - 1], end, "Path should end at end");
    }
}
