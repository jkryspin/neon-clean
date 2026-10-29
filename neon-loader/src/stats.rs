//! Spending ability points and skill points through the game's own windows, putting skills on quick slots, and
//! noticing level-ups.
//!
//! What goes where comes from a character build (`builds/warrior.json`, `char_build.rs`): per level, which stats the
//! 5 AP go to and which skills the SP go to. Without a build (or before the level is known) every AP goes to STR and
//! every Beginner SP to Three Snails, as before. Both windows are driven the way a player does it: open with S / K,
//! check by pixel that the window really is there (so a click can never land on the game world), click a "+" while it
//! is enabled (orange), close the window.
//!
//! The level is noticed from the level box in the status bar, not from memory (the memory reading never updates).

use crate::char_build::{take_known, CharBuild, Stat};
use crate::quest::Shot;
use crate::shop::{pause, xdo, Win, QUICKSLOTS, UI_LOCK};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tracing::{info, warn};

/// Stat window (key S): reference-window pixels, measured on the 1422x900 frame. The DEX/INT/LUK "+" buttons are
/// 18 px apart below STR's (measured 2026-10-25 on a headless client: labels at 415/433/451/469 of the game view).
const STAT_STR_PLUS: (i32, i32) = (772, 522);
const STAT_ROW_PITCH: i32 = 18;
const STAT_REGION: (i32, i32, i32, i32) = (620, 340, 190, 280);
/// The olive-green stat name boxes (STR / DEX / INT / LUK) prove the window is open.
const STAT_LABEL_GREEN: (u8, u8, u8) = (149, 162, 63);
const STAT_LABELS_BOX: ((i32, i32), (i32, i32)) = ((626, 672), (513, 582));
/// Skill window (key K): the "+" of the first row (Three Snails on the Beginner tab); rows are 40 px apart and six
/// fit without scrolling (measured 2026-10-25). Each row also has a second button right of "+" that is not used.
const SKILL_FIRST_PLUS: (i32, i32) = (759, 421);
const SKILL_ROW_PITCH: i32 = 40;
/// Top-left of the first row's 28x28 icon; clicking the icon picks the skill up (to drop it on a quick slot).
const SKILL_FIRST_ICON: (i32, i32) = (624, 397);
/// Centre of the first tab (Beginner); the job tabs follow to the right, one tab width apart.
const SKILL_FIRST_TAB: (i32, i32) = (633, 327);
const SKILL_TAB_PITCH: i32 = 32;
/// Colour of the selected tab.
const SKILL_TAB_ON: (u8, u8, u8) = (234, 101, 129);
const SKILL_REGION: (i32, i32, i32, i32) = (612, 296, 200, 370);
/// The dark book-name bar ("Beginner's Basics") proves the skill window is open.
const SKILL_BAR: (u8, u8, u8) = (56, 63, 74);
const SKILL_BAR_BOX: ((i32, i32), (i32, i32)) = ((650, 800), (360, 378));
/// The level number in the status bar.
const LEVEL_BOX: (i32, i32, i32, i32) = (350, 836, 40, 26);
/// A "+" button counts as enabled with at least this many orange pixels in its 16x16 box.
const ENABLED_MIN: usize = 25;
const MAX_STAT_CLICKS: u32 = 30;
const MAX_SKILL_CLICKS: u32 = 30;

static BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Spent {
    pub ap: u32,
    pub sp: u32,
    /// Skills put on quick slots this time.
    pub slotted: Vec<String>,
}

/// What the stream last said about the character, set by the bot loop every tick.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Progress {
    pub level: Option<u16>,
    pub job: Option<u16>,
    pub ap: Option<u32>,
    pub sp: Option<u32>,
}

static PROGRESS: Mutex<Progress> = Mutex::new(Progress { level: None, job: None, ap: None, sp: None });
static BUILD: OnceLock<CharBuild> = OnceLock::new();
/// Skills of the build known to be on their quick slots, and whether that changed since the bot last looked.
static SLOTTED: Mutex<(Vec<String>, bool)> = Mutex::new((Vec::new(), false));

/// Use this build for every later spend (once per process).
pub fn set_build(b: CharBuild) {
    let _ = BUILD.set(b);
}

pub fn build() -> Option<&'static CharBuild> {
    BUILD.get()
}

pub fn observe(p: Progress) {
    *PROGRESS.lock().unwrap_or_else(|e| e.into_inner()) = p;
}

/// The skills now on their quick slots, once each time the list changes.
pub fn take_slotted() -> Option<Vec<String>> {
    let mut g = SLOTTED.lock().unwrap_or_else(|e| e.into_inner());
    if !g.1 {
        return None;
    }
    g.1 = false;
    Some(g.0.clone())
}

fn plus_enabled(shot: &Shot, at: (i32, i32)) -> bool {
    shot.count_orange((at.0 - 8, at.0 + 8), (at.1 - 8, at.1 + 8)) >= ENABLED_MIN
}

/// Open a window with `key`, check it is there; closes it again on failure so the next attempt starts clean.
fn open(win: &Win, key: &str, region: (i32, i32, i32, i32), proof: impl Fn(&Shot) -> bool) -> Result<(), String> {
    for attempt in 0..2 {
        xdo(&["key", key])?;
        pause(700);
        if Shot::take(win, region).map_or(false, |s| proof(&s)) {
            return Ok(());
        }
        // Maybe it was already open and the key closed it; or it moved. Press again once.
        if attempt == 0 {
            continue;
        }
    }
    Err(format!("the window for key {key} did not appear where expected"))
}

fn stat_window_open(s: &Shot) -> bool {
    s.count_near(STAT_LABEL_GREEN, 22, STAT_LABELS_BOX.0, STAT_LABELS_BOX.1) >= 150
}

fn skill_window_open(s: &Shot) -> bool {
    s.count_near(SKILL_BAR, 14, SKILL_BAR_BOX.0, SKILL_BAR_BOX.1) >= 300
}

fn stat_plus(stat: Stat) -> (i32, i32) {
    let row = match stat {
        Stat::Str => 0,
        Stat::Dex => 1,
        Stat::Int => 2,
        Stat::Luk => 3,
    };
    (STAT_STR_PLUS.0, STAT_STR_PLUS.1 + STAT_ROW_PITCH * row)
}

fn skill_plus(row: u32) -> (i32, i32) {
    (SKILL_FIRST_PLUS.0, SKILL_FIRST_PLUS.1 + SKILL_ROW_PITCH * row as i32)
}

/// Top-left of a row's icon.
fn skill_icon(row: u32) -> (i32, i32) {
    (SKILL_FIRST_ICON.0, SKILL_FIRST_ICON.1 + SKILL_ROW_PITCH * row as i32)
}

/// Show tab `tab` of the open skill window. The Beginner tab lists three skills, a job tab more, so the fourth row's
/// icon tells which kind is showing.
fn show_tab(win: &Win, tab: u32) -> Result<(), String> {
    let x = SKILL_FIRST_TAB.0 + SKILL_TAB_PITCH * tab as i32;
    win.click((x, SKILL_FIRST_TAB.1))?;
    pause(450);
    // The selected tab is pink, the others grey. (Rows are no proof: a Beginner tab can have four skills or more.)
    let y = SKILL_FIRST_TAB.1;
    let pink = Shot::take(win, SKILL_REGION).map_or(0, |s| s.count_near(SKILL_TAB_ON, 30, (x - 14, x - 6), (y - 4, y + 4)));
    if pink < 10 {
        return Err(format!("skill tab {tab} did not show"));
    }
    Ok(())
}

/// The AP to spend now, in order: the build's for the newest levels, or all STR without a build or a level.
fn ap_plan(build: Option<&CharBuild>, p: Progress) -> Vec<Stat> {
    match (build, p.level) {
        (Some(b), Some(l)) => take_known(&b.ap_queue(l), p.ap),
        _ => vec![Stat::Str; MAX_STAT_CLICKS as usize],
    }
}

/// The SP to spend now as (tab, row, skill) groups, newest level first. Job tabs only once the job is known.
fn sp_plan(build: Option<&CharBuild>, p: Progress) -> Vec<Vec<(u32, u32, String)>> {
    let (Some(b), Some(level)) = (build, p.level) else {
        // No build: the old rule, Three Snails (first Beginner row) up to three times.
        return vec![vec![(0, 0, "three_snails".to_string()); 3]];
    };
    let job_tab = match p.job {
        Some(j) if j >= 100 => 1,
        _ => 0,
    };
    // One pool of points across tabs: merge both tabs' levels, newest first.
    let mut by_level: std::collections::BTreeMap<std::cmp::Reverse<u16>, Vec<(u32, u32, String)>> = Default::default();
    for tab in 0..=job_tab {
        for (lvl, names) in b.sp_queue(level, tab) {
            for n in names {
                let row = b.skill(&n).map_or(0, |s| s.row);
                by_level.entry(std::cmp::Reverse(lvl)).or_default().push((tab, row, n));
            }
        }
    }
    let mut left = p.sp.map(|n| n as usize);
    let mut out = vec![];
    for (_, mut v) in by_level {
        if let Some(n) = left.as_mut() {
            if *n == 0 {
                break;
            }
            v.truncate(*n);
            *n -= v.len();
        }
        out.push(v);
    }
    out
}

/// Spend the unspent ability and skill points as the build says, then put learnt skills on their quick slots.
pub fn assign_all() -> Result<Spent, String> {
    let _ui = UI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let win = Win::find()?;
    let p = *PROGRESS.lock().unwrap_or_else(|e| e.into_inner());
    let build = BUILD.get();
    let mut spent = Spent::default();

    open(&win, "s", STAT_REGION, stat_window_open)?;
    for stat in ap_plan(build, p).into_iter().take(MAX_STAT_CLICKS as usize) {
        // Any AP left shows as an enabled STR "+" (and the others with it).
        let enabled = Shot::take(&win, STAT_REGION).map_or(false, |s| stat_window_open(&s) && plus_enabled(&s, stat_plus(Stat::Str)));
        if !enabled {
            break;
        }
        win.click(stat_plus(stat))?;
        spent.ap += 1;
        pause(450);
    }
    xdo(&["key", "s"])?;
    pause(400);

    if open(&win, "k", SKILL_REGION, skill_window_open).is_ok() {
        let result = spend_skills(&win, build, p, &mut spent);
        xdo(&["key", "k"])?;
        pause(300);
        result?;
    }
    Ok(spent)
}

fn spend_skills(win: &Win, build: Option<&CharBuild>, p: Progress, spent: &mut Spent) -> Result<(), String> {
    let mut tab: Option<u32> = None;
    let mut clicks = 0;
    let switch = |want: u32, tab: &mut Option<u32>| -> Result<(), String> {
        if *tab != Some(want) {
            show_tab(win, want)?;
            *tab = Some(want);
        }
        Ok(())
    };
    'levels: for group in sp_plan(build, p) {
        // Within a level a skill can wait on another's prerequisite (Slash Blast needs Power Strike): keep passing over
        // the level's points while one of them goes in.
        let mut pending = group;
        loop {
            let before = pending.len();
            let mut i = 0;
            while i < pending.len() {
                let (t, row, name) = pending[i].clone();
                switch(t, &mut tab)?;
                let enabled = Shot::take(win, SKILL_REGION).map_or(false, |s| skill_window_open(&s) && plus_enabled(&s, skill_plus(row)));
                if enabled && clicks < MAX_SKILL_CLICKS {
                    win.click(skill_plus(row))?;
                    clicks += 1;
                    spent.sp += 1;
                    info!("skill point on {name}");
                    pause(500);
                    pending.remove(i);
                } else {
                    i += 1;
                }
            }
            if pending.is_empty() || pending.len() == before {
                break;
            }
        }
        if !pending.is_empty() {
            // Nothing left to spend (or this level's skills are maxed or blocked): older levels' points were spent.
            break 'levels;
        }
    }

    // Quick slots for learnt skills (Power Strike on Shift ...): pick the icon up, drop it on the slot.
    let (Some(b), Some(level)) = (build, p.level) else { return Ok(()) };
    let job_tab = if p.job.is_some_and(|j| j >= 100) { 1 } else { 0 };
    let mut on_slots = vec![];
    for sk in b.slotted(level).into_iter().filter(|s| s.tab <= job_tab) {
        let slot = sk.slot.as_deref().unwrap_or_default();
        let Some(&(_, _, pos)) = QUICKSLOTS.iter().find(|(n, _, _)| *n == slot) else {
            warn!("build: unknown quick slot '{slot}' for {}", sk.name);
            continue;
        };
        if !win.slot_filled(pos) {
            switch(sk.tab, &mut tab)?;
            let (ix, iy) = skill_icon(sk.row);
            win.click((ix + 14, iy + 14))?;
            pause(400);
            win.click(pos)?;
            pause(500);
            if !win.slot_filled(pos) {
                warn!("{} did not land on the {slot} quick slot", sk.name);
                continue;
            }
            info!("{} put on the {slot} quick slot", sk.name);
            spent.slotted.push(sk.name.clone());
        }
        on_slots.push(sk.name.clone());
    }
    let mut g = SLOTTED.lock().unwrap_or_else(|e| e.into_inner());
    if g.0 != on_slots {
        *g = (on_slots, true);
    }
    Ok(())
}

/// Hash of the level box: it changes when the level does.
fn level_signature(win: &Win) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let (x, y) = win.scale((LEVEL_BOX.0, LEVEL_BOX.1));
        let (w, h) = win.size((LEVEL_BOX.2, LEVEL_BOX.3));
        let out = std::process::Command::new("import")
            .args(["-window", &win.id, "-crop", &format!("{w}x{h}+{x}+{y}"), "+repage", "-format", "%#", "info:"])
            .envs(crate::shop::x_env())
            .output()
            .ok()?;
        let sig = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!sig.is_empty()).then_some(sig)
    }
    #[cfg(target_os = "macos")]
    {
        Some(format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)))
    }
}

/// Spend points now, then again after every level-up. Runs on its own thread for the life of the process.
/// Set by the executor on a level-up or a job change: spend points on the next pass.
static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Ask the watcher to spend points now (after a level-up).
pub fn request_assign() {
    REQUESTED.store(true, Ordering::SeqCst);
}

pub fn spawn_watcher() {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        pause(6000);
        let mut last: Option<String> = None;
        let mut first = true;
        loop {
            let sig = Win::find().ok().and_then(|w| level_signature(&w));
            // The level box's pixels also change while a map loads, so a change there is not trusted on its own;
            // the reliable signal is the executor's request on a rise in maximum HP.
            let _changed = matches!((&last, &sig), (Some(a), Some(b)) if a != b);
            let requested = REQUESTED.swap(false, Ordering::SeqCst);
            if sig.is_some() && (first || requested) {
                if requested {
                    info!("level or job changed: spending ability and skill points");
                    pause(3500); // let the level-up effect and the window state settle
                }
                match assign_all() {
                    Ok(s) if s.ap + s.sp > 0 || !s.slotted.is_empty() => {
                        info!("spent {} ability point(s) and {} skill point(s); new on quick slots: {:?}", s.ap, s.sp, s.slotted)
                    }
                    Ok(_) => info!("no ability or skill points to spend"),
                    Err(e) => warn!("spending points failed: {e}"),
                }
                first = false;
            }
            if sig.is_some() {
                last = Win::find().ok().and_then(|w| level_signature(&w)).or(sig);
            }
            pause(2000);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warrior() -> CharBuild {
        CharBuild::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../builds/warrior.json")).unwrap()
    }

    #[test]
    fn without_a_build_everything_goes_to_str_and_three_snails() {
        assert!(ap_plan(None, Progress::default()).iter().all(|s| *s == Stat::Str));
        assert_eq!(sp_plan(None, Progress::default()), vec![vec![(0, 0, "three_snails".to_string()); 3]]);
    }

    #[test]
    fn a_job_tab_is_only_used_once_the_job_is_known() {
        let b = warrior();
        let p = Progress { level: Some(11), job: None, ap: None, sp: Some(4) };
        let flat: Vec<_> = sp_plan(Some(&b), p).concat();
        assert!(flat.iter().all(|(t, _, _)| *t == 0), "{flat:?}");
        let p = Progress { job: Some(100), ..p };
        let groups = sp_plan(Some(&b), p);
        // 4 SP at level 11 = level 11's three, then level 10's first (Nimble Feet on the Beginner tab, then Power Strike).
        assert_eq!(groups[0], vec![(1, 4, "power_strike".into()), (1, 4, "power_strike".into()), (1, 5, "slash_blast".into())]);
        assert_eq!(groups[1], vec![(0, 2, "nimble_feet".into())]);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn stat_buttons_are_a_column() {
        assert_eq!(stat_plus(Stat::Dex), (772, 540));
        assert_eq!(skill_plus(5), (759, 621));
        let b = warrior();
        let p = Progress { level: Some(15), job: Some(100), ap: Some(5), sp: None };
        assert_eq!(ap_plan(Some(&b), p), vec![Stat::Str, Stat::Str, Stat::Dex, Stat::Dex, Stat::Dex]);
    }
}
