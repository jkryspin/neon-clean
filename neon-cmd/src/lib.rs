//! Command layer (plan M5 / A5 / A8).
//!
//! The brain emits typed [`Command`]s. Every command passes the [`SafetyGate`] (timing variance, rate limit, idle
//! breaks, session cap, pause switches), is carried out by an [`Actuator`] (stage A: keys through XTEST; stage B, only after
//! written Nexon approval: packets), and is checked by the [`Executor`] against the world model: each command names the
//! effect it expects and a deadline, so a key that did nothing shows up as a timeout instead of silence.

use neon_world::Snapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heading {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Start walking (key held until `Stop`).
    Walk(Heading),
    Stop,
    Jump,
    Attack,
    /// Press a named key once (a hotkey: potion, buff, pickup).
    Tap(String),
    /// Click at screen coordinates of the client window.
    Click { x: i32, y: i32 },
    /// Type text then Enter (chat).
    Say(String),
}

type Check = Box<dyn Fn(&Snapshot) -> bool + Send>;

/// The effect a command should have on the world, checked every tick until the deadline.
pub enum Expect {
    /// No observable effect (e.g. releasing a key): satisfied immediately.
    Nothing,
    MapIs(u32),
    MeHpAtLeast(u32),
    DropGone(u32),
    MobGone(u32),
    Custom(Check),
}

impl Expect {
    fn holds(&self, s: &Snapshot) -> bool {
        match self {
            Expect::Nothing => true,
            Expect::MapIs(m) => s.map == Some(*m),
            Expect::MeHpAtLeast(h) => s.me.hp.map_or(false, |v| v >= *h),
            Expect::DropGone(oid) => !s.drops.contains_key(oid),
            Expect::MobGone(oid) => !s.mobs.contains_key(oid),
            Expect::Custom(f) => f(s),
        }
    }
}

// ------------------------------------------------------------------ safety gate

/// Small deterministic generator, so humanised timing is reproducible in tests (xorshift64*).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    pub fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    /// `base` varied by up to ±`frac` (0.25 = the ±25% the authorization asks for).
    pub fn jitter(&mut self, base_ms: u64, frac: f64) -> u64 {
        let f = 1.0 + (self.next_f64() * 2.0 - 1.0) * frac;
        (base_ms as f64 * f).round() as u64
    }
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + ((hi - lo) as f64 * self.next_f64()) as u64
    }
}

#[derive(Debug, Clone)]
pub struct GateConfig {
    /// Minimum gap between two commands before jitter.
    pub min_gap_ms: u64,
    pub jitter: f64,
    pub max_per_minute: usize,
    /// An idle break every this-many minutes (random in range) ...
    pub break_every_min: (u64, u64),
    /// ... lasting this many seconds (random in range).
    pub break_len_s: (u64, u64),
    /// The session ends (no more commands) after this long.
    pub max_session_ms: u64,
}

impl Default for GateConfig {
    fn default() -> Self {
        // From docs/authorization.md: +-25% timing, breaks every 45-90 min, 6 h session maximum, no packet spamming.
        GateConfig { min_gap_ms: 120, jitter: 0.25, max_per_minute: 240, break_every_min: (45, 90), break_len_s: (30, 180), max_session_ms: 6 * 3600 * 1000 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Allow,
    /// Try again after this many ms.
    Wait(u64),
    Blocked(String),
}

#[derive(Debug)]
pub struct SafetyGate {
    cfg: GateConfig,
    rng: Rng,
    start_ms: u64,
    last_ms: Option<u64>,
    required_gap: u64,
    recent: std::collections::VecDeque<u64>,
    next_break_at: u64,
    break_until: Option<u64>,
    /// Set by the brain/stream: a staff member is on the map, a lie detector is up, HP is critical, ...
    pauses: std::collections::BTreeSet<String>,
}

impl SafetyGate {
    pub fn new(cfg: GateConfig, now_ms: u64, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let first = now_ms + rng.range(cfg.break_every_min.0, cfg.break_every_min.1) * 60_000;
        let gap = rng.jitter(cfg.min_gap_ms, cfg.jitter);
        SafetyGate { cfg, rng, start_ms: now_ms, last_ms: None, required_gap: gap, recent: Default::default(), next_break_at: first, break_until: None, pauses: Default::default() }
    }

    /// Hold all commands while `reason` is set (e.g. "gm", "lie_detector", "low_hp"). Idempotent.
    pub fn pause(&mut self, reason: &str) {
        self.pauses.insert(reason.to_string());
    }
    pub fn resume(&mut self, reason: &str) {
        self.pauses.remove(reason);
    }

    pub fn check(&mut self, now_ms: u64) -> Verdict {
        if now_ms.saturating_sub(self.start_ms) >= self.cfg.max_session_ms {
            return Verdict::Blocked("session limit reached".into());
        }
        if let Some(r) = self.pauses.iter().next() {
            return Verdict::Blocked(format!("paused: {r}"));
        }
        if let Some(until) = self.break_until {
            if now_ms < until {
                return Verdict::Wait(until - now_ms);
            }
            self.break_until = None;
            self.next_break_at = now_ms + self.rng.range(self.cfg.break_every_min.0, self.cfg.break_every_min.1) * 60_000;
        } else if now_ms >= self.next_break_at {
            let len = self.rng.range(self.cfg.break_len_s.0, self.cfg.break_len_s.1) * 1000;
            self.break_until = Some(now_ms + len);
            return Verdict::Wait(len);
        }
        while self.recent.front().map_or(false, |t| now_ms.saturating_sub(*t) >= 60_000) {
            self.recent.pop_front();
        }
        if self.recent.len() >= self.cfg.max_per_minute {
            return Verdict::Wait(60_000 - now_ms.saturating_sub(*self.recent.front().unwrap()));
        }
        if let Some(last) = self.last_ms {
            let since = now_ms.saturating_sub(last);
            if since < self.required_gap {
                return Verdict::Wait(self.required_gap - since);
            }
        }
        Verdict::Allow
    }

    /// Record that a command was sent now (call only after `check` returned `Allow`).
    pub fn record(&mut self, now_ms: u64) {
        self.last_ms = Some(now_ms);
        self.recent.push_back(now_ms);
        self.required_gap = self.rng.jitter(self.cfg.min_gap_ms, self.cfg.jitter);
    }

    pub fn on_break(&self) -> bool {
        self.break_until.is_some()
    }
}

// ------------------------------------------------------------------ actuators

pub trait Actuator {
    fn execute(&mut self, cmd: &Command) -> Result<(), String>;
}

/// Stage A actuator: builds the `xdotool` invocations for one X display (the instance's own Xvfb). Execution is behind
/// a closure so tests never touch a display.
pub struct XtestActuator<F: FnMut(&[String]) -> Result<(), String>> {
    pub display: u32,
    pub run: F,
    /// Which key each heading holds.
    pub left_key: String,
    pub right_key: String,
    pub jump_key: String,
    pub attack_key: String,
}

impl<F: FnMut(&[String]) -> Result<(), String>> XtestActuator<F> {
    pub fn new(display: u32, run: F) -> Self {
        XtestActuator { display, run, left_key: "Left".into(), right_key: "Right".into(), jump_key: "alt".into(), attack_key: "ctrl".into() }
    }

    /// The argv `xdotool` would get for this command (the DISPLAY is set by the runner).
    pub fn argv(&self, cmd: &Command) -> Vec<Vec<String>> {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        match cmd {
            Command::Walk(Heading::Left) => vec![s(&["xdotool", "keyup", &self.right_key, "keydown", &self.left_key])],
            Command::Walk(Heading::Right) => vec![s(&["xdotool", "keyup", &self.left_key, "keydown", &self.right_key])],
            Command::Stop => vec![s(&["xdotool", "keyup", &self.left_key, &self.right_key])],
            Command::Jump => vec![s(&["xdotool", "key", &self.jump_key])],
            Command::Attack => vec![s(&["xdotool", "key", &self.attack_key])],
            Command::Tap(k) => vec![s(&["xdotool", "key", k])],
            Command::Click { x, y } => vec![s(&["xdotool", "mousemove", &x.to_string(), &y.to_string(), "click", "1"])],
            Command::Say(t) => vec![s(&["xdotool", "key", "Return"]), s(&["xdotool", "type", "--delay", "80", t]), s(&["xdotool", "key", "Return"])],
        }
    }
}

impl<F: FnMut(&[String]) -> Result<(), String>> Actuator for XtestActuator<F> {
    fn execute(&mut self, cmd: &Command) -> Result<(), String> {
        for a in self.argv(cmd) {
            (self.run)(&a)?;
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ executor

pub type CmdId = u64;

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The expected effect was seen.
    Confirmed { after_ms: u64 },
    /// The deadline passed without the effect.
    TimedOut,
    /// The actuator failed.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Submit {
    Sent(CmdId),
    /// Not sent: the gate said wait or blocked.
    Held(Verdict),
}

struct Pending {
    id: CmdId,
    sent_ms: u64,
    deadline_ms: u64,
    expect: Expect,
}

pub struct Executor<A: Actuator> {
    pub actuator: A,
    pub gate: SafetyGate,
    pending: Vec<Pending>,
    next_id: CmdId,
}

impl<A: Actuator> Executor<A> {
    pub fn new(actuator: A, gate: SafetyGate) -> Self {
        Executor { actuator, gate, pending: Vec::new(), next_id: 1 }
    }

    pub fn submit(&mut self, cmd: Command, expect: Expect, within_ms: u64, now_ms: u64) -> (Submit, Option<(CmdId, Outcome)>) {
        match self.gate.check(now_ms) {
            Verdict::Allow => {}
            v => return (Submit::Held(v), None),
        }
        let id = self.next_id;
        self.next_id += 1;
        self.gate.record(now_ms);
        if let Err(e) = self.actuator.execute(&cmd) {
            return (Submit::Sent(id), Some((id, Outcome::Failed(e))));
        }
        self.pending.push(Pending { id, sent_ms: now_ms, deadline_ms: now_ms + within_ms, expect });
        (Submit::Sent(id), None)
    }

    /// Check pending commands against the latest world; returns those that resolved.
    pub fn tick(&mut self, snap: &Snapshot, now_ms: u64) -> Vec<(CmdId, Outcome)> {
        let mut done = Vec::new();
        self.pending.retain(|p| {
            if p.expect.holds(snap) {
                done.push((p.id, Outcome::Confirmed { after_ms: now_ms.saturating_sub(p.sent_ms) }));
                false
            } else if now_ms >= p.deadline_ms {
                done.push((p.id, Outcome::TimedOut));
                false
            } else {
                true
            }
        });
        done
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

// ------------------------------------------------------------------ staff presence

/// Staff recognition by name only (no GM flag has been decoded from the wire yet): the same rules as the brain's
/// Phase 8 check in `neon-loader`.
#[derive(Debug, Clone)]
pub struct StaffRules {
    pub names: std::collections::HashSet<String>,
    pub prefixes: Vec<String>,
}

impl Default for StaffRules {
    fn default() -> Self {
        StaffRules { names: Default::default(), prefixes: vec!["gm_".into(), "[gm]".into(), "gm ".into()] }
    }
}

impl StaffRules {
    pub fn find(&self, s: &Snapshot) -> Option<String> {
        s.players.values().filter_map(|p| p.name.as_ref()).find(|n| {
            let l = n.to_lowercase();
            self.names.contains(&l) || self.prefixes.iter().any(|p| l.starts_with(p.as_str()))
        }).cloned()
    }

    /// Pause the gate while staff are on the map, resume when they are gone. Call every tick.
    pub fn sync(&self, gate: &mut SafetyGate, s: &Snapshot) {
        match self.find(s) {
            Some(_) => gate.pause("gm"),
            None => gate.resume("gm"),
        }
    }
}
