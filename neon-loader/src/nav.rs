//! Map navigation: platforms (footholds), ladders/ropes and the ways between them.
//!
//! Geometry comes from the client's own map data (`neon-calib/tools/wzmap.py`), so
//! the graph is exact: every walkable segment is a node, and edges are the moves
//! the character can actually make (walk, fall off an edge, drop through a platform,
//! climb a ladder/rope, short jump up). `step` replans from the character's current
//! position every call, so a missed jump or a knock-back just produces a new route.

use serde::Deserialize;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct RawFoothold {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    #[serde(default)]
    pub no_drop: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ladder {
    pub x: i32,
    pub y1: i32,
    pub y2: i32,
    /// true = ladder, false = rope (a rope must be grabbed with a jump).
    pub ladder: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Portal {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<i32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub to_map: Option<u32>,
}

/// One map's raw geometry as written by `wzmap.py`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MapGeom {
    pub footholds: Vec<RawFoothold>,
    #[serde(default)]
    pub ladders: Vec<Ladder>,
    #[serde(default)]
    pub portals: Vec<Portal>,
    /// Mob spawn counts by template id (empty for towns).
    #[serde(default)]
    pub mobs: std::collections::HashMap<String, u32>,
    /// Where each mob spawns.
    #[serde(default)]
    pub mob_spawns: Vec<SpawnPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct SpawnPoint {
    pub id: u32,
    pub x: i32,
    pub y: i32,
}

impl MapGeom {
    pub fn load(dir: &Path, map_id: u32) -> Option<MapGeom> {
        let text = std::fs::read_to_string(dir.join(format!("{map_id}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    }
}

pub use crate::brain::Dir;

/// One real jump from a decision trace, compared with the navigator's prediction.
pub struct JumpCheck {
    pub t: u64,
    pub from: (i32, i32),
    pub dir: Option<Dir>,
    pub real: (i32, i32),
    pub real_seg: Option<usize>,
    pub predicted: Option<(usize, i32)>,
    pub from_seg: Option<usize>,
    pub matched: bool,
    /// The model gives more than one possible landing for this take-off.
    pub marginal: bool,
}

impl JumpCheck {
    pub fn describe(&self) -> String {
        format!(
            "t={} from {:?} seg {:?} dir {:?} -> real {:?} seg {:?}, predicted {:?}",
            self.t, self.from, self.from_seg, self.dir, self.real, self.real_seg, self.predicted
        )
    }
}

/// Every jump in a trace (a jump key tap while standing, not a rope grab or a drop): where it really landed (first
/// grounded read with the feet on a floor after being airborne) against `predict_jump`.
pub fn check_jumps(n: &Nav, text: &str) -> Vec<JumpCheck> {
    struct F { t: u64, x: i32, y: i32, gr: Option<bool>, acts: String, decision: String }
    let mut frames = vec![];
    for line in text.lines() {
        let mut parts = line.splitn(3, " | ");
        let (Some(head), Some(decision), acts) = (parts.next(), parts.next(), parts.next().unwrap_or("")) else { continue };
        let mut it = head.split_whitespace();
        let Some(t) = it.next().and_then(|v| v.parse().ok()) else { continue };
        let Some((x, y)) = it.next().and_then(|p| p.strip_prefix("pos=")).and_then(|p| p.split_once(',')) else { continue };
        let (Ok(x), Ok(y)) = (x.parse(), y.parse()) else { continue };
        let gr = match it.next() { Some("gr=Some(true)") => Some(true), Some("gr=Some(false)") => Some(false), _ => None };
        frames.push(F { t, x, y, gr, acts: acts.to_string(), decision: decision.to_string() });
    }
    let mut held: Option<Dir> = None;
    let mut released: Option<(Dir, u64)> = None;
    let mut out = vec![];
    let mut marginal = 0;
    for i in 0..frames.len() {
        let f = &frames[i];
        if f.acts.contains("Move(Some(Left))") { held = Some(Dir::Left) } else if f.acts.contains("Move(Some(Right))") { held = Some(Dir::Right) } else if f.acts.contains("Move(None)") {
            if let Some(d) = held { released = Some((d, f.t)); }
            held = None
        }
        if !f.acts.contains("Tap(\"alt\")") || f.decision.contains("Climb") || f.acts.contains("DropDown") || f.acts.contains("Vert(Some") {
            continue;
        }
        // Take-off from the last grounded read at or before the press.
        let Some(o) = frames[..=i].iter().rev().find(|g| g.gr == Some(true)) else { continue };
        let Some(from_seg) = n.locate_within(o.x, o.y, 4) else { continue };
        let mut airborne = false;
        let mut land = None;
        for g in &frames[i + 1..] {
            if g.t > f.t + 2500 { break; }
            if g.gr == Some(false) { airborne = true; }
            if airborne && g.gr == Some(true) {
                if let Some(s) = n.locate_within(g.x, g.y, 1) {
                    land = Some((g.x, g.y, s));
                    break;
                }
            }
        }
        let Some((lx, ly, ls)) = land else { continue };
        let predicted = n.predict_jump(o.x, o.y, held);
        // The slide after a release lasts ~0.2 s.
        let drift = released.filter(|&(_, t)| f.t.saturating_sub(t) <= 300).map(|(d, _)| d);
        let set = n.predict_jump_set(o.x, o.y, held, drift);
        let matched = set.iter().any(|p| match p {
            Some(ps) => n.same_floor(*ps, ls),
            None => n.same_floor(from_seg, ls),
        });
        if set.len() > 1 {
            marginal += 1;
        }
        out.push(JumpCheck { t: f.t, from: (o.x, o.y), dir: held, real: (lx, ly), real_seg: Some(ls), predicted, from_seg: Some(from_seg), matched, marginal: set.len() > 1 });
    }
    let _ = marginal;
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vert {
    Up,
    Down,
}

/// Within this distance of a maneuver's start point we nudge instead of walking.
/// A ladder whose bottom ends more than this far above the floor is grabbed with a jump: holding Up under one 29 px
/// up (Snail Hunting Ground III) did nothing and two bots stood there.
pub const LADDER_REACH_PX: i32 = 16;
pub const ALIGN_ZONE: i32 = 70;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// How high a standing jump reaches (px). Measured: the Beginner jump rises 78.
    pub jump_height: i32,
    /// Horizontal reach added by a running jump (px).
    pub jump_reach: i32,
    /// Feet may be this far from a segment and still count as standing on it.
    pub stand_tol: i32,
    /// Close enough to a ladder / maneuver point to start it.
    pub align_tol: i32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { jump_height: 72, jump_reach: 60, stand_tol: 14, align_tol: 6 }
    }
}

/// Distance (px) to run in before a take-off window: about 0.27 s at walking speed.
const RUN_UP: i32 = 40;
/// A run-up stretch must stay within this many px of the take-off height.
const RUN_LEVEL: i32 = 25;
/// A take-off up to this far past the window is still taken (the position reading lags by up to ~15 px).
const LATE_JUMP: i32 = 10;
/// Never back off closer than this to the end of a platform: stopping coasts about 15 px.
const RETREAT_EDGE: i32 = 32;

/// Jump physics: rise 77 px with the peak at 0.2775 s are the client's own (Map.wz/Physics.img: jumpSpeed 555,
/// gravityAcc 2000). Sideways 150 px/s with a direction held at take-off: fitted so the predicted landing platform
/// matches 154 of 154 real jumps in Lith Harbor (`neon-loader --nav-check`, 2026-10-25). Walls are checked at the feet.
const AIR_RISE: f64 = 77.0;
const AIR_PEAK_T: f64 = 0.2775;
const AIR_SPEED_DEFAULT: f64 = 150.0;

/// Air speed; `NEON_AIR_SPEED` overrides it (calibration sweeps).
fn air_speed() -> f64 {
    static V: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("NEON_AIR_SPEED").ok().and_then(|v| v.parse().ok()).unwrap_or(AIR_SPEED_DEFAULT))
}
const AIR_DT: f64 = 0.01;
const AIR_MAX_T: f64 = 1.1;
const AIR_BODY_H: f64 = 0.0;
const AIR_HALF_W: f64 = 10.0;
/// The feet must be this far above a wall's top to pass it. Measured: a 67 px lip under a platform (jump rise 78) blocked
/// the character 17 times in a row with a 2 px allowance in the model; block faces of 60 px were climbed every time.
const AIR_WALL_CLEAR: f64 = 12.0;
/// Land at least this far inside a platform's free (cliff) end.
const AIR_LAND_MARGIN: i32 = 8;
/// How far above the feet a rope's bottom may hang and still be caught with a jump: the jump rises 77 px and the
/// hands reach above the feet (L Forest III's way west is a rope ending 72 px above the floor).
const ROPE_GRAB_REACH: i32 = 90;
/// A floor within this many px of a ladder's end counts as that end's floor.
const LADDER_END_PX: i32 = 8;
/// Extra route cost of a jump whose take-off window is a single point.
const PINPOINT_PENALTY: i32 = 400;
/// Distance a running character covers in one client frame (~150 px/s x ~50 ms).
const RUN_FRAME_PX: i32 = 8;
/// How far off its take-off point the bot may be when it jumps.
const JUMP_ALIGN_ERR: i32 = 3;
/// Take-off points are tried every JUMP_STEP px; a take-off range is at most JUMP_WINDOW px wide.
const JUMP_STEP: i32 = 4;
const JUMP_WINDOW: i32 = 16;
/// A jump may land this far below where it started (jumping a gap).
const JUMP_MAX_DROP: i32 = 300;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Link {
    Walk,
    /// Walk off the end in this direction and fall.
    Fall(Dir),
    /// Stand at the x, hold Down and jump.
    Drop,
    /// Climb the ladder (index) up or down.
    Climb(usize, Vert),
    /// Jump, holding this direction if any.
    Jump(Option<Dir>),
}

#[derive(Debug, Clone, Copy)]
struct Edge {
    to: usize,
    /// Cost of the maneuver itself, excluding walking to where it starts.
    base: i32,
    link: Link,
    /// Where the maneuver can start (x range on the source segment).
    xlo: i32,
    xhi: i32,
    /// Where we end up standing on `to`.
    land: i32,
}

impl Edge {
    fn at(to: usize, base: i32, link: Link, x: i32, land: i32) -> Edge {
        Edge { to, base, link, xlo: x, xhi: x, land }
    }
    fn over(to: usize, base: i32, link: Link, xlo: i32, xhi: i32) -> Edge {
        Edge { to, base, link, xlo, xhi, land: (xlo + xhi) / 2 }
    }
    /// The x to start from, given where the character is.
    fn start_x(&self, x: i32) -> i32 {
        x.clamp(self.xlo, self.xhi)
    }
    /// Cost to take this edge starting from x.
    fn start_x_pub(&self, x: i32) -> i32 {
        self.start_x(x)
    }
    fn cost_from(&self, x: i32) -> i32 {
        (self.start_x(x) - x).abs() + self.base
    }
}

#[derive(Debug, Clone, Copy)]
struct Seg {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    no_drop: bool,
}

impl Seg {
    fn y_at(&self, x: i32) -> i32 {
        if self.x2 == self.x1 {
            return self.y1;
        }
        let t = (x - self.x1) as f64 / (self.x2 - self.x1) as f64;
        (self.y1 as f64 + t * (self.y2 - self.y1) as f64).round() as i32
    }
    fn spans(&self, x: i32, pad: i32) -> bool {
        x >= self.x1 - pad && x <= self.x2 + pad
    }
    fn mid(&self) -> i32 {
        (self.x1 + self.x2) / 2
    }
}

/// One step of a planned route.
#[derive(Debug, Clone)]
pub struct RouteStep {
    pub from: usize,
    pub to: usize,
    pub kind: String,
    pub start: i32,
    pub land: i32,
    pub from_y: i32,
    pub to_y: i32,
    /// The take-off window.
    pub lo: i32,
    pub hi: i32,
}

/// What the character should do right now to follow the route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    /// Already at the goal.
    Arrived,
    /// Hold this direction.
    Walk(Dir),
    /// Close to the spot a maneuver needs: nudge this way in small steps (distance in px)
    /// instead of holding a key, which overshoots and then reverses.
    Align(Dir, i32),
    /// Release everything and wait (airborne, or aligned and about to act).
    Stop,
    /// Jump now, holding a direction if given.
    Jump(Option<Dir>),
    /// Past a take-off window: walk this way until this x to get a run-up, then come back at speed.
    Retreat(Dir, i32),
    /// In the take-off window: jump in this direction, but only from a running start; `.1` is the staging x to back
    /// off to for a run-up if there is no momentum yet.
    RunJump(Dir, i32),
    /// Hold Down and tap jump to drop through the platform.
    DropThrough,
    /// Hold this vertical key (on or about to grab a ladder/rope; `grab` = tap jump too).
    Climb { dir: Vert, grab: bool },
    /// No route from here to the goal.
    NoRoute,
}

pub struct Nav {
    segs: Vec<Seg>,
    edges: Vec<Vec<Edge>>,
    /// (from segment, edge index) of every edge, by target segment.
    preds: Vec<Vec<(usize, usize)>>,
    pub ladders: Vec<Ladder>,
    /// Vertical footholds (x, top y, bottom y): a wall rising from a platform's end stops you walking off it.
    walls: Vec<(i32, i32, i32)>,
    /// Walkable-run id per segment: segments joined end to end share one (you walk between them, never jump).
    walk_comp: Vec<usize>,
    /// x extent of the whole walkable run each segment belongs to (a floor is often split into short segments).
    run_span: Vec<(i32, i32)>,
    tune: Tuning,
}

impl Nav {
    pub fn new(geom: &MapGeom, tune: Tuning) -> Nav {
        // Walls (zero-width) are not floors.
        let segs: Vec<Seg> = geom
            .footholds
            .iter()
            .filter(|f| f.x1 != f.x2)
            .map(|f| {
                let (a, b) = if f.x1 <= f.x2 { ((f.x1, f.y1), (f.x2, f.y2)) } else { ((f.x2, f.y2), (f.x1, f.y1)) };
                Seg { x1: a.0, y1: a.1, x2: b.0, y2: b.1, no_drop: f.no_drop }
            })
            .collect();
        let walls = geom.footholds.iter().filter(|f| f.x1 == f.x2).map(|f| (f.x1, f.y1.min(f.y2), f.y1.max(f.y2))).collect();
        let mut nav = Nav { edges: vec![vec![]; segs.len()], preds: vec![vec![]; segs.len()], segs, ladders: geom.ladders.clone(), walls, walk_comp: vec![], run_span: vec![], tune };
        nav.build();
        nav
    }

    pub fn segment_count(&self) -> usize {
        self.segs.len()
    }

    fn build(&mut self) {
        let n = self.segs.len();
        let t = self.tune;
        let mut edges: Vec<Vec<Edge>> = vec![vec![]; n];

        // Which ends are connected to another segment.
        let mut left_linked = vec![false; n];
        let mut right_linked = vec![false; n];
        for a in 0..n {
            for b in 0..n {
                if a == b {
                    continue;
                }
                let (sa, sb) = (self.segs[a], self.segs[b]);
                if close(sa.x2, sa.y2, sb.x1, sb.y1) {
                    right_linked[a] = true;
                    left_linked[b] = true;
                    edges[a].push(Edge::at(b, 1, Link::Walk, sa.x2, sb.x1));
                    edges[b].push(Edge::at(a, 1, Link::Walk, sb.x1, sa.x2));
                }
            }
        }

        // Which segments are one walkable run: union the ones joined end to end.
        let mut comp: Vec<usize> = (0..n).collect();
        for a in 0..n {
            for e in &edges[a] {
                if e.link == Link::Walk {
                    let (ca, cb) = (comp[a], comp[e.to]);
                    if ca != cb {
                        for c in comp.iter_mut() {
                            if *c == cb {
                                *c = ca;
                            }
                        }
                    }
                }
            }
        }
        let mut span = vec![(i32::MAX, i32::MIN); n];
        for i in 0..n {
            let c = comp[i];
            span[c].0 = span[c].0.min(self.segs[i].x1);
            span[c].1 = span[c].1.max(self.segs[i].x2);
        }
        self.run_span = (0..n).map(|i| span[comp[i]]).collect();
        self.walk_comp = comp;

        for a in 0..n {
            let sa = self.segs[a];

            // Falling off a free end.
            for (dir, ex, ey, linked) in [(Dir::Left, sa.x1, sa.y1, left_linked[a]), (Dir::Right, sa.x2, sa.y2, right_linked[a])] {
                if linked || self.walled(ex, ey) {
                    continue;
                }
                let probe = ex + if dir == Dir::Left { -8 } else { 8 };
                if let Some(b) = self.below(probe, ey + 2, None) {
                    let fall = self.segs[b].y_at(probe) - ey;
                    edges[a].push(Edge::at(b, 30 + fall / 4, Link::Fall(dir), ex, probe.clamp(self.segs[b].x1, self.segs[b].x2)));
                }
            }

            // Dropping through the platform: one edge per distinct segment below,
            // usable anywhere along the stretch where that segment is underneath.
            if !sa.no_drop {
                let mut runs: Vec<(usize, i32, i32)> = vec![];
                let mut x = sa.x1 + 12;
                while x <= sa.x2 - 12 {
                    if let Some(b) = self.below(x, sa.y_at(x) + 8, Some(a)) {
                        match runs.iter_mut().find(|r| r.0 == b) {
                            Some(r) => r.2 = x,
                            None => runs.push((b, x, x)),
                        }
                    }
                    x += 8;
                }
                for (b, lo, hi) in runs {
                    let fall = self.segs[b].y_at((lo + hi) / 2) - sa.y_at((lo + hi) / 2);
                    edges[a].push(Edge::over(b, 60 + fall / 4, Link::Drop, lo, hi));
                }
            }

            // Jumps: simulate one from every take-off point and keep the ranges that land safely on another platform.
            for (to, dir, lo, hi, land, rise) in self.jump_edges(a, &left_linked, &right_linked) {
                let reach = (land - (lo + hi) / 2).abs();
                // A take-off window of a single sampled point is marginal and hard to hit (the bot shuffled around one
                // for minutes): keep it only as a last resort.
                let pinpoint = if hi - lo < JUMP_STEP { PINPOINT_PENALTY } else { 0 };
                edges[a].push(Edge { to, base: 45 + rise.max(0) + reach / 3 + pinpoint, link: Link::Jump(dir), xlo: lo, xhi: hi, land });
            }
        }

        // Ladders and ropes connect every pair of segments they reach.
        for (li, l) in self.ladders.iter().enumerate() {
            let mut attached = self.ladder_segments(l);
            attached.sort_by_key(|&i| self.segs[i].y_at(l.x.clamp(self.segs[i].x1, self.segs[i].x2)));
            for (ai, &a) in attached.iter().enumerate() {
                for &b in &attached[ai + 1..] {
                    let (ya, yb) = (self.y_on(a, l.x), self.y_on(b, l.x));
                    if yb - ya < 20 {
                        continue; // same level
                    }
                    let climb = (yb - ya) * 14 / 10 + 40;
                    edges[b].push(Edge::at(a, climb, Link::Climb(li, Vert::Up), l.x, l.x));
                    edges[a].push(Edge::at(b, climb, Link::Climb(li, Vert::Down), l.x, l.x));
                }
            }
        }
        let mut preds = vec![vec![]; n];
        for (u, es) in edges.iter().enumerate() {
            for (ei, e) in es.iter().enumerate() {
                preds[e.to].push((u, ei));
            }
        }
        self.preds = preds;
        self.edges = edges;
    }

    /// Segments a ladder/rope can be entered from or left onto. The bottom reaches a little
    /// below its drawn end: a character standing on the floor grabs it with their body.
    fn ladder_segments(&self, l: &Ladder) -> Vec<usize> {
        (0..self.segs.len())
            .filter(|&i| {
                let s = self.segs[i];
                s.spans(l.x, 4) && {
                    let y = s.y_at(l.x.clamp(s.x1, s.x2));
                    y >= l.y1 - 16 && y <= l.y2 + ROPE_GRAB_REACH
                }
            })
            .collect()
    }

    fn y_on(&self, i: usize, x: i32) -> i32 {
        let s = self.segs[i];
        s.y_at(x.clamp(s.x1, s.x2))
    }

    /// The nearest segment strictly below (x, y), optionally excluding one.
    fn below(&self, x: i32, y: i32, skip: Option<usize>) -> Option<usize> {
        (0..self.segs.len())
            .filter(|&i| Some(i) != skip && self.segs[i].spans(x, 0))
            .filter(|&i| self.segs[i].y_at(x) > y)
            .min_by_key(|&i| self.segs[i].y_at(x))
    }

    /// The segment the character is standing on.
    /// How far (px) the floor goes on from (x, y) in direction `d` before it ends: following pieces that join end to
    /// end (a slope continuing into a flat piece). None when (x, y) is not on a floor.
    pub fn edge_ahead(&self, x: i32, y: i32, d: Dir) -> Option<i32> {
        let mut cur = self.locate(x, y)?;
        for _ in 0..64 {
            let s = &self.segs[cur];
            let (lo, hi) = (s.x1.min(s.x2), s.x1.max(s.x2));
            let end = if d == Dir::Right { hi } else { lo };
            let end_y = s.y_at(end);
            let next = self.segs.iter().enumerate().find(|(i, n)| {
                let (nlo, nhi) = (n.x1.min(n.x2), n.x1.max(n.x2));
                *i != cur && nhi > nlo && (if d == Dir::Right { (nlo - end).abs() <= 2 && nhi > end } else { (nhi - end).abs() <= 2 && nlo < end }) && (n.y_at(end) - end_y).abs() <= 6
            });
            match next {
                Some((i, _)) => cur = i,
                None => return Some((end - x).abs()),
            }
        }
        None
    }

    pub fn locate(&self, x: i32, y: i32) -> Option<usize> {
        self.locate_within(x, y, self.tune.stand_tol)
    }

    /// Like `locate` with a custom tolerance (mobs hover a little, so use a bigger one).
    /// Where a jump taken from (x, y) lands by the navigator's model: (segment, x). `None` means it comes
    /// back down on the platform it left (or lands nowhere).
    pub fn predict_jump(&self, x: i32, y: i32, dir: Option<Dir>) -> Option<(usize, i32)> {
        let a = self.locate_within(x, y, 8)?;
        let linked = vec![true; self.segs.len()];
        self.simulate_jump(a, x, dir, air_speed(), &linked, &linked)
    }

    /// Are two segments parts of one walkable floor (joined end to end)?
    pub fn same_floor(&self, a: usize, b: usize) -> bool {
        self.walk_comp[a] == self.walk_comp[b]
    }

    /// Every landing a jump from (x, y) can have across the spread of real jumps (air speed +-5 %, take-off +-3 px):
    /// the floors it may end on (`None` = back on the platform it left). More than one entry means a marginal jump.
    /// `drift`: a direction released just before the jump, whose momentum the character may still carry.
    pub fn predict_jump_set(&self, x: i32, y: i32, dir: Option<Dir>, drift: Option<Dir>) -> Vec<Option<usize>> {
        let Some(a) = self.locate_within(x, y, 8) else { return vec![] };
        let linked = vec![true; self.segs.len()];
        let mut out: Vec<Option<usize>> = vec![];
        // A running character covers up to one more client frame (~8 px) between the last read and leaving the ground.
        let lead = match dir { Some(Dir::Left) => -RUN_FRAME_PX, Some(Dir::Right) => RUN_FRAME_PX, None => 0 };
        let mut tries: Vec<(Option<Dir>, f64, i32)> =
            [(1.0, 0), (0.95, 0), (1.05, 0), (1.0, -JUMP_ALIGN_ERR), (1.0, JUMP_ALIGN_ERR), (1.0, lead)].iter().map(|&(v, dx)| (dir, v, dx)).collect();
        if dir.is_none() {
            if let Some(d) = drift {
                // Momentum left over from a released direction: some fraction of walking speed, decaying.
                tries.extend([(Some(d), 0.25, 0), (Some(d), 0.5, 0), (Some(d), 0.75, 0)]);
            }
        }
        for (dir, v, dx) in tries {
            let xs = (x + dx).clamp(self.segs[a].x1, self.segs[a].x2);
            let r = self.simulate_jump(a, xs, dir, air_speed() * v, &linked, &linked).map(|(b, _)| b);
            let r = r.filter(|&b| !self.same_floor(a, b));
            if !out.iter().any(|o| match (o, r) { (Some(p), Some(q)) => self.same_floor(*p, q), (None, None) => true, _ => false }) {
                out.push(r);
            }
        }
        out
    }

    /// Height of segment `i` at `x`.
    pub fn seg_y(&self, i: usize, x: i32) -> i32 {
        self.y_on(i, x)
    }

    pub fn locate_within(&self, x: i32, y: i32, tol: i32) -> Option<usize> {
        (0..self.segs.len())
            .filter(|&i| self.segs[i].spans(x, 2))
            .map(|i| (i, (self.y_on(i, x) - y).abs()))
            .filter(|&(_, d)| d <= tol)
            .min_by_key(|&(_, d)| d)
            .map(|(i, _)| i)
    }

    /// A platform to stand on near (tx, ty): within `tol` px vertically, reachable from `from` (at x) and able to get
    /// back. The lowest such platform wins: an NPC's own booth or ledge sits above the street it faces, and standing
    /// inside it (Phil's ticket booth) traps the bot against its walls.
    pub fn stand_near(&self, from: usize, x: i32, tx: i32, ty: i32, tol: i32) -> Option<usize> {
        let mut cands: Vec<(i32, usize)> = (0..self.segs.len())
            .filter(|&i| self.segs[i].spans(tx, 2))
            .map(|i| (self.y_on(i, tx), i))
            .filter(|&(y, _)| (y - ty).abs() <= tol)
            .collect();
        cands.sort_by_key(|&(y, i)| (Reverse(y), i));
        cands.into_iter().map(|(_, i)| i).find(|&i| i == from || (self.cost(from, x, i).is_some() && self.cost(i, tx, from).is_some()))
    }

    /// Where a jump from standing at `x0` on segment `a` comes down, holding `dir` from take-off: the segment and x it lands
    /// on, or None if it lands back on `a`, hits a wall it cannot clear, or never lands. The arc is the measured one
    /// (rise 78 px, peak 0.26 s, hang 0.52 s, 170 px/s sideways in the air), walls stop the body until the feet clear
    /// their top, and platforms are one-way: they only catch a falling body.
    fn simulate_jump(&self, a: usize, x0: i32, dir: Option<Dir>, speed: f64, left_linked: &[bool], right_linked: &[bool]) -> Option<(usize, i32)> {
        let y0 = self.y_on(a, x0) as f64;
        let sign = match dir { Some(Dir::Left) => -1.0, Some(Dir::Right) => 1.0, None => 0.0 };
        // The body cannot stand overlapping a wall: take-off points that close are not real.
        if self.walls.iter().any(|&(wx, top, bottom)| (x0 - wx).abs() < AIR_HALF_W as i32 && y0 > top as f64 - AIR_WALL_CLEAR && y0 - AIR_BODY_H <= bottom as f64 + 0.5) {
            return None;
        }
        let k = AIR_RISE / (AIR_PEAK_T * AIR_PEAK_T);
        let (mut x, mut y_prev) = (x0 as f64, y0);
        let mut t = 0.0;
        while t < AIR_MAX_T {
            t += AIR_DT;
            let y = y0 - (AIR_RISE - k * (t - AIR_PEAK_T) * (t - AIR_PEAK_T));
            // Sideways, unless a wall is in the way.
            let nx = x + sign * speed * AIR_DT;
            let mut blocked = false;
            if sign != 0.0 {
                for &(wx, top, bottom) in &self.walls {
                    let wx = wx as f64;
                    let crossing = if sign > 0.0 { x + AIR_HALF_W <= wx + 0.5 && nx + AIR_HALF_W > wx } else { x - AIR_HALF_W >= wx - 0.5 && nx - AIR_HALF_W < wx };
                    // The body spans feet-60..feet; it is clear of a wall once the feet are at or above its top.
                    if crossing && y > top as f64 - AIR_WALL_CLEAR && y - AIR_BODY_H <= bottom as f64 + 0.5 {
                        blocked = true;
                        break;
                    }
                }
            }
            if !blocked {
                x = nx;
            }
            // Falling: the first platform whose surface the feet pass through.
            if y > y_prev {
                let mut hit: Option<(usize, f64)> = None;
                for (i, sg) in self.segs.iter().enumerate() {
                    let xi = x.round() as i32;
                    if xi < sg.x1 || xi > sg.x2 {
                        continue;
                    }
                    let ys = sg.y_at(xi) as f64;
                    if y_prev <= ys + 0.5 && y >= ys && hit.map_or(true, |(_, hy)| ys < hy) {
                        hit = Some((i, ys));
                    }
                }
                if let Some((i, _)) = hit {
                    if i == a {
                        return None;
                    }
                    let sg = self.segs[i];
                    let xi = x.round() as i32;
                    // Do not land at a free end of the platform: the next step walks off it.
                    let free_left = !left_linked_at(self, i, true, left_linked, right_linked);
                    let free_right = !left_linked_at(self, i, false, left_linked, right_linked);
                    let m = AIR_LAND_MARGIN.min((sg.x2 - sg.x1) / 2);
                    if (free_left && xi < sg.x1 + m) || (free_right && xi > sg.x2 - m) {
                        return None;
                    }
                    return Some((i, xi));
                }
            }
            y_prev = y;
        }
        None
    }

    /// Jump edges out of segment `a`: (to, direction, take-off x range, landing x, rise).
    fn jump_edges(&self, a: usize, left_linked: &[bool], right_linked: &[bool]) -> Vec<(usize, Option<Dir>, i32, i32, i32, i32)> {
        let sa = self.segs[a];
        let mut found: Vec<((usize, Option<Dir>), Vec<(i32, i32)>)> = vec![];
        let mut x0 = sa.x1;
        while x0 <= sa.x2 {
            for dir in [None, Some(Dir::Left), Some(Dir::Right)] {
                // A jump counts only if it lands on the same platform across the spread of real jumps: air speed
                // +-6 % and a take-off position off by a few px (the bot aligns to about +-6 px).
                let nominal = self.simulate_jump(a, x0, dir, air_speed(), left_linked, right_linked);
                let robust = nominal.filter(|&(b, _)| {
                    [
                        (air_speed() * 0.95, 0),
                        (air_speed() * 1.05, 0),
                        (air_speed(), -JUMP_ALIGN_ERR),
                        (air_speed(), JUMP_ALIGN_ERR),
                        // No one-frame lead here: the bot presses planned jumps early (brain.rs PRESS_LEAD_MS) for it.
                    ]
                        .iter()
                        .all(|&(v, dx)| {
                            let x = (x0 + dx).clamp(sa.x1, sa.x2);
                            self.simulate_jump(a, x, dir, v, left_linked, right_linked).map_or(false, |(b2, _)| b2 == b)
                        })
                });
                if let Some((b, land)) = robust {
                    let rise = self.y_on(a, x0) - self.y_on(b, land);
                    if rise >= -JUMP_MAX_DROP && rise <= self.tune.jump_height + 6 && !(rise.abs() < 6 && self.walk_comp[a] == self.walk_comp[b]) {
                        match found.iter_mut().find(|(k, _)| *k == (b, dir)) {
                            Some((_, v)) => v.push((x0, land)),
                            None => found.push(((b, dir), vec![(x0, land)])),
                        }
                    }
                }
            }
            x0 += JUMP_STEP;
        }
        let mut out = vec![];
        for ((b, dir), pts) in found {
            // Contiguous runs of take-off points; use the middle of each run, never its marginal ends.
            let mut start = 0;
            for i in 0..=pts.len() {
                if i == pts.len() || pts[i].0 - pts[i.saturating_sub(1)].0 > JUMP_STEP + 1 {
                    let run = &pts[start..i];
                    if !run.is_empty() {
                        let (lo, hi) = (run[0].0, run[run.len() - 1].0);
                        let w = hi - lo;
                        let (mut l, mut h) = if w >= 3 * JUMP_STEP { (lo + w / 4, hi - w / 4) } else { ((lo + hi) / 2, (lo + hi) / 2) };
                        if h - l > JUMP_WINDOW {
                            let c = (l + h) / 2;
                            l = c - JUMP_WINDOW / 2;
                            h = c + JUMP_WINDOW / 2;
                        }
                        let mid = run.iter().min_by_key(|p| (p.0 - (l + h) / 2).abs()).unwrap();
                        let rise = self.y_on(a, mid.0) - self.y_on(b, mid.1);
                        out.push((b, dir, l, h, mid.1, rise));
                    }
                    start = i;
                }
            }
        }
        out
    }

    fn joined_flat(&self, a: usize, b: usize) -> bool {
        let (sa, sb) = (self.segs[a], self.segs[b]);
        sa.y1 == sa.y2 && sb.y1 == sb.y2 && sa.y1 == sb.y1 && (close(sa.x2, sa.y2, sb.x1, sb.y1) || close(sb.x2, sb.y2, sa.x1, sa.y1))
    }

    /// Is segment `a` (flat) continued past this end by another flat segment at the same height? Then the end is
    /// just a joint of a split floor, not a take-off. A slope or a step at the joint is a real edge.
    fn flat_continuation(&self, a: usize, right: bool) -> bool {
        let sa = self.segs[a];
        if sa.y1 != sa.y2 {
            return false;
        }
        let (x, y) = if right { (sa.x2, sa.y2) } else { (sa.x1, sa.y1) };
        self.segs.iter().enumerate().any(|(c, sc)| c != a && sc.y1 == sc.y2 && sc.y1 == y && if right { close(sc.x1, sc.y1, x, y) } else { close(sc.x2, sc.y2, x, y) })
    }

    /// x extent of the walkable run through segment `cur` that stays level with the take-off point at `x` (within RUN_LEVEL px):
    /// the run-up space for a jump. A run is often cut into short segments and may continue as a slope; a slope that leads
    /// down (or up) away from the take-off height is not room to run, only the level stretch is.
    fn level_run(&self, cur: usize, x: i32) -> (i32, i32) {
        let y0 = self.y_on(cur, x);
        let (mut left, mut right) = (self.segs[cur], self.segs[cur]);
        let (mut x1, mut x2) = (left.x1, right.x2);
        for _ in 0..self.segs.len() {
            let Some(b) = self.segs.iter().find(|b| close(b.x2, b.y2, left.x1, left.y1) && (b.y1 - y0).abs() <= RUN_LEVEL && (b.y2 - y0).abs() <= RUN_LEVEL) else { break };
            left = *b;
            x1 = b.x1;
        }
        for _ in 0..self.segs.len() {
            let Some(b) = self.segs.iter().find(|b| close(b.x1, b.y1, right.x2, right.y2) && (b.y1 - y0).abs() <= RUN_LEVEL && (b.y2 - y0).abs() <= RUN_LEVEL) else { break };
            right = *b;
            x2 = b.x2;
        }
        (x1, x2)
    }

    /// Does a wall rise from the platform end at (x, y)? (A cliff wall hangs below a ledge instead.)
    fn walled(&self, x: i32, y: i32) -> bool {
        self.walls.iter().any(|&(wx, top, bottom)| (wx - x).abs() <= 2 && (bottom - y).abs() <= 6 && top < y - 12)
    }

    /// The segment (and x) to plan from: where we stand, or the nearest end of the ladder we hang on.
    pub fn anchor(&self, x: i32, y: i32, grounded: bool) -> Option<(usize, i32)> {
        if grounded {
            if let Some(c) = self.locate(x, y) {
                return Some((c, x));
            }
        }
        let l = &self.ladders[self.ladder_at(x, y)?];
        self.ladder_segments(l).into_iter().min_by_key(|&i| (self.y_on(i, l.x) - y).abs()).map(|i| (i, l.x))
    }

    /// Where to wait out low HP: the nearest rope/ladder at least `min_len` px long, ground mobs cannot
    /// follow you up it. Returns (segment at its foot, x, ladder index).
    pub fn refuge(&self, x: i32, y: i32, grounded: bool, min_len: i32) -> Option<(usize, i32, usize)> {
        let (seg, x0) = self.anchor(x, y, grounded)?;
        let costs = self.costs_from(seg, x0);
        self.ladders
            .iter()
            .enumerate()
            .filter(|(_, l)| l.y2 - l.y1 >= min_len)
            .filter_map(|(li, l)| {
                // The segment that reaches the bottom of this ladder.
                let foot = self.ladder_segments(l).into_iter().max_by_key(|&i| self.y_on(i, l.x))?;
                let c = costs[foot]? + (x0 - l.x).abs() / 2;
                Some((c, foot, l.x, li))
            })
            .min_by_key(|t| t.0)
            .map(|(_, foot, lx, li)| (foot, lx, li))
    }

    /// (left x, middle x) of segment `i`.
    pub fn segment_span(&self, i: usize) -> (i32, i32) {
        (self.segs[i].x1, self.segs[i].mid())
    }

    /// The platform at the top of ladder `li`: somewhere to stand (HP does not regenerate on a rope).
    pub fn ladder_top(&self, li: usize) -> Option<usize> {
        let l = self.ladders.get(li)?;
        self.ladder_segments(l).into_iter().min_by_key(|&i| self.y_on(i, l.x))
    }

    /// One planned maneuver, for inspection and tests.
    pub fn route(&self, from: usize, x: i32, goal: usize) -> Vec<RouteStep> {
        let (mut cur, mut x, mut out) = (from, x, vec![]);
        while cur != goal && out.len() < 300 {
            let Some(e) = self.first_edge(cur, x, goal) else { break };
            let (a, b) = (self.segs[cur], self.segs[e.to]);
            out.push(RouteStep {
                from: cur,
                to: e.to,
                kind: format!("{:?}", e.link),
                start: e.start_x_pub(x),
                land: e.land,
                from_y: a.y_at(e.start_x_pub(x).clamp(a.x1, a.x2)),
                to_y: b.y_at(e.land.clamp(b.x1, b.x2)),
                lo: e.xlo,
                hi: e.xhi,
            });
            x = e.land;
            cur = e.to;
        }
        out
    }

    /// The highest platform reachable from (`from`, `x`): smallest y, nearest first.
    pub fn highest_reachable(&self, from: usize, x: i32) -> Option<usize> {
        let costs = self.costs_from(from, x);
        (0..self.segs.len()).filter(|&i| costs[i].is_some()).min_by_key(|&i| (self.segs[i].y1.min(self.segs[i].y2), costs[i]))
    }

    /// (x1, y1, x2, y2) of a segment.
    pub fn segment(&self, i: usize) -> (i32, i32, i32, i32) {
        let s = self.segs[i];
        (s.x1, s.y1, s.x2, s.y2)
    }

    /// The segment at the foot of ladder `li`.
    pub fn ladder_foot(&self, li: usize) -> Option<usize> {
        let l = self.ladders.get(li)?;
        self.ladder_segments(l).into_iter().max_by_key(|&i| self.y_on(i, l.x))
    }

    /// Ladder the character is on (or at the foot/top of), if any.
    pub fn ladder_at(&self, x: i32, y: i32) -> Option<usize> {
        self.ladders
            .iter()
            .position(|l| (l.x - x).abs() <= 8 && y >= l.y1 - 12 && y <= l.y2 + ROPE_GRAB_REACH)
    }

    /// Travel cost from standing at (`cur`, `x`) to every segment (None = unreachable).
    /// States are the edges themselves, so each move is costed from where the last one landed.
    pub fn costs_from(&self, cur: usize, x: i32) -> Vec<Option<i32>> {
        let n = self.segs.len();
        let mut best: Vec<Option<i32>> = vec![None; n];
        best[cur] = Some(0);
        let mut dist: Vec<Vec<i32>> = self.edges.iter().map(|es| vec![i32::MAX; es.len()]).collect();
        let mut heap = BinaryHeap::new();
        for (ei, e) in self.edges[cur].iter().enumerate() {
            let d = e.cost_from(x);
            dist[cur][ei] = d;
            heap.push(Reverse((d, cur, ei)));
        }
        while let Some(Reverse((d, u, ei))) = heap.pop() {
            if d > dist[u][ei] {
                continue;
            }
            let e = self.edges[u][ei];
            if best[e.to].map_or(true, |b| d < b) {
                best[e.to] = Some(d);
            }
            for (ej, e2) in self.edges[e.to].iter().enumerate() {
                let nd = d + e2.cost_from(e.land);
                if nd < dist[e.to][ej] {
                    dist[e.to][ej] = nd;
                    heap.push(Reverse((nd, e.to, ej)));
                }
            }
        }
        best
    }

    /// Cost of the route from (`from`, `x`) to `to`.
    /// Platform number of every floor piece: pieces whose ends meet (within 2 px across, 6 px up or down) belong to the
    /// same platform, the stretch a walking mob can cover.
    pub fn platforms(&self) -> Vec<usize> {
        let n = self.segs.len();
        let mut group: Vec<usize> = (0..n).collect();
        fn root(g: &mut Vec<usize>, i: usize) -> usize {
            let mut r = i;
            while g[r] != r {
                r = g[r];
            }
            g[i] = r;
            r
        }
        for i in 0..n {
            for j in (i + 1)..n {
                let (a, b) = (&self.segs[i], &self.segs[j]);
                let (alo, ahi, blo, bhi) = (a.x1.min(a.x2), a.x1.max(a.x2), b.x1.min(b.x2), b.x1.max(b.x2));
                let joined = [(ahi, blo), (bhi, alo)].iter().any(|&(e, f)| (e - f).abs() <= 2 && (a.y_at(e.clamp(alo, ahi)) - b.y_at(f.clamp(blo, bhi))).abs() <= 6);
                if joined {
                    let (ri, rj) = (root(&mut group, i), root(&mut group, j));
                    group[ri] = rj;
                }
            }
        }
        (0..n).map(|i| root(&mut group, i)).collect()
    }

    /// The nearest place to stand (by route cost from `from`, at x) on a platform where no mob spawns: walking mobs
    /// stay on the platform they spawned on, so nothing comes there. `spawns` are spawn points (x, y). Only platforms
    /// at least `min_width` px wide count.
    pub fn safe_spot(&self, from: usize, x: i32, spawns: &[(i32, i32)], min_width: i32) -> Option<(usize, i32, i32)> {
        let group = self.platforms();
        let mut unsafe_groups = std::collections::HashSet::new();
        for &(sx, sy) in spawns {
            if let Some(i) = self.locate_within(sx, sy, 12) {
                unsafe_groups.insert(group[i]);
            }
        }
        let mut width: std::collections::HashMap<usize, i32> = Default::default();
        for (i, s) in self.segs.iter().enumerate() {
            *width.entry(group[i]).or_default() += (s.x2 - s.x1).abs();
        }
        let costs = self.costs_from(from, x);
        (0..self.segs.len())
            .filter(|&i| !unsafe_groups.contains(&group[i]) && width[&group[i]] >= min_width && (self.segs[i].x2 - self.segs[i].x1).abs() >= 20)
            .filter_map(|i| costs[i].map(|c| (c, i)))
            .min()
            .map(|(_, i)| {
                let s = &self.segs[i];
                let mx = s.mid();
                (i, mx, s.y_at(mx))
            })
    }

    pub fn cost(&self, from: usize, x: i32, to: usize) -> Option<i32> {
        self.costs_from(from, x)[to]
    }

    /// Remaining cost after taking each edge, for a given goal: `h[u][ei]`.
    fn costs_to(&self, goal: usize) -> Vec<Vec<i32>> {
        let mut h: Vec<Vec<i32>> = self.edges.iter().map(|es| vec![i32::MAX; es.len()]).collect();
        let mut heap = BinaryHeap::new();
        for (u, es) in self.edges.iter().enumerate() {
            for (ei, e) in es.iter().enumerate() {
                if e.to == goal {
                    h[u][ei] = 0;
                    heap.push(Reverse((0, u, ei)));
                }
            }
        }
        while let Some(Reverse((d, u, ei))) = heap.pop() {
            if d > h[u][ei] {
                continue;
            }
            // Edges leading into `u` can be followed by this one.
            let e = self.edges[u][ei];
            for &(pu, pei) in &self.preds[u] {
                let pe = self.edges[pu][pei];
                let nd = d + e.cost_from(pe.land);
                if nd < h[pu][pei] {
                    h[pu][pei] = nd;
                    heap.push(Reverse((nd, pu, pei)));
                }
            }
        }
        h
    }

    /// The edge to take from `cur` at horizontal position `x` to reach `goal`.
    fn first_edge(&self, cur: usize, x: i32, goal: usize) -> Option<Edge> {
        let h = self.costs_to(goal);
        self.edges[cur]
            .iter()
            .enumerate()
            .filter(|(ei, _)| h[cur][*ei] != i32::MAX)
            .min_by_key(|(ei, e)| e.cost_from(x) + h[cur][*ei])
            .map(|(_, e)| *e)
    }

    /// Where should the character head next to reach `goal_x` on segment `goal`?
    /// `grounded` is the character's foothold flag (false while airborne or on a rope).
    pub fn step(&self, x: i32, y: i32, grounded: bool, goal: usize, goal_x: i32, tol: i32) -> Cmd {
        let t = self.tune;

        // On a ladder/rope: keep climbing toward whichever end gets us closer to the goal.
        if !grounded {
            if let Some(li) = self.ladder_at(x, y) {
                let l = &self.ladders[li];
                let ends = self.ladder_segments(l);
                let mut best: Option<(i32, Vert)> = None;
                for i in ends {
                    let cost = if i == goal { Some((self.y_on(i, l.x) - y).abs()) } else { self.cost(i, l.x, goal).map(|c| c + (self.y_on(i, l.x) - y).abs()) };
                    if let Some(c) = cost {
                        // A floor at the ladder's top is reached by holding Up past the top (and the bottom one by
                        // holding Down); comparing heights there flipped Up/Down every frame as the read wobbled a few
                        // px around the floor (stuck on a ladder top in 3-Way Road-Split).
                        let fy = self.y_on(i, l.x);
                        let dir = if fy <= l.y1 + LADDER_END_PX {
                            Vert::Up
                        } else if fy >= l.y2 - LADDER_END_PX {
                            Vert::Down
                        } else if fy < y {
                            Vert::Up
                        } else {
                            Vert::Down
                        };
                        if best.map_or(true, |(bc, _)| c < bc) {
                            best = Some((c, dir));
                        }
                    }
                }
                return match best {
                    Some((_, dir)) => Cmd::Climb { dir, grab: false },
                    None => Cmd::NoRoute,
                };
            }
            return Cmd::Stop; // airborne: nothing useful to press
        }

        let Some(cur) = self.locate(x, y) else { return Cmd::Stop };
        if cur == goal {
            let dx = goal_x - x;
            return if dx.abs() <= tol { Cmd::Arrived } else { Cmd::Walk(if dx > 0 { Dir::Right } else { Dir::Left }) };
        }
        let Some(e) = self.first_edge(cur, x, goal) else { return Cmd::NoRoute };
        let ex = e.start_x_pub(x);
        let dx = ex - x;
        let toward = if dx > 0 { Dir::Right } else { Dir::Left };
        match e.link {
            // Across a joint between two pieces of floor, keep going the way the next piece lies. Heading for the joint
            // point itself turned the character around when it was already a pixel or two past it (the lookup allows
            // 2 px of slop), which reset its running start.
            Link::Walk => Cmd::Walk(if self.segs[e.to].mid() >= self.segs[cur].mid() { Dir::Right } else { Dir::Left }),
            // Keep pressing past the end so we actually leave the platform.
            Link::Fall(dir) => Cmd::Walk(dir),
            Link::Drop => {
                if dx.abs() > t.align_tol {
                    approach(toward, dx.abs())
                } else {
                    Cmd::DropThrough
                }
            }
            Link::Climb(li, dir) => {
                if dx.abs() > t.align_tol {
                    approach(toward, dx.abs())
                } else {
                    Cmd::Climb { dir, grab: dir == Vert::Up && (!self.ladders[li].ladder || y - self.ladders[li].y2 > LADDER_REACH_PX) }
                }
            }
            Link::Jump(None) => {
                if dx.abs() > t.align_tol {
                    approach(toward, dx.abs())
                } else {
                    Cmd::Jump(None)
                }
            }
            Link::Jump(Some(dir)) => {
                // Run into the take-off window moving in the jump direction and jump as it is entered: jumping from a
                // run places the take-off within a few px, where stop-and-nudge alignment jittered and missed.
                let (lo, hi) = (e.xlo, e.xhi);
                let run = self.level_run(cur, (lo + hi) / 2);
                // Where to back off to for a run-up: RUN_UP before the window, clear of the end of the whole walkable run.
                let staging = if dir == Dir::Right { (lo - RUN_UP).max(run.0 + RETREAT_EDGE) } else { (hi + RUN_UP).min(run.1 - RETREAT_EDGE) };
                match dir {
                    Dir::Right if x < lo => Cmd::Walk(Dir::Right),
                    Dir::Left if x > hi => Cmd::Walk(Dir::Left),
                    // In the window, or just past it (position readings lag by up to ~15 px): jump from the run.
                    Dir::Right if x <= hi + LATE_JUMP => Cmd::RunJump(Dir::Right, staging),
                    Dir::Left if x >= lo - LATE_JUMP => Cmd::RunJump(Dir::Left, staging),
                    // Well past it: back off for a run-up if the platform has room, else just walk back to the window.
                    Dir::Right => if staging < x - 12 { Cmd::Retreat(Dir::Left, staging) } else { Cmd::Walk(Dir::Left) },
                    Dir::Left => if staging > x + 12 { Cmd::Retreat(Dir::Right, staging) } else { Cmd::Walk(Dir::Right) },
                }
            }
        }
    }
}

/// Walk while far, nudge when close.
fn approach(toward: Dir, dist: i32) -> Cmd {
    if dist <= ALIGN_ZONE {
        Cmd::Align(toward, dist)
    } else {
        Cmd::Walk(toward)
    }
}

/// Is the left (or right) end of segment `i` joined to another segment? (From the flags computed in `build`.)
fn left_linked_at(_n: &Nav, i: usize, left: bool, left_linked: &[bool], right_linked: &[bool]) -> bool {
    if left { left_linked[i] } else { right_linked[i] }
}

fn close(ax: i32, ay: i32, bx: i32, by: i32) -> bool {
    (ax - bx).abs() <= 3 && (ay - by).abs() <= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fh(x1: i32, y1: i32, x2: i32, y2: i32) -> RawFoothold {
        RawFoothold { x1, y1, x2, y2, no_drop: false }
    }

    /// Ground 0..1000 at y=0; a platform 200..600 at y=-300 (too high to jump);
    /// a ladder at x=650 from the ground to y=-300 beside the platform's end; a low
    /// step 800..900 at y=-40 (jumpable); a ledge 100..300 at y=200 below the ground.
    fn geom() -> MapGeom {
        MapGeom {
            footholds: vec![fh(0, 0, 1000, 0), fh(200, -300, 600, -300), fh(800, -40, 900, -40), fh(100, 200, 300, 200)],
            ladders: vec![Ladder { x: 600, y1: -300, y2: 0, ladder: true }],
            portals: vec![],
            mobs: Default::default(), mob_spawns: vec![],
        }
    }

    #[test]
    fn locates_standing_segment() {
        let n = Nav::new(&geom(), Tuning::default());
        assert!(n.locate(500, 0).is_some());
        assert_eq!(n.locate(500, -300), n.locate(400, -300));
        assert_ne!(n.locate(500, 0), n.locate(500, -300));
        assert!(n.locate(500, -150).is_none(), "mid-air is on nothing");
    }

    #[test]
    fn high_platform_is_reached_via_the_ladder_not_a_jump() {
        let n = Nav::new(&geom(), Tuning::default());
        let ground = n.locate(500, 0).unwrap();
        let top = n.locate(400, -300).unwrap();
        assert!(n.cost(ground, 500, top).is_some());
        // Far from the ladder: walk toward it.
        assert_eq!(n.step(100, 0, true, top, 400, 10), Cmd::Walk(Dir::Right));
        // Within the alignment zone of the ladder: small nudges, not a held key.
        assert_eq!(n.step(560, 0, true, top, 400, 10), Cmd::Align(Dir::Right, 40));
        // At the ladder: climb up.
        assert_eq!(n.step(600, 0, true, top, 400, 10), Cmd::Climb { dir: Vert::Up, grab: false });
        // Never asks for a jump to cover 300px.
        for x in (0..1000).step_by(50) {
            assert!(!matches!(n.step(x, 0, true, top, 400, 10), Cmd::Jump(_)));
        }
    }

    #[test]
    fn a_ladder_ending_above_reach_needs_a_grab_jump() {
        let mut g = geom();
        g.ladders[0].y2 = -29;
        let n = Nav::new(&g, Tuning::default());
        let top = n.locate(400, -300).unwrap();
        assert_eq!(n.step(600, 0, true, top, 400, 10), Cmd::Climb { dir: Vert::Up, grab: true });
    }

    #[test]
    fn rope_needs_a_grab_jump() {
        let mut g = geom();
        g.ladders[0].ladder = false;
        let n = Nav::new(&g, Tuning::default());
        let top = n.locate(400, -300).unwrap();
        assert_eq!(n.step(600, 0, true, top, 400, 10), Cmd::Climb { dir: Vert::Up, grab: true });
    }

    #[test]
    fn keeps_climbing_while_on_the_ladder_then_arrives() {
        let n = Nav::new(&geom(), Tuning::default());
        let top = n.locate(400, -300).unwrap();
        assert_eq!(n.step(600, -150, false, top, 400, 10), Cmd::Climb { dir: Vert::Up, grab: false });
        // Reached the top platform: grounded again, now walk to the goal.
        assert_eq!(n.step(600, -300, true, top, 400, 10), Cmd::Walk(Dir::Left));
    }

    #[test]
    fn leaves_the_top_by_falling_when_cheaper() {
        let n = Nav::new(&geom(), Tuning::default());
        let ground = n.locate(500, 0).unwrap();
        // Falling off the platform's end is cheaper than climbing down.
        assert_eq!(n.step(600, -300, true, ground, 700, 10), Cmd::Walk(Dir::Right));
    }

    #[test]
    fn low_step_is_jumped() {
        let n = Nav::new(&geom(), Tuning::default());
        let step = n.locate(850, -40).unwrap();
        assert_eq!(n.step(850, 0, true, step, 850, 10), Cmd::Jump(None));
        assert_eq!(n.step(500, 0, true, step, 850, 10), Cmd::Walk(Dir::Right));
    }

    #[test]
    fn lower_ledge_is_reached_by_falling_off_the_ground_end() {
        let n = Nav::new(&geom(), Tuning::default());
        let ledge = n.locate(200, 200).unwrap();
        // Ground spans 0..1000, ledge below x 100..300: drop through (ground allows it).
        let cmd = n.step(200, 0, true, ledge, 200, 10);
        assert_eq!(cmd, Cmd::DropThrough);
    }

    #[test]
    fn no_drop_platforms_are_not_dropped_through() {
        let mut g = geom();
        g.footholds[0].no_drop = true;
        let n = Nav::new(&g, Tuning::default());
        let ground = n.locate(500, 0).unwrap();
        let ledge = n.locate(200, 200).unwrap();
        // Only reachable by walking off the left end (x<0 has nothing below at 100..300) => unreachable.
        assert!(n.cost(ground, 500, ledge).is_none());
        assert_eq!(n.step(500, 0, true, ledge, 200, 10), Cmd::NoRoute);
    }

    #[test]
    fn same_segment_walks_then_arrives() {
        let n = Nav::new(&geom(), Tuning::default());
        let g = n.locate(500, 0).unwrap();
        assert_eq!(n.step(300, 0, true, g, 500, 10), Cmd::Walk(Dir::Right));
        assert_eq!(n.step(495, 0, true, g, 500, 10), Cmd::Arrived);
    }

    #[test]
    fn linked_segments_walk() {
        let g = MapGeom { footholds: vec![fh(0, 0, 100, 0), fh(100, 0, 200, -20)], ladders: vec![], portals: vec![], mobs: Default::default(), mob_spawns: vec![] };
        let n = Nav::new(&g, Tuning::default());
        let far = n.locate(190, -18).unwrap();
        assert_eq!(n.step(10, 0, true, far, 190, 10), Cmd::Walk(Dir::Right));
    }

    #[test]
    fn real_hunting_ground_is_fully_connected_from_the_ground_floor() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../neon-calib/maps");
        let Some(geom) = MapGeom::load(&dir, 10001010) else { return };
        let n = Nav::new(&geom, Tuning::default());
        let ground = n.locate(1500, 215).expect("arrival point is on a segment");
        let costs = n.costs_from(ground, 1500);
        // The map's four main floors (y=215, -85, -325, -565) must all be reachable.
        for fy in [-85, -325, -565] {
            let reachable = (0..n.segment_count()).any(|i| {
                costs[i].is_some() && n.segs[i].y1 == fy && n.segs[i].y2 == fy
            });
            assert!(reachable, "floor y={fy} unreachable");
        }
    }

    #[test]
    fn a_wall_rising_from_a_platform_end_stops_the_bot_walking_into_it() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../neon-calib/maps");
        let Some(geom) = MapGeom::load(&dir, 10000000) else { return };
        let n = Nav::new(&geom, Tuning::default());
        let street = n.locate(900, 347).expect("the dock east of the booth");
        let olaf = n.stand_near(street, 900, 4100, 427, 60).expect("Olaf's street");
        // Phil's booth floor (y 326, x 741..861) is walled in on both sides: from its right end the plan must
        // not be "keep walking right" (that is what pinned the bot against the wall); it has to drop out of it.
        let cmd = n.step(850, 326, true, olaf, 4100, 6);
        assert_ne!(cmd, Cmd::Walk(Dir::Right), "{cmd:?}");
        assert_ne!(cmd, Cmd::NoRoute);
    }

    #[test]
    fn the_street_in_front_of_phils_booth_is_the_place_to_stand() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../neon-calib/maps");
        let Some(geom) = MapGeom::load(&dir, 10000000) else { return };
        let n = Nav::new(&geom, Tuning::default());
        let street = n.locate(900, 347).expect("the dock east of the booth");
        // Phil stands at (804, 323) on the booth's floor (y 326); the street below is y 347.
        let goal = n.stand_near(street, 900, 804, 323, 60).expect("a place to stand");
        assert_eq!(n.y_on(goal, 804), 347);
    }

    #[test]
    fn a_block_face_is_climbed_by_standing_next_to_it_not_from_the_far_end_of_the_floor() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../neon-calib/maps");
        let Some(geom) = MapGeom::load(&dir, 10000011) else { return };
        let n = Nav::new(&geom, Tuning::default());
        // Floor y=215 from x=579 meets a 60 px block (top y=155, x 501..579). The way up is next to its face.
        let floor = n.locate(600, 215).expect("floor");
        let top = n.locate(540, 155).expect("block top");
        let jump = n.edges[floor].iter().find(|e| e.to == top).expect("an edge up onto the block");
        assert_eq!(jump.link, Link::Jump(Some(Dir::Left)));
        assert!(jump.xlo >= 579 && jump.xhi <= 650 && jump.xhi - jump.xlo <= 16, "take-off window {}..{}", jump.xlo, jump.xhi);
        assert!((jump.land - 560).abs() <= 25 && jump.land < 579, "lands on the block top, got {}", jump.land);
    }

    #[test]
    fn a_run_up_uses_the_whole_walkable_floor_not_just_the_short_piece_under_the_feet() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../neon-calib/maps");
        let Some(geom) = MapGeom::load(&dir, 10000011) else { return };
        let n = Nav::new(&geom, Tuning::default());
        // The y=215 floor is cut into 40-50 px pieces; the block (top y=155, x 501..579) is climbed from the east.
        let floor = n.locate(600, 215).expect("floor piece");
        let top = n.locate(540, 155).expect("block top");
        let e = n.edges[floor].iter().find(|e| e.to == top).expect("edge onto the block");
        // Standing in the take-off window: the plan must be a run-up with real room behind it.
        match n.step((e.xlo + e.xhi) / 2, 215, true, top, 540, 6) {
            Cmd::RunJump(Dir::Left, staging) => assert!(staging >= e.xhi + RUN_UP - 2, "staging {staging} leaves no run-up for window {}..{}", e.xlo, e.xhi),
            other => panic!("expected a running jump, got {other:?}"),
        }
    }

    #[test]
    fn the_run_up_for_the_slope_top_jump_extends_back_along_the_flat_above_the_slope() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../neon-calib/maps");
        let Some(geom) = MapGeom::load(&dir, 10000011) else { return };
        let n = Nav::new(&geom, Tuning::default());
        // The slope (720,-25)-(810,-85) runs into the flat (810..900, y -85); the platform above-left (681..759, y -145)
        // is reached by jumping left off the top of the slope.
        let slope = n.locate(800, -79).expect("slope");
        let lip = n.locate(720, -145).expect("platform above");
        let e = n.edges[slope].iter().find(|e| e.to == lip).expect("edge off the slope top");
        match n.step((e.xlo + e.xhi) / 2, n.y_on(slope, (e.xlo + e.xhi) / 2), true, lip, 720, 6) {
            Cmd::RunJump(Dir::Left, staging) => assert!(staging >= e.xhi + RUN_UP - 2 && staging > 810, "staging {staging} stays on the slope (window {}..{})", e.xlo, e.xhi),
            other => panic!("expected a running jump, got {other:?}"),
        }
    }

    #[test]
    fn a_run_up_never_extends_past_the_end_of_the_level_stretch() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../neon-calib/maps");
        let Some(geom) = MapGeom::load(&dir, 10000011) else { return };
        let n = Nav::new(&geom, Tuning::default());
        // The y=95 floor (360..399 is just its last 39 px) is level for a long way to the west: that is room to run.
        let small = n.locate(380, 95).expect("narrow platform");
        let (x1, x2) = n.level_run(small, 380);
        assert!(x1 <= -300 && x2 >= 399, "the y=95 floor is one long level run, got {x1}..{x2}");
        // And the take-off from the flat floor that is level for a long way does get a long run.
        let floor = n.locate(600, 215).expect("floor piece");
        let (f1, f2) = n.level_run(floor, 600);
        assert!(f2 - f1 > 150, "floor run {f1}..{f2}");
    }

    #[test]
    fn walking_across_a_floor_joint_never_turns_back_for_the_joint() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../neon-calib/maps");
        let Some(geom) = MapGeom::load(&dir, 10000011) else { return };
        let n = Nav::new(&geom, Tuning::default());
        let goal = n.locate(430, 35).expect("platform east and above");
        // The floor pieces 321..360 and 360..399 (y 95) meet at 360; 362 is 2 px past the joint. Going east, the plan
        // must say east (towards the take-off window at ~366), not "back to 360".
        for x in [358, 360, 361, 362, 363, 365] {
            let c = n.step(x, 95, true, goal, 430, 6);
            assert!(!matches!(c, Cmd::Walk(Dir::Left) | Cmd::Retreat(..)), "x={x}: {c:?}");
        }
    }

    #[test]
    fn at_a_ladder_top_it_keeps_climbing_up_onto_the_floor() {
        // 3-Way Road-Split: a ladder (x -606, y -203..33) under a floor at y -205; the read wobbles -202..-208 at the top.
        let g = MapGeom {
            footholds: vec![
                RawFoothold { x1: -669, y1: -205, x2: -540, y2: -205, no_drop: false },
                RawFoothold { x1: -669, y1: 35, x2: -540, y2: 35, no_drop: false },
            ],
            ladders: vec![Ladder { x: -606, y1: -203, y2: 33, ladder: true }],
            portals: vec![],
            mobs: Default::default(), mob_spawns: vec![],
        };
        let n = Nav::new(&g, Tuning::default());
        let top = n.locate(-600, -205).unwrap();
        for y in [-180, -202, -205, -208] {
            assert_eq!(n.step(-606, y, false, top, -600, 6), Cmd::Climb { dir: Vert::Up, grab: false }, "at y {y}");
        }
    }

}


