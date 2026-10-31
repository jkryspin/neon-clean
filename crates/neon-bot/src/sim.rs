//! Shadow simulation: a model character driven by the same inputs as the real one, on the map's
//! footholds, so the movement model can be compared tick by tick with what the client actually did.
//!
//! The bot feeds it every tick (`observe`) with the real state and the inputs that were held since
//! the previous tick. The shadow runs freely from the last reset; it is put back on the real
//! character when it drifts too far, the map changes, or a rope/ladder is used (not modelled).
//! Each tick is one line of `--sim-log` (whitespace separated, header first) for `tools/simfit.py`.

use crate::brain::Dir;
use crate::nav::{MapGeom, RawFoothold};
use neon_state::GameState;
use std::io::Write;
use std::path::PathBuf;

/// Movement constants, fitted to a controlled recording in Lith Harbor (2026-10-25, simcal.py):
/// walking ~149 px/s, a short slide after release; the sideways speed of a jump is set at take-off
/// (walking speed if a direction is held then, even from a standstill; otherwise the ground speed)
/// and kept until landing, releasing or pressing a direction in the air barely changes it; the jump
/// rises 78 px with its peak at ~0.30 s.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Ground walking speed, px/s.
    pub walk: f64,
    /// Sideways speed of a jump taken with a direction held, px/s.
    pub air: f64,
    /// Ground acceleration toward the walking speed while a direction is held, px/s^2.
    pub accel: f64,
    /// Ground deceleration with no direction held (the slide after release), px/s^2.
    pub friction: f64,
    /// Sideways acceleration from holding a direction in the air, px/s^2.
    pub air_accel: f64,
    /// Upward speed at take-off, px/s.
    pub jump_v0: f64,
    /// Gravity, px/s^2.
    pub gravity: f64,
    /// Terminal falling speed, px/s.
    pub max_fall: f64,
    /// Delay between sending a key and the character reacting in the position read, ms.
    pub latency_ms: u64,
    /// Step like the client: only on its frames (changed reads), over the elapsed frame time.
    pub frame_sync: bool,
    /// Longest frame interval the client integrates in one step, ms (frame-synced mode).
    pub frame_cap_ms: u64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            walk: 145.0,
            air: 154.75,
            accel: 7537.5,
            friction: 868.75,
            air_accel: 31.25,
            jump_v0: 555.0,
            gravity: 2000.0,
            max_fall: 670.0,
            latency_ms: 0,
            frame_sync: false,
            frame_cap_ms: 70,
        }
    }
}

/// What the player pressed during one interval.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Input {
    pub dir: Option<Dir>,
    /// A jump was pressed during the interval.
    pub jump: bool,
    /// Down was held (with `jump`: fall through the platform).
    pub down: bool,
    /// Up/Down on a rope or ladder: the shadow cannot follow, so it resets.
    pub climb: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub grounded: bool,
    /// Platform we are dropping through: ignored for landing until we are below it.
    drop_seg: Option<usize>,
}

/// Map geometry the shadow moves on.
#[derive(Default)]
pub struct World {
    floors: Vec<RawFoothold>,
    /// Vertical footholds: (x, top, bottom).
    walls: Vec<(i32, i32, i32)>,
}

const STEP_S: f64 = 0.005;
/// Steepest step up/down a walking body follows between adjoining floors, px.
const FOLLOW_PX: f64 = 14.0;
/// Half the body width for wall collisions, px.
const HALF_W: f64 = 10.0;

impl World {
    pub fn new(footholds: &[RawFoothold]) -> Self {
        let mut w = World::default();
        for f in footholds {
            if f.x1 == f.x2 {
                w.walls.push((f.x1, f.y1.min(f.y2), f.y1.max(f.y2)));
            } else {
                let (a, b) = if f.x1 <= f.x2 {
                    ((f.x1, f.y1), (f.x2, f.y2))
                } else {
                    ((f.x2, f.y2), (f.x1, f.y1))
                };
                w.floors.push(RawFoothold {
                    x1: a.0,
                    y1: a.1,
                    x2: b.0,
                    y2: b.1,
                    no_drop: f.no_drop,
                });
            }
        }
        w
    }

    fn y_at(f: &RawFoothold, x: f64) -> Option<f64> {
        if x < f.x1 as f64 || x > f.x2 as f64 {
            return None;
        }
        let t = (x - f.x1 as f64) / (f.x2 - f.x1) as f64;
        Some(f.y1 as f64 + t * (f.y2 - f.y1) as f64)
    }

    /// The floor under a walking body at `x`, near height `y`.
    fn floor_near(&self, x: f64, y: f64) -> Option<(usize, f64)> {
        self.floors
            .iter()
            .enumerate()
            .filter_map(|(i, f)| Self::y_at(f, x).map(|fy| (i, fy)))
            .filter(|(_, fy)| (fy - y).abs() <= FOLLOW_PX)
            .min_by(|a, b| (a.1 - y).abs().total_cmp(&(b.1 - y).abs()))
    }

    /// First floor crossed by feet falling from `y0` to `y1` at `x`.
    fn landing(&self, x: f64, y0: f64, y1: f64, skip: Option<usize>) -> Option<(usize, f64)> {
        self.floors
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != skip)
            .filter_map(|(i, f)| Self::y_at(f, x).map(|fy| (i, fy)))
            .filter(|(_, fy)| y0 <= fy + 0.5 && y1 >= *fy)
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    fn blocked(&self, x0: f64, x1: f64, y: f64) -> bool {
        self.walls.iter().any(|&(wx, top, bottom)| {
            let wx = wx as f64;
            let crossing = if x1 > x0 {
                x0 + HALF_W <= wx + 0.5 && x1 + HALF_W > wx
            } else {
                x0 - HALF_W >= wx - 0.5 && x1 - HALF_W < wx
            };
            // The client checks walls at the feet, not over the body's height.
            crossing && y > top as f64 && y <= bottom as f64 + 0.5
        })
    }

    /// Advance the body by `dt` seconds under `input` (jump/down are applied on the first sub-step).
    pub fn step(&self, p: &Params, b: &mut Body, input: Input, dt: f64) {
        let sign = match input.dir {
            Some(Dir::Left) => -1.0,
            Some(Dir::Right) => 1.0,
            None => 0.0,
        };
        let mut jump = input.jump;
        let mut t = 0.0;
        while t < dt - 1e-9 {
            let h = STEP_S.min(dt - t);
            t += h;
            if b.grounded {
                if jump {
                    jump = false;
                    if input.down {
                        // Fall through, unless this floor forbids it.
                        if let Some((i, _)) = self
                            .floor_near(b.x, b.y)
                            .filter(|(i, _)| !self.floors[*i].no_drop)
                        {
                            b.drop_seg = Some(i);
                            b.grounded = false;
                            b.vx = 0.0;
                            b.vy = 0.0;
                        }
                    } else {
                        // Sideways speed is fixed at take-off: walking speed in the held direction (even
                        // from a standstill), else whatever we had on the ground.
                        b.grounded = false;
                        b.vy = -p.jump_v0;
                        if sign != 0.0 {
                            // At least `air` in the held direction; a running start keeps its speed.
                            let along = (b.vx * sign).max(0.0);
                            b.vx = sign * along.max(p.air);
                        }
                    }
                    if !b.grounded {
                        continue;
                    }
                }
                b.vx = approach(
                    b.vx,
                    sign * p.walk,
                    if sign != 0.0 { p.accel } else { p.friction } * h,
                );
                let nx = b.x + b.vx * h;
                if self.blocked(b.x, nx, b.y) {
                    continue;
                }
                match self.floor_near(nx, b.y) {
                    Some((_, fy)) => {
                        b.x = nx;
                        b.y = fy;
                    }
                    None => {
                        // Walked off the end.
                        b.x = nx;
                        b.grounded = false;
                        b.vy = 0.0;
                    }
                }
            } else {
                b.vy = (b.vy + p.gravity * h).min(p.max_fall);
                if sign != 0.0 {
                    b.vx = approach(b.vx, sign * p.walk, p.air_accel * h);
                }
                let nx = b.x + b.vx * h;
                if !self.blocked(b.x, nx, b.y) {
                    b.x = nx;
                }
                let ny = b.y + b.vy * h;
                if b.vy > 0.0 {
                    if let Some((_, fy)) = self.landing(b.x, b.y, ny, b.drop_seg) {
                        b.y = fy;
                        b.vy = 0.0;
                        b.grounded = true;
                        b.drop_seg = None;
                        continue;
                    }
                }
                b.y = ny;
            }
        }
    }
}

/// Move `v` toward `target` by at most `dv`.
fn approach(v: f64, target: f64, dv: f64) -> f64 {
    if v < target {
        (v + dv).min(target)
    } else {
        (v - dv).max(target)
    }
}

/// Running comparison statistics, by motion mode.
#[derive(Debug, Default, Clone, Copy)]
pub struct ErrStats {
    pub n: u64,
    pub sum: f64,
    pub max: f64,
}

impl ErrStats {
    fn add(&mut self, e: f64) {
        self.n += 1;
        self.sum += e;
        self.max = self.max.max(e);
    }
    pub fn mean(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.sum / self.n as f64
        }
    }
}

/// How far back a read can be: the client's longest frame interval seen (ms).
const FRAME_WINDOW_MS: u64 = 70;

/// Distance at which the shadow is considered lost and put back on the real character, px.
const RESET_ERR: f64 = 80.0;

pub struct Shadow {
    pub params: Params,
    maps_dir: PathBuf,
    map: Option<u32>,
    world: World,
    body: Option<Body>,
    since_reset_ms: u64,
    last_ts: Option<u64>,
    /// Previous real position (x, y, ts) for velocity at a reset.
    prev: Option<(f64, f64, u64)>,
    /// Inputs waiting for their latency to pass: (time they take effect, input).
    queue: std::collections::VecDeque<(u64, Input)>,
    /// Frame-synced mode: time of the last client frame, and a jump / drop pressed since then.
    frame_ts: u64,
    frame_jump: bool,
    frame_down: bool,
    /// Recent simulated positions (time, x, y), to match reads that are up to a client frame old.
    path: std::collections::VecDeque<(u64, f64, f64)>,
    /// Input currently in effect (held direction).
    cur: Input,
    log: Option<std::io::BufWriter<std::fs::File>>,
    pub ground: ErrStats,
    pub air: ErrStats,
    /// Error at the moment the real character lands after being airborne.
    pub landing: ErrStats,
    was_airborne: bool,
    pub resets: u64,
    /// Put the shadow back on the real character this often (ms), to measure short-horizon error.
    pub resync_ms: Option<u64>,
}

impl Shadow {
    pub fn new(maps_dir: PathBuf, log: Option<PathBuf>) -> Self {
        let log = log
            .and_then(|p| std::fs::File::create(p).ok())
            .map(std::io::BufWriter::new)
            .map(|mut w| {
                let _ = writeln!(w, "ts dt dir jump down climb ax ay agr sx sy sgr age err");
                w
            });
        Self {
            params: Params::default(),
            maps_dir,
            map: None,
            world: World::default(),
            body: None,
            since_reset_ms: 0,
            last_ts: None,
            prev: None,
            queue: Default::default(),
            path: Default::default(),
            frame_ts: 0,
            frame_jump: false,
            frame_down: false,
            cur: Input::default(),
            log,
            ground: ErrStats::default(),
            air: ErrStats::default(),
            landing: ErrStats::default(),
            was_airborne: false,
            resets: 0,
            resync_ms: None,
        }
    }

    pub fn body(&self) -> Option<Body> {
        self.body
    }

    fn reset_to(&mut self, s: &GameState) {
        let (x, y) = (s.player.x as f64, s.player.y as f64);
        // Carry the real character's current speed over from the previous frame.
        let (vx, vy) = match self.prev {
            Some((px, py, pt)) if s.ts_ms > pt && s.ts_ms - pt <= 200 => {
                let dt = (s.ts_ms - pt) as f64 / 1000.0;
                (
                    ((x - px) / dt).clamp(-400.0, 400.0),
                    ((y - py) / dt).clamp(-900.0, 900.0),
                )
            }
            _ => (0.0, 0.0),
        };
        // Standing: snap to the floor under the feet (the read can sit a few px off).
        let floor = self.world.floor_near(x, y);
        let grounded = s.player.grounded.unwrap_or(floor.is_some()) && floor.is_some();
        self.body = Some(Body {
            x,
            y: if grounded {
                floor.map_or(y, |f| f.1)
            } else {
                y
            },
            vx,
            vy: if grounded { 0.0 } else { vy },
            grounded,
            drop_seg: None,
        });
        self.since_reset_ms = 0;
        self.resets += 1;
        self.path.clear();
        self.frame_ts = s.ts_ms;
        self.frame_jump = false;
        self.frame_down = false;
    }

    /// Advance the shadow to this state's time with the inputs held since the previous call, then
    /// compare it with where the real character is.
    pub fn observe(&mut self, s: &GameState, input: Input) {
        self.observe_inner(s, input);
        self.prev = Some((s.player.x as f64, s.player.y as f64, s.ts_ms));
    }

    fn observe_inner(&mut self, s: &GameState, input: Input) {
        let Some(map) = s.player.map_id else { return };
        if !s.caps.position {
            self.body = None;
            return;
        }
        if self.map != Some(map) {
            self.map = Some(map);
            self.world = MapGeom::load(&self.maps_dir, map)
                .map(|g| World::new(&g.footholds))
                .unwrap_or_default();
            self.body = None;
        }
        let t0 = self.last_ts.unwrap_or(s.ts_ms);
        let dt_ms = s.ts_ms.saturating_sub(t0);
        self.last_ts = Some(s.ts_ms);
        // The inputs were sent at the previous frame; the game reacts `latency_ms` later.
        self.queue.push_back((t0 + self.params.latency_ms, input));
        // A long gap (stream stall, pause) cannot be replayed faithfully.
        if self.body.is_none() || dt_ms > 500 || input.climb {
            self.reset_to(s);
            return;
        }
        let mut b = self.body.unwrap();
        if self.params.frame_sync {
            // Like the client: nothing moves between frames; on a frame (seen as a changed read) the keys held at
            // that moment, and any jump pressed since the last frame, drive physics over the whole frame interval.
            while let Some(&(at, inp)) = self.queue.front() {
                if at > s.ts_ms {
                    break;
                }
                self.queue.pop_front();
                self.cur = inp;
                self.frame_jump |= inp.jump;
                self.frame_down |= inp.down;
            }
            let moved = self.prev.map_or(true, |(px, py, _)| {
                px != s.player.x as f64 || py != s.player.y as f64
            });
            if moved {
                let dt = s
                    .ts_ms
                    .saturating_sub(self.frame_ts)
                    .min(self.params.frame_cap_ms);
                let inp = Input {
                    dir: self.cur.dir,
                    jump: self.frame_jump,
                    down: self.frame_down,
                    climb: false,
                };
                self.world
                    .step(&self.params, &mut b, inp, dt as f64 / 1000.0);
                self.frame_jump = false;
                self.frame_down = false;
                self.frame_ts = s.ts_ms;
            }
            self.path.clear();
            self.path.push_back((s.ts_ms, b.x, b.y));
        }
        // Step through the interval, switching inputs at the moments they take effect.
        let mut t = if self.params.frame_sync { s.ts_ms } else { t0 };
        while t < s.ts_ms {
            let mut fresh = Input {
                dir: self.cur.dir,
                ..Default::default()
            };
            while let Some(&(at, inp)) = self.queue.front() {
                if at > t {
                    break;
                }
                self.queue.pop_front();
                self.cur = inp;
                fresh = Input {
                    dir: inp.dir,
                    jump: fresh.jump || inp.jump,
                    down: fresh.down || inp.down,
                    climb: inp.climb,
                };
            }
            let next = self
                .queue
                .front()
                .map_or(s.ts_ms, |&(at, _)| at.clamp(t + 1, s.ts_ms))
                .min(t + 5);
            self.world
                .step(&self.params, &mut b, fresh, (next - t) as f64 / 1000.0);
            t = next;
            self.path.push_back((t, b.x, b.y));
        }
        while self
            .path
            .front()
            .map_or(false, |&(pt, _, _)| pt + 200 < s.ts_ms)
        {
            self.path.pop_front();
        }
        self.since_reset_ms += dt_ms;
        let (ax, ay) = (s.player.x as f64, s.player.y as f64);
        // The client moves the character on its own frames (~20/s, unevenly spaced), so the value read now is where
        // it was at some instant of the last frame interval: score the nearest point of the simulated path in it.
        let err = self
            .path
            .iter()
            .filter(|&&(pt, _, _)| pt + FRAME_WINDOW_MS >= s.ts_ms)
            .map(|&(_, x, y)| ((x - ax).powi(2) + (y - ay).powi(2)).sqrt())
            .fold(((b.x - ax).powi(2) + (b.y - ay).powi(2)).sqrt(), f64::min);
        // The read updates in bursts: only a changed value says where the character is now.
        let fresh = self.prev.map_or(true, |(px, py, _)| px != ax || py != ay);
        if fresh {
            if b.grounded {
                self.ground.add(err)
            } else {
                self.air.add(err)
            }
            // Landing: the first fresh grounded read after time in the air (y settled on a floor).
            match s.player.grounded {
                Some(false) => self.was_airborne = true,
                // The flag flips a frame before y settles: count the landing once the feet are on the floor.
                Some(true)
                    if self.was_airborne
                        && self
                            .world
                            .floor_near(ax, ay)
                            .map_or(false, |(_, fy)| (fy - ay).abs() < 1.0) =>
                {
                    self.was_airborne = false;
                    self.landing.add(err);
                }
                _ => {}
            }
        }
        if let Some(w) = self.log.as_mut() {
            let dir = match input.dir {
                Some(Dir::Left) => "L",
                Some(Dir::Right) => "R",
                None => "-",
            };
            let gr = |g: Option<bool>| match g {
                Some(true) => "1",
                Some(false) => "0",
                None => "?",
            };
            let _ = writeln!(
                w,
                "{} {} {} {} {} {} {} {} {} {:.1} {:.1} {} {} {:.1}",
                s.ts_ms,
                dt_ms,
                dir,
                input.jump as u8,
                input.down as u8,
                input.climb as u8,
                s.player.x,
                s.player.y,
                gr(s.player.grounded),
                b.x,
                b.y,
                b.grounded as u8,
                self.since_reset_ms,
                err
            );
            let _ = w.flush();
        }
        self.body = Some(b);
        if err > RESET_ERR
            || (self.resync_ms.map_or(false, |r| self.since_reset_ms >= r)
                && s.player.grounded == Some(true)
                && b.grounded)
        {
            self.reset_to(s);
        }
    }

    /// One line for logs / the status file.
    pub fn summary(&self) -> String {
        format!(
            "sim: ground err {:.1} px avg ({} ticks, max {:.0}), air err {:.1} px avg ({} ticks, max {:.0}), landing err {:.1} px ({} landings), {} resets",
            self.ground.mean(),
            self.ground.n,
            self.ground.max,
            self.air.mean(),
            self.air.n,
            self.air.max,
            self.landing.mean(),
            self.landing.n,
            self.resets
        )
    }
}

/// Replay a `--trace-file` decision trace (recorded in one map) through the shadow, writing the
/// comparison log to `out`. The trace has the real position each frame and the actions sent.
pub fn replay(
    trace: &std::path::Path,
    maps_dir: PathBuf,
    map: u32,
    jump_key: &str,
    out: Option<PathBuf>,
) -> Result<Shadow, String> {
    let text = std::fs::read_to_string(trace).map_err(|e| format!("{}: {e}", trace.display()))?;
    replay_text(&text, Shadow::new(maps_dir, out), map, jump_key)
}

/// Replay already-loaded trace text through `sh`.
pub fn replay_text(text: &str, mut sh: Shadow, map: u32, jump_key: &str) -> Result<Shadow, String> {
    let mut held: Option<Dir> = None;
    let mut climbing = false;
    let mut pending = Input::default();
    let jump_tap = format!("Tap(\"{jump_key}\")");
    for line in text.lines() {
        // "<ts> pos=x,y gr=Some(true) tgt=... mobs=N | decision | actions"
        let mut parts = line.splitn(3, " | ");
        let (Some(head), _, acts) = (parts.next(), parts.next(), parts.next().unwrap_or("")) else {
            continue;
        };
        let mut it = head.split_whitespace();
        let Some(ts) = it.next().and_then(|t| t.parse::<u64>().ok()) else {
            continue;
        };
        let Some((x, y)) = it
            .next()
            .and_then(|p| p.strip_prefix("pos="))
            .and_then(|p| p.split_once(','))
        else {
            continue;
        };
        let (Ok(x), Ok(y)) = (x.parse::<i32>(), y.parse::<i32>()) else {
            continue;
        };
        let gr = match it.next() {
            Some("gr=Some(true)") => Some(true),
            Some("gr=Some(false)") => Some(false),
            _ => None,
        };
        let mut s = GameState::default();
        s.ts_ms = ts;
        s.caps.position = true;
        s.player.map_id = Some(map);
        s.player.x = x;
        s.player.y = y;
        s.player.grounded = gr;
        sh.observe(&s, pending);
        // Inputs sent at this frame drive the shadow until the next one.
        pending = Input::default();
        if acts.contains("Move(Some(Left))") {
            held = Some(Dir::Left);
        } else if acts.contains("Move(Some(Right))") {
            held = Some(Dir::Right);
        } else if acts.contains("Move(None)") {
            held = None;
        }
        if acts.contains("Vert(Some(") {
            climbing = true;
        } else if acts.contains("Vert(None)") {
            climbing = false;
        }
        if acts.contains("Nudge(Left") {
            pending.dir = Some(Dir::Left);
        } else if acts.contains("Nudge(Right") {
            pending.dir = Some(Dir::Right);
        }
        pending.dir = pending.dir.or(held);
        pending.climb = climbing;
        if acts.contains(&jump_tap) {
            pending.jump = true;
        }
        if acts.contains("DropDown") {
            pending.jump = true;
            pending.down = true;
        }
    }
    Ok(sh)
}

/// Fit the movement constants to recorded traces (all in `map`) by coordinate descent on the
/// average position error with the shadow re-synced every `resync_ms`.
pub fn fit(
    traces: &[String],
    maps_dir: PathBuf,
    map: u32,
    jump_key: &str,
    resync_ms: u64,
) -> Result<(Params, f64, f64), String> {
    let texts: Vec<String> = traces
        .iter()
        .map(|t| std::fs::read_to_string(t).map_err(|e| format!("{t}: {e}")))
        .collect::<Result<_, _>>()?;
    let score = |p: &Params| -> Result<f64, String> {
        let (mut sum, mut n) = (0.0, 0u64);
        let (mut land_sum, mut land_n) = (0.0, 0u64);
        for t in &texts {
            let mut sh = Shadow::new(maps_dir.clone(), None);
            sh.params = *p;
            sh.resync_ms = Some(resync_ms);
            let sh = replay_text(t, sh, map, jump_key)?;
            sum += sh.ground.sum + sh.air.sum;
            n += sh.ground.n + sh.air.n;
            land_sum += sh.landing.sum;
            land_n += sh.landing.n;
        }
        // Where jumps end matters most to navigation: weigh the landing error in.
        let all = if n == 0 {
            f64::MAX / 4.0
        } else {
            sum / n as f64
        };
        let land = if land_n == 0 {
            0.0
        } else {
            land_sum / land_n as f64
        };
        Ok(all + 2.0 * land)
    };
    let mut best = Params::default();
    let start = score(&best)?;
    let mut cur = start;
    // (getter/setter, step)
    type Knob = (fn(&mut Params) -> &mut f64, f64);
    let mut knobs: Vec<Knob> = vec![
        (|p| &mut p.walk, 4.0),
        (|p| &mut p.air, 4.0),
        (|p| &mut p.accel, 200.0),
        (|p| &mut p.friction, 200.0),
        (|p| &mut p.air_accel, 20.0),
        // jump_v0, gravity and max_fall are the client's own (Map.wz/Physics.img: jumpSpeed 555,
        // gravityAcc 2000, fallSpeed 670): not fitted.
    ];
    let mut lat_step: i64 = 20;
    for _round in 0..12 {
        let mut improved = false;
        for (get, step) in knobs.iter_mut() {
            for dir in [1.0, -1.0] {
                loop {
                    let mut p = best;
                    let v = get(&mut p);
                    *v = (*v + dir * *step).max(0.0);
                    let sc = score(&p)?;
                    if sc + 1e-6 < cur {
                        best = p;
                        cur = sc;
                        improved = true;
                    } else {
                        break;
                    }
                }
            }
        }
        for dir in [1i64, -1] {
            loop {
                let mut p = best;
                p.frame_cap_ms = (p.frame_cap_ms as i64 + dir * lat_step).clamp(10, 200) as u64;
                let sc = score(&p)?;
                if sc + 1e-6 < cur {
                    best = p;
                    cur = sc;
                    improved = true;
                } else {
                    break;
                }
            }
        }
        for dir in [1i64, -1] {
            loop {
                let mut p = best;
                p.latency_ms = (p.latency_ms as i64 + dir * lat_step).max(0) as u64;
                let sc = score(&p)?;
                if sc + 1e-6 < cur {
                    best = p;
                    cur = sc;
                    improved = true;
                } else {
                    break;
                }
            }
        }
        if !improved {
            for (_, step) in knobs.iter_mut() {
                *step /= 2.0;
            }
            lat_step = (lat_step / 2).max(1);
        }
    }
    Ok((best, start, cur))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat() -> World {
        World::new(&[
            RawFoothold {
                x1: -1000,
                y1: 0,
                x2: 1000,
                y2: 0,
                no_drop: false,
            },
            RawFoothold {
                x1: 0,
                y1: -60,
                x2: 200,
                y2: -60,
                no_drop: false,
            },
        ])
    }

    fn standing(x: f64, y: f64) -> Body {
        Body {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            grounded: true,
            drop_seg: None,
        }
    }

    #[test]
    fn walks_at_walk_speed() {
        let w = flat();
        let p = Params::default();
        let mut b = standing(-500.0, 0.0);
        w.step(
            &p,
            &mut b,
            Input {
                dir: Some(Dir::Right),
                ..Default::default()
            },
            1.0,
        );
        // Walking speed after the short acceleration.
        let ramp = p.walk * p.walk / (2.0 * p.accel);
        assert!((b.x - (-500.0 + p.walk - ramp)).abs() < 2.0, "{b:?}");
        assert_eq!(b.vx, p.walk);
        assert!(b.grounded);
    }

    #[test]
    fn standing_jump_rises_78_and_lands_back() {
        let w = flat();
        let p = Params::default();
        let mut b = standing(-500.0, 0.0);
        let (t_peak, rise) = (
            p.jump_v0 / p.gravity,
            p.jump_v0 * p.jump_v0 / (2.0 * p.gravity),
        );
        assert!(
            (70.0..=80.0).contains(&rise),
            "measured rise is ~78 px, got {rise}"
        );
        w.step(
            &p,
            &mut b,
            Input {
                jump: true,
                ..Default::default()
            },
            t_peak,
        );
        assert!((b.y + rise).abs() < 3.0, "peak {b:?}");
        assert_eq!(b.x, -500.0, "a standing jump goes straight up");
        w.step(&p, &mut b, Input::default(), 0.4);
        assert!(b.grounded && b.y == 0.0, "{b:?}");
    }

    #[test]
    fn jumps_onto_a_platform_above() {
        let w = flat();
        let p = Params::default();
        let mut b = standing(-60.0, 0.0);
        w.step(
            &p,
            &mut b,
            Input {
                dir: Some(Dir::Right),
                ..Default::default()
            },
            0.3,
        );
        w.step(
            &p,
            &mut b,
            Input {
                dir: Some(Dir::Right),
                jump: true,
                ..Default::default()
            },
            0.6,
        );
        assert!(b.grounded && b.y == -60.0, "{b:?}");
    }

    #[test]
    fn drops_through_and_walks_off_edges() {
        let w = flat();
        let p = Params::default();
        let mut b = standing(100.0, -60.0);
        w.step(
            &p,
            &mut b,
            Input {
                jump: true,
                down: true,
                ..Default::default()
            },
            0.5,
        );
        assert!(b.grounded && b.y == 0.0, "dropped {b:?}");
        let mut b = standing(190.0, -60.0);
        w.step(
            &p,
            &mut b,
            Input {
                dir: Some(Dir::Right),
                ..Default::default()
            },
            0.5,
        );
        assert!(b.grounded && b.y == 0.0 && b.x > 200.0, "walked off {b:?}");
    }

    #[test]
    fn walls_stop_walking() {
        let mut fh = vec![RawFoothold {
            x1: -1000,
            y1: 0,
            x2: 1000,
            y2: 0,
            no_drop: false,
        }];
        fh.push(RawFoothold {
            x1: 100,
            y1: -200,
            x2: 100,
            y2: 0,
            no_drop: false,
        });
        let w = World::new(&fh);
        let mut b = standing(0.0, 0.0);
        w.step(
            &Params::default(),
            &mut b,
            Input {
                dir: Some(Dir::Right),
                ..Default::default()
            },
            2.0,
        );
        assert!(b.x <= 90.5, "{b:?}");
    }
}
