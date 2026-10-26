//! Quests, driven the way a player does them: walk to the quest NPC, click it and answer its dialog.
//!
//! The quest data (who starts and who completes a quest, level and item requirements, rewards) comes from
//! the client's own `QuestData` files (`neon-calib/tools/wzquests.py` -> `neon-calib/quests.json`), and the
//! NPC positions from the map files (`neon-calib/maps/*.json`). Nothing is sent to the server: the NPC is
//! clicked in the game window like a shop NPC (see `shop.rs`).

use crate::plan::{Plan, Step};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The game view is the window's client area: 1366x768 starting at (28, 97) of the 1422x900 reference window.
const VIEW_ORIGIN: (i32, i32) = (28, 97);
const VIEW_SIZE: (i32, i32) = (1366, 768);
/// Where the camera keeps the player: horizontally centred, vertically low (feet near screen y 660). Measured in
/// Forest West, 3-Way Road-Split and Lith Harbor.
const ANCHOR: (i32, i32) = (683, 563);
/// An NPC's position is its feet; its body is clickable this far above.
const NPC_BODY_OFFSET: i32 = 40;

static DATA: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();

/// Remember where the quest and map data live (call once at start-up).
pub fn configure(quests: PathBuf, maps: PathBuf) {
    let _ = DATA.set((quests, maps));
}

/// One quest as the plan needs it: who to talk to for the start / the completion.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestInfo {
    pub name: String,
    pub npc: u32,
    pub lvmin: u32,
    /// EXP the phase pays (the hand-in's reward): a hand-in is confirmed by our EXP changing.
    pub exp: Option<u64>,
}

/// An NPC placed in a map.
#[derive(Debug, Clone, PartialEq)]
pub struct Located {
    pub map: u32,
    pub at: [i32; 2],
    pub bounds: [i32; 4],
}

/// World coordinate shown at the view's top/left edge. The camera follows the player but never shows beyond the
/// map's view rectangle; a map smaller than the view is centred in it.
fn camera(p: i32, lo: i32, hi: i32, view: i32, anchor: i32) -> i32 {
    if hi - lo <= view { lo - (view - (hi - lo)) / 2 } else { (p - anchor).clamp(lo, hi - view) }
}

/// Where to click (reference window pixels) to hit the NPC standing at `npc` while the player is at `player`.
pub fn click_point(npc: (i32, i32), player: (i32, i32), bounds: [i32; 4]) -> (i32, i32) {
    let left = camera(player.0, bounds[0], bounds[1], VIEW_SIZE.0, ANCHOR.0);
    let top = camera(player.1, bounds[2], bounds[3], VIEW_SIZE.1, ANCHOR.1);
    (VIEW_ORIGIN.0 + npc.0 - left, VIEW_ORIGIN.1 + npc.1 - top - NPC_BODY_OFFSET)
}

/// Where to click for an NPC at `npc` given the real camera: the world -> screen mapping is exactly 1:1.
pub fn click_point_camera(npc: (i32, i32), camera: (i32, i32)) -> (i32, i32) {
    (VIEW_ORIGIN.0 + npc.0 - camera.0, VIEW_ORIGIN.1 + npc.1 - camera.1 - NPC_BODY_OFFSET)
}

/// Is that click inside the game view (and clear of the bottom status bar)?
pub fn on_screen(p: (i32, i32)) -> bool {
    (VIEW_ORIGIN.0 + 20..VIEW_ORIGIN.0 + VIEW_SIZE.0 - 20).contains(&p.0) && (VIEW_ORIGIN.1 + 20..790).contains(&p.1)
}

fn load_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The kills a quest asks for at its hand-in, in the quest data's order (the order of the server's progress digits).
pub fn required_kills(quest: u32) -> Vec<(u32, u32)> {
    let Some((quests, _)) = DATA.get() else { return vec![] };
    let Some(q) = load_json(quests) else { return vec![] };
    q.get(quest.to_string())
        .and_then(|q| q.get("end"))
        .and_then(|e| e.get("mobs"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|m| Some((m.get("id")?.as_u64()? as u32, m.get("count")?.as_u64()? as u32))).collect())
        .unwrap_or_default()
}

/// Look a quest up in the extracted quest file.
pub fn quest_info(quest: u32, complete: bool) -> Option<QuestInfo> {
    let (quests, _) = DATA.get()?;
    let q = load_json(quests)?;
    let q = q.get(quest.to_string())?;
    let phase = q.get(if complete { "end" } else { "start" })?;
    Some(QuestInfo {
        name: q.get("name")?.as_str().unwrap_or_default().to_string(),
        npc: phase.get("npc")?.as_u64()? as u32,
        lvmin: q.get("start")?.get("lvmin").and_then(Value::as_u64).unwrap_or(0) as u32,
        exp: phase.get("exp").and_then(Value::as_u64).filter(|e| *e > 0),
    })
}

#[derive(Deserialize)]
struct MapNpc {
    id: u32,
    x: Option<i32>,
    y: Option<i32>,
}

/// Find a map that has this NPC (`prefer` first, then the lowest map id).
pub fn find_npc(npc: u32, prefer: Option<u32>) -> Option<Located> {
    let (_, maps) = DATA.get()?;
    let mut best: Option<Located> = None;
    let mut ids: Vec<u32> = std::fs::read_dir(maps).ok()?.filter_map(|e| e.ok()?.path().file_stem()?.to_str()?.parse().ok()).collect();
    ids.sort();
    for map in ids {
        let Some(v) = load_json(&maps.join(format!("{map}.json"))) else { continue };
        let Some(npcs) = v.get("npcs").and_then(Value::as_array) else { continue };
        for n in npcs {
            let Ok(n) = serde_json::from_value::<MapNpc>(n.clone()) else { continue };
            if n.id != npc {
                continue;
            }
            let (Some(x), Some(y)) = (n.x, n.y) else { continue };
            let b = v.get("bounds").cloned().unwrap_or(Value::Null);
            let get = |k: &str, d: i32| b.get(k).and_then(Value::as_i64).map_or(d, |v| v as i32);
            let found = Located { map, at: [x, y], bounds: [get("VRLeft", -100_000), get("VRRight", 100_000), get("VRTop", -100_000), get("VRBottom", 100_000)] };
            if Some(map) == prefer {
                return Some(found);
            }
            best.get_or_insert(found);
        }
    }
    best
}

/// Fill in the NPC's map and position of every quest step that does not name them. Returns warnings.
pub fn resolve(plan: &mut Plan) -> Vec<String> {
    let mut warnings = vec![];
    let mut cache: HashMap<(u32, bool), Option<(u32, Located)>> = HashMap::new();
    resolve_steps(&mut plan.steps, &mut cache, &mut warnings);
    resolve_steps(&mut plan.resupply, &mut cache, &mut warnings);
    warnings
}

type NpcCache = HashMap<(u32, bool), Option<(u32, Located)>>;

fn resolve_steps(steps: &mut [Step], cache: &mut NpcCache, warnings: &mut Vec<String>) {
    for step in steps {
        // Resupply trips nested in grind and kill steps.
        if let Step::Grind { resupply: Some(r), .. } | Step::Kill { resupply: Some(r), .. } = step {
            resolve_steps(r, cache, warnings);
            continue;
        }
        // A shop named by its NPC id.
        if let Step::Shop { map, npc_id, at, bounds, .. } = step {
            if *npc_id != 0 && *at == [0, 0] {
                match find_npc(*npc_id, (*map != 0).then_some(*map)) {
                    Some(l) => {
                        *map = l.map;
                        *at = l.at;
                        *bounds = l.bounds;
                    }
                    None => {
                        *map = 0;
                        warnings.push(format!("shop NPC {npc_id}: not in the map data; the step will be skipped"));
                    }
                }
            }
            continue;
        }
        let Step::Quest { quest, complete, map, npc, at, bounds, .. } = step else { continue };
        if *map != 0 && *npc != 0 && *at != [0, 0] {
            continue;
        }
        if *quest == 0 {
            // Just talking to a named NPC.
            match find_npc(*npc, (*map != 0).then_some(*map)) {
                Some(l) => {
                    *map = l.map;
                    *at = l.at;
                    *bounds = l.bounds;
                }
                None => warnings.push(format!("NPC {npc}: not in the map data; the step will be skipped")),
            }
            continue;
        }
        let hit = cache
            .entry((*quest, *complete))
            .or_insert_with(|| {
                let info = quest_info(*quest, *complete)?;
                find_npc(info.npc, (*map != 0).then_some(*map)).map(|l| (info.npc, l))
            })
            .clone();
        match hit {
            Some((id, l)) => {
                *npc = id;
                *map = l.map;
                *at = l.at;
                *bounds = l.bounds;
            }
            None => warnings.push(format!("quest {quest}: cannot find the {} NPC in the map data; the step will be skipped", if *complete { "completion" } else { "start" })),
        }
    }
}

/// One conversation to have, handed to the bot once the character stands at the NPC.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestJob {
    pub quest: u32,
    /// The game camera (world coordinates of the view's top-left corner) when known: then the NPC's screen
    /// position is exact, `screen = origin + world - camera`.
    pub camera: Option<(i32, i32)>,
    /// The map the NPC stands in (to find the other NPCs).
    pub map: u32,
    pub complete: bool,
    pub npc: [i32; 2],
    pub bounds: [i32; 4],
    /// Where the character stands (map coordinates), to place the click.
    pub player: (i32, i32),
    /// Answer a YES / NO question with YES.
    pub yes: bool,
}

impl QuestJob {
    pub fn click(&self) -> (i32, i32) {
        match self.camera {
            Some(c) => click_point_camera((self.npc[0], self.npc[1]), c),
            None => click_point((self.npc[0], self.npc[1]), self.player, self.bounds),
        }
    }
}

// ---- the dialog ---------------------------------------------------------------------------------------
//
// An NPC dialog is a frame whose left edge and bottom bar are fixed but whose height follows the text. Its anchor
// is the green-yellow END CHAT button at the bottom left; NEXT / BACK are blue buttons 51 px above it at the right,
// ACCEPT / DECLINE / OK sit in the bottom bar itself (ACCEPT and OK are orange, DECLINE pink), and a list of quests
// marks its entries with a red arrow. Positions are in reference-window pixels (1422x900).

/// Tall enough for a reward page: its button bar sits at y ~685 (Teo's second hand-in stayed open below the old 670).
const DIALOG_REGION: (i32, i32, i32, i32) = (440, 340, 560, 410);
const END_CHAT_X: (i32, i32) = (455, 545);
const NEXT_ABOVE_BAR: i32 = 51;
const ARROW_X: (i32, i32) = (618, 640);
const MAX_STEPS: u32 = 45;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Paint {
    Blue,
    Orange,
    Pink,
    EndChat,
    Red,
}

fn is(p: Paint, (r, g, b): (u8, u8, u8)) -> bool {
    let (r, g, b) = (r as i32, g as i32, b as i32);
    match p {
        Paint::Blue => b >= 215 && r <= 75 && (115..=175).contains(&g),
        Paint::Orange => r >= 225 && (115..=180).contains(&g) && b <= 75,
        Paint::Pink => r >= 150 && g <= 70 && (75..=115).contains(&b),
        Paint::EndChat => (165..=220).contains(&r) && g >= 195 && b <= 65 && g - b >= 140,
        Paint::Red => r >= 200 && g <= 70 && b <= 70,
    }
}

/// A raw RGB screenshot of part of the window.
pub(crate) struct Shot {
    w: usize,
    h: usize,
    /// Reference pixels per shot pixel, and the region's origin in reference pixels.
    k: f64,
    origin: (i32, i32),
    rgb: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Blob {
    /// Centre in reference-window pixels.
    x: i32,
    y: i32,
    n: usize,
    /// Width of the bounding box in reference pixels (a button's label splits its rows, so count the box).
    w: i32,
}

impl Shot {
    pub(crate) fn take(win: &crate::shop::Win, region: (i32, i32, i32, i32)) -> Option<Shot> {
        let (x, y) = win.scale((region.0, region.1));
        let (w, h) = win.size((region.2, region.3));
        let out = std::process::Command::new("import")
            .args(["-window", &win.id, "-crop", &format!("{w}x{h}+{x}+{y}"), "+repage", "-depth", "8", "rgb:-"])
            .envs(crate::shop::x_env())
            .output()
            .ok()?;
        let (w, h) = (w as usize, h as usize);
        (out.stdout.len() >= w * h * 3).then(|| Shot { w, h, k: 1422.0 / win.w, origin: (region.0, region.1), rgb: out.stdout })
    }

    /// How many pixels of the whole shot `f` says yes to.
    pub(crate) fn count_all(&self, f: impl Fn((u8, u8, u8)) -> bool) -> usize {
        self.rgb.chunks_exact(3).take(self.w * self.h).filter(|p| f((p[0], p[1], p[2]))).count()
    }

    /// How many pixels of the shot are within `tol` (per channel) of `rgb`, inside the box (reference pixels).
    pub(crate) fn count_near(&self, rgb: (u8, u8, u8), tol: i32, bx: (i32, i32), by: (i32, i32)) -> usize {
        let to_px = |v: i32, o: i32| (((v - o) as f64) / self.k).round().max(0.0) as usize;
        let (x0, x1) = (to_px(bx.0, self.origin.0).min(self.w), to_px(bx.1, self.origin.0).min(self.w));
        let (y0, y1) = (to_px(by.0, self.origin.1).min(self.h), to_px(by.1, self.origin.1).min(self.h));
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let (r, g, b) = self.px(x, y);
                if (r as i32 - rgb.0 as i32).abs() <= tol && (g as i32 - rgb.1 as i32).abs() <= tol && (b as i32 - rgb.2 as i32).abs() <= tol {
                    n += 1;
                }
            }
        }
        n
    }

    /// How many pixels in the box are button-orange (an enabled "+" button).
    pub(crate) fn count_orange(&self, bx: (i32, i32), by: (i32, i32)) -> usize {
        let to_px = |v: i32, o: i32| (((v - o) as f64) / self.k).round().max(0.0) as usize;
        let (x0, x1) = (to_px(bx.0, self.origin.0).min(self.w), to_px(bx.1, self.origin.0).min(self.w));
        let (y0, y1) = (to_px(by.0, self.origin.1).min(self.h), to_px(by.1, self.origin.1).min(self.h));
        (y0..y1).map(|y| (x0..x1).filter(|&x| is(Paint::Orange, self.px(x, y))).count()).sum()
    }

    fn px(&self, x: usize, y: usize) -> (u8, u8, u8) {
        let i = (y * self.w + x) * 3;
        (self.rgb[i], self.rgb[i + 1], self.rgb[i + 2])
    }

    /// Connected blobs of one colour inside a box given in reference pixels (x0, x1, y0, y1).
    fn blobs(&self, paint: Paint, bx: (i32, i32), by: (i32, i32)) -> Vec<Blob> {
        let to_px = |v: i32, o: i32| (((v - o) as f64) / self.k).round().max(0.0) as usize;
        let (x0, x1) = (to_px(bx.0, self.origin.0).min(self.w), to_px(bx.1, self.origin.0).min(self.w));
        let (y0, y1) = (to_px(by.0, self.origin.1).min(self.h), to_px(by.1, self.origin.1).min(self.h));
        let mut seen = vec![false; self.w * self.h];
        let mut out = vec![];
        for y in y0..y1 {
            for x in x0..x1 {
                if seen[y * self.w + x] || !is(paint, self.px(x, y)) {
                    continue;
                }
                let (mut stack, mut n, mut sx, mut sy, mut lo, mut hi) = (vec![(x, y)], 0usize, 0usize, 0usize, x, x);
                seen[y * self.w + x] = true;
                while let Some((cx, cy)) = stack.pop() {
                    n += 1;
                    sx += cx;
                    sy += cy;
                    lo = lo.min(cx);
                    hi = hi.max(cx);
                    for (nx, ny) in [(cx + 1, cy), (cx.wrapping_sub(1), cy), (cx, cy + 1), (cx, cy.wrapping_sub(1))] {
                        if nx >= x0 && nx < x1 && ny >= y0 && ny < y1 && !seen[ny * self.w + nx] && is(paint, self.px(nx, ny)) {
                            seen[ny * self.w + nx] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
                out.push(Blob { x: self.origin.0 + (sx as f64 / n as f64 * self.k).round() as i32, y: self.origin.1 + (sy as f64 / n as f64 * self.k).round() as i32, n, w: ((hi - lo + 1) as f64 * self.k).round() as i32 });
            }
        }
        out
    }
}

/// What the dialog on screen offers.
#[derive(Debug, Clone, PartialEq)]
enum Screen {
    /// No dialog: the conversation is over.
    Closed,
    /// ACCEPT (wide orange button next to DECLINE): the quest offer.
    Accept(i32, i32),
    /// A narrow orange button (YES, at x, y) next to a pink one: a YES / NO question (buying, travelling, ...).
    /// Answered only when the step says so.
    Question(i32, i32),
    /// An OK-style button with nothing to decline.
    Ok(i32, i32),
    Next(i32, i32),
    /// A quest list entry (the red arrow).
    Entry(i32, i32),
    /// Text still typing out, or nothing clickable yet.
    Waiting,
}

fn read_screen(shot: &Shot) -> Screen {
    let (full_x, full_y) = (DIALOG_REGION.0..DIALOG_REGION.0 + DIALOG_REGION.2, DIALOG_REGION.1..DIALOG_REGION.1 + DIALOG_REGION.3);
    let bars: Vec<Blob> = shot.blobs(Paint::EndChat, END_CHAT_X, (full_y.start, full_y.end)).into_iter().filter(|b| b.n >= 30).collect();
    let Some(bar) = bars.iter().max_by_key(|b| b.n) else { return Screen::Closed };
    let band = |c: i32| (c - 14, c + 14);
    let _ = full_x;
    let orange: Vec<Blob> = shot.blobs(Paint::Orange, (780, 975), band(bar.y)).into_iter().filter(|b| b.n >= 25).collect();
    let pink: Vec<Blob> = shot.blobs(Paint::Pink, (780, 975), band(bar.y)).into_iter().filter(|b| b.n >= 25).collect();
    if let Some(o) = orange.iter().min_by_key(|b| b.x) {
        if pink.is_empty() {
            return Screen::Ok(o.x, o.y);
        }
        // ACCEPT / DECLINE buttons are about 55 px wide, YES / NO about 38.
        return if o.w >= 46 { Screen::Accept(o.x, o.y) } else { Screen::Question(o.x, o.y) };
    }
    let blue: Vec<Blob> = shot.blobs(Paint::Blue, (880, 975), band(bar.y - NEXT_ABOVE_BAR)).into_iter().filter(|b| b.n >= 25).collect();
    if let Some(n) = blue.iter().max_by_key(|b| b.x) {
        return Screen::Next(n.x, n.y);
    }
    let arrows: Vec<Blob> = shot.blobs(Paint::Red, ARROW_X, (bar.y - 200, bar.y - 40)).into_iter().filter(|b| b.n >= 6).collect();
    if let Some(a) = arrows.iter().min_by_key(|b| b.y) {
        return Screen::Entry(a.x + 55, a.y);
    }
    Screen::Waiting
}

/// Close the dialog with END CHAT (always safe). True if there was one.
pub fn end_chat(win: &crate::shop::Win) -> bool {
    if let Some(shot) = Shot::take(win, DIALOG_REGION) {
        if let Some(bar) = shot.blobs(Paint::EndChat, END_CHAT_X, (DIALOG_REGION.1, DIALOG_REGION.1 + DIALOG_REGION.3)).into_iter().filter(|b| b.n >= 30).max_by_key(|b| b.n) {
            let _ = win.click((bar.x, bar.y));
            return true;
        }
    }
    false
}

/// Answer whatever NPC dialog is open until it closes: pick the first quest of a list, press NEXT, ACCEPT and OK.
/// Never DECLINE and never END CHAT; YES only when `yes` is set. Returns what happened.
pub fn drive_dialog(win: &crate::shop::Win, yes: bool) -> Result<String, String> {
    let (mut accepted, mut clicks, mut closed_in_a_row) = (false, 0u32, 0u32);
    for _ in 0..MAX_STEPS {
        crate::shop::pause(650);
        let Some(shot) = Shot::take(win, DIALOG_REGION) else { return Err("cannot capture the game window".into()) };
        match read_screen(&shot) {
            Screen::Closed => {
                closed_in_a_row += 1;
                if closed_in_a_row >= 2 {
                    return Ok(if accepted { format!("accepted ({clicks} clicks)") } else if clicks > 0 { format!("finished ({clicks} clicks)") } else { "no dialog opened".into() });
                }
            }
            Screen::Waiting => closed_in_a_row = 0,
            Screen::Question(x, y) if yes => {
                closed_in_a_row = 0;
                clicks += 1;
                win.click((x, y))?;
            }
            Screen::Question(..) => {
                end_chat(win);
                return Err("the NPC asked a yes/no question; not answering it".into());
            }
            Screen::Accept(x, y) => {
                closed_in_a_row = 0;
                accepted = true;
                clicks += 1;
                win.click((x, y))?;
            }
            Screen::Ok(x, y) | Screen::Next(x, y) | Screen::Entry(x, y) => {
                closed_in_a_row = 0;
                clicks += 1;
                win.click((x, y))?;
            }
        }
    }
    end_chat(win);
    Err("the dialog did not end after many steps".into())
}

/// Offsets tried around the predicted click, nearest first. With the real camera the prediction is exact; the sweep
/// only absorbs sprite size and the camera having moved since it was read. Without a camera (old model) it is a guess.
const SWEEP_X: [i32; 5] = [0, -20, 20, -40, 40];
const SWEEP_Y: [i32; 4] = [0, -20, 20, -40];
/// How long a click gets to open a dialog before the next spot is tried.
const OPEN_WAIT_MS: u64 = 600;
/// Keep this far from another NPC's predicted spot so a sweep does not open the wrong conversation.
const OTHER_NPC_CLEARANCE: i32 = 40;

/// Reference-pixel click points of the other NPCs of the map, for the sweep to keep away from.
fn other_npcs(job: &QuestJob) -> Vec<(i32, i32)> {
    let Some((_, maps)) = DATA.get() else { return vec![] };
    let Some(v) = load_json(&maps.join(format!("{}.json", job.map))) else { return vec![] };
    v.get("npcs").and_then(Value::as_array).map(|a| a.iter().filter_map(|n| {
        let (x, y) = (n.get("x")?.as_i64()? as i32, n.get("y")?.as_i64()? as i32);
        ((x, y) != (job.npc[0], job.npc[1])).then(|| match job.camera {
            Some(c) => click_point_camera((x, y), c),
            None => click_point((x, y), job.player, job.bounds),
        })
    }).collect()).unwrap_or_default()
}

/// Have the conversation: click the NPC (trying a few offsets until a dialog opens) and answer it until the quest
/// is accepted / handed in.
pub fn run(job: &QuestJob) -> Result<String, String> {
    let _ui = crate::shop::UI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let win = crate::shop::Win::find()?;
    let base = job.click();
    let others = other_npcs(job);
    for dy in SWEEP_Y {
        for dx in SWEEP_X {
            let p = (base.0 + dx, base.1 + dy);
            if !on_screen(p) || others.iter().any(|o| (o.0 - p.0).abs() < OTHER_NPC_CLEARANCE && (o.1 - p.1).abs() < 60) {
                continue;
            }
            win.click(p)?;
            crate::shop::pause(OPEN_WAIT_MS);
            let opened = Shot::take(&win, DIALOG_REGION).map_or(false, |s| read_screen(&s) != Screen::Closed);
            if opened {
                return drive_dialog(&win, job.yes).map(|r| format!("{r} (click at {p:?})"));
            }
        }
    }
    // Leave evidence: what the screen looked like when the NPC could not be found.
    if let Ok(dir) = std::env::var("NEON_DEBUG_DIR") {
        let _ = std::process::Command::new("import").args(["-window", &win.id, &format!("{dir}/quest-{}-fail.png", job.quest)]).envs(crate::shop::x_env()).output();
    }
    Err(format!("no dialog opened around {base:?} (player at {:?}): the NPC is not where the camera model says", job.player))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_point_matches_the_measured_dept_store_view() {
        // Henesys Department Store: a small map is centred; Luna (-258, 84) was clicked at about (452, 518).
        let p = click_point((-258, 84), (300, 182), [-400, 400, -300, 300]);
        assert!((p.0 - 452).abs() <= 6 && (p.1 - 518).abs() <= 12, "{p:?}");
    }

    #[test]
    fn click_point_matches_phil_in_lith_harbor() {
        // Standing about 35 px left of the booth, Phil (804, 323) showed his face at about (743, 593).
        let p = click_point((804, 323), (770, 347), [-832, 5422, -1191, 837]);
        assert!((p.0 - 743).abs() <= 12 && (p.1 - 593).abs() <= 30, "{p:?}");
    }

    #[test]
    fn the_camera_stops_at_the_map_edges() {
        let b = [0, 3000, 0, 2000];
        assert_eq!(click_point((1500, 1000), (1500, 1000), b).0, 28 + 683, "player on the NPC: horizontally centred");
        assert_eq!(click_point((50, 1000), (10, 1000), b).0, 28 + 50, "camera pinned at the left edge");
        assert_eq!(click_point((50, 20), (10, 10), b).1, 97 + 20 - NPC_BODY_OFFSET, "and at the top");
    }

    #[test]
    fn far_npcs_are_off_screen() {
        assert!(on_screen((711, 440)));
        assert!(!on_screen(click_point((2000, 500), (1000, 500), [0, 3000, 0, 1000])));
    }

    #[test]
    fn button_colours_are_told_apart() {
        assert!(is(Paint::Blue, (51, 153, 238)) && !is(Paint::Orange, (51, 153, 238)));
        assert!(is(Paint::Orange, (238, 136, 51)) && is(Paint::Orange, (255, 170, 34)) && !is(Paint::Pink, (238, 136, 51)));
        assert!(is(Paint::Pink, (187, 51, 102)) && !is(Paint::Orange, (187, 51, 102)));
        assert!(is(Paint::EndChat, (187, 204, 34)) && !is(Paint::EndChat, (51, 153, 238)));
        assert!(is(Paint::Red, (220, 30, 30)) && !is(Paint::Orange, (220, 30, 30)));
    }

    fn paint_box(sh: &mut Shot, c: (u8, u8, u8), x: i32, y: i32, w: i32, h: i32) {
        for yy in y..y + h {
            for xx in x..x + w {
                let (px, py) = ((xx - sh.origin.0) as usize, (yy - sh.origin.1) as usize);
                let i = (py * sh.w + px) * 3;
                sh.rgb[i] = c.0;
                sh.rgb[i + 1] = c.1;
                sh.rgb[i + 2] = c.2;
            }
        }
    }

    fn blank() -> Shot {
        Shot { w: 560, h: 330, k: 1.0, origin: (440, 340), rgb: vec![200; 560 * 330 * 3] }
    }

    #[test]
    fn reads_the_screens_of_the_phil_conversation() {
        let mut s = blank();
        assert_eq!(read_screen(&s), Screen::Closed);
        paint_box(&mut s, (187, 204, 34), 457, 593, 82, 14); // END CHAT, bottom bar at y 600
        assert_eq!(read_screen(&s), Screen::Waiting, "text still typing");
        paint_box(&mut s, (51, 153, 238), 898, 535, 44, 14); // NEXT 51 px above the bar
        paint_box(&mut s, (51, 153, 238), 848, 535, 44, 14); // BACK
        assert!(matches!(read_screen(&s), Screen::Next(x, y) if (x - 920).abs() <= 3 && (y - 542).abs() <= 3));
        paint_box(&mut s, (238, 136, 51), 846, 593, 56, 14); // ACCEPT
        paint_box(&mut s, (187, 51, 102), 906, 593, 56, 14); // DECLINE
        assert!(matches!(read_screen(&s), Screen::Accept(x, _) if (x - 874).abs() <= 3), "accept wins over next, decline is never picked");
        let mut yn = blank();
        paint_box(&mut yn, (187, 204, 34), 457, 593, 82, 14);
        paint_box(&mut yn, (238, 136, 51), 860, 593, 38, 14); // YES
        paint_box(&mut yn, (187, 51, 102), 906, 593, 38, 14); // NO
        assert!(matches!(read_screen(&yn), Screen::Question(..)), "a yes/no question is recognised (answered only on request)");
    }

    #[test]
    fn a_lone_orange_button_is_ok_and_a_red_arrow_is_a_quest_entry() {
        let mut s = blank();
        paint_box(&mut s, (187, 204, 34), 457, 593, 82, 14);
        paint_box(&mut s, (238, 136, 51), 925, 593, 40, 14);
        assert!(matches!(read_screen(&s), Screen::Ok(..)));
        let mut s = blank();
        paint_box(&mut s, (187, 204, 34), 457, 593, 82, 14);
        paint_box(&mut s, (220, 30, 30), 624, 447, 5, 7); // the arrow before "(Lv. 8) Phil's Call"
        assert!(matches!(read_screen(&s), Screen::Entry(x, y) if (x - 681).abs() <= 3 && (y - 450).abs() <= 3));
    }

    #[test]
    fn the_real_camera_places_olaf_where_the_screenshot_showed_him() {
        // Camera (3830, -19): Olaf (4100, 427) stood with his feet at about screen (298, 543).
        let p = click_point_camera((4100, 427), (3830, -19));
        assert_eq!(p.0, 298);
        assert!((p.1 + NPC_BODY_OFFSET - 543).abs() <= 2, "{p:?}");
    }

    #[test]
    fn an_accept_button_whose_label_splits_its_rows_is_still_wide() {
        let mut s = blank();
        paint_box(&mut s, (187, 204, 34), 457, 593, 82, 14);
        paint_box(&mut s, (238, 136, 51), 846, 593, 56, 14); // ACCEPT
        paint_box(&mut s, (51, 51, 51), 850, 598, 48, 5); // the label's dark letters cross the middle rows
        paint_box(&mut s, (187, 51, 102), 906, 593, 56, 14); // DECLINE
        assert!(matches!(read_screen(&s), Screen::Accept(..)), "{:?}", read_screen(&s));
    }
}
