//! NPC shop automation by clicking the client's own shop window (no packets, no memory writes).
//!
//! Layout constants are pixel positions in the 1422x900 game window the client runs in; they are
//! scaled by the real window size, so a resized window still lands on the same buttons. A shop trip is:
//! click the NPC, then for every sell tab sell stack after stack from one list slot (the list shifts up
//! after each sale, so slot `keep` is always the next stack to sell), buy, and leave. Whether a slot holds
//! an item is read from the pixels of its icon (an empty slot is a flat colour).

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;
use tracing::{info, warn};

const REF_W: f64 = 1422.0;
const REF_H: f64 = 900.0;

const ALL_TAB: (i32, i32) = (515, 400);
/// The shop window's meso counter (top-left, size), hashed to see whether a purchase went through.
const MESOS_BOX: (i32, i32, i32, i32) = (840, 358, 95, 16);
const SELL_BUTTON: (i32, i32) = (897, 335);
const BUY_BUTTON: (i32, i32) = (667, 335);
const LEAVE_BUTTON: (i32, i32) = (667, 315);
const DIALOG_OK: (i32, i32) = (762, 521);
/// OK of the "You don't have enough Mesos" notice (measured 2026-10-25, Lith Harbor Department Store).
const NOT_ENOUGH_OK: (i32, i32) = (812, 508);
const SELL_SLOT_X: i32 = 800;
const BUY_SLOT_X: i32 = 590;
const FIRST_SLOT_Y: i32 = 432;
const SLOT_PITCH: i32 = 40;
const VISIBLE_SLOTS: u32 = 6;
/// Icon area of sell slot 0 (top-left, size 28).
const ICON_X: i32 = 719;
const ICON_Y: i32 = 422;
/// Icon area of buy-list row 0 (left list), mirroring the sell list's icon-to-text offset (unverified).
const BUY_ICON_X: i32 = 509;
/// Where the mouse wheel scrolls the buy list.
const BUY_LIST_CENTER: (i32, i32) = (560, 520);
/// Rows of a shop's own list that can be bought from (beyond the visible six by scrolling).
const MAX_BUY_ROW: u32 = 60;
/// Grey-level deviation above which an icon area counts as holding an item.
const ICON_MIN_DEV: f64 = 0.05;
/// Stacks sold per tab at most, a guard against a dialog that never goes away.
const MAX_SALES: u32 = 40;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Buy {
    /// Row in the shop's item list, 0 = first.
    pub slot: u32,
    pub qty: u32,
    /// Price of one, for a step with a meso `reserve` (0 = unknown: then nothing is bought under a reserve).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub price: u32,
    /// Item id (e.g. 2000000 Red Potion): bought into an existing stack the server reports only a count, so the bot
    /// names the slot that grew with this.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub item: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShopJob {
    /// NPC position in reference-window pixels (1422x900).
    pub npc: [i32; 2],
    /// Inventory tabs to sell from: "equip", "use", "setup", "etc".
    #[serde(default)]
    pub sell: Vec<String>,
    /// Rows at the top of every sell list that are never sold (0..5), by position (used when the
    /// item at that position is unknown).
    #[serde(default)]
    pub keep: u32,
    /// Item ids never sold, wherever they sit in the list (requires the inventory to be known for
    /// that slot; falls back to `keep` alone otherwise).
    #[serde(default)]
    pub keep_items: Vec<u32>,
    #[serde(default)]
    pub buy: Vec<Buy>,
}

/// `ShopJob.sell` tab name -> the server's inventory tab number (1 equip, 2 use, 3 setup, 4 etc).
fn tab_number(name: &str) -> Option<u8> {
    INV_TABS.iter().position(|(n, _)| *n == name).map(|i| i as u8 + 1)
}

/// The next item to sell (its index into `items` and the sell-list row it currently sits at), given
/// the tab's items in slot order and which of them have already been sold this trip. An item is
/// protected from sale either by position (`keep`, for when its id is unknown) or by id (`keep_items`,
/// wherever it sits); protected items still occupy a row, since the list only compacts around sales.
fn next_sale(items: &[Option<u32>], keep: u32, keep_items: &[u32], sold: &[bool]) -> Option<(usize, u32)> {
    let mut row = 0u32;
    for (idx, item) in items.iter().enumerate() {
        if sold[idx] {
            continue;
        }
        let protected = (idx as u32) < keep || item.map_or(false, |id| keep_items.contains(&id));
        if protected {
            row += 1;
            continue;
        }
        return Some((idx, row));
    }
    None
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ShopReport {
    pub sold: u32,
    pub bought: u32,
}

fn tab_pos(name: &str) -> Option<(i32, i32)> {
    Some(match name {
        "equip" => (738, 400),
        "use" => (783, 400),
        "setup" => (826, 400),
        "etc" => (871, 400),
        _ => return None,
    })
}

pub(crate) struct Win {
    pub(crate) id: String,
    pub(crate) w: f64,
    pub(crate) h: f64,
    /// Where the reference frame's origin lies relative to the window: (0, 0) for the 1422x900 frame a desktop
    /// window manager draws, (-28, -97) for a client with no frame (a headless display), whose window is the
    /// 1366x768 game view alone.
    pub(crate) origin: (i32, i32),
}

/// The game view inside the 1422x900 reference frame (quest.rs measures against the same numbers).
const VIEW: (i32, i32, f64, f64) = (28, 97, 1366.0, 768.0);

impl Win {
    pub(crate) fn find() -> Result<Self, String> {
        let ids = xdo(&["search", "--name", "MapleStory"])?;
        let id = ids.lines().next().map(str::trim).filter(|s| !s.is_empty()).ok_or("game window not found")?.to_string();
        let geo = xdo(&["getwindowgeometry", "--shell", &id])?;
        let val = |k: &str| -> Option<f64> { geo.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).and_then(|v| v.trim().parse().ok()) };
        let (w, h) = (val("WIDTH").ok_or("no window width")?, val("HEIGHT").ok_or("no window height")?);
        Ok(Self::sized(id, w, h))
    }

    pub(crate) fn sized(id: String, w: f64, h: f64) -> Self {
        if (w, h) == (VIEW.2, VIEW.3) {
            return Self { id, w: REF_W, h: REF_H, origin: (-VIEW.0, -VIEW.1) };
        }
        Self { id, w, h, origin: (0, 0) }
    }

    /// A point in reference pixels -> window pixels.
    pub(crate) fn scale(&self, p: (i32, i32)) -> (i32, i32) {
        let (x, y) = self.size(p);
        (x + self.origin.0, y + self.origin.1)
    }

    /// A width/height in reference pixels -> window pixels.
    pub(crate) fn size(&self, d: (i32, i32)) -> (i32, i32) {
        (((d.0 as f64) * self.w / REF_W).round() as i32, ((d.1 as f64) * self.h / REF_H).round() as i32)
    }

    /// Move to `p` (reference pixels), pause briefly so the client registers the hover, then click.
    pub(crate) fn click(&self, p: (i32, i32)) -> Result<(), String> {
        let (x, y) = self.scale(p);
        xdo(&["mousemove", "--window", &self.id, &x.to_string(), &y.to_string(), "sleep", "0.15", "click", "1"]).map(|_| ())
    }

    /// Hash of the meso counter's pixels; it changes whenever the amount does.
    pub(crate) fn mesos_signature(&self) -> Option<String> {
        let (x, y) = self.scale((MESOS_BOX.0, MESOS_BOX.1));
        let (w, h) = self.size((MESOS_BOX.2, MESOS_BOX.3));
        let out = Command::new("import").args(["-window", &self.id, "-crop", &format!("{w}x{h}+{x}+{y}"), "+repage", "-format", "%#", "info:"]).envs(x_env()).output().ok()?;
        let sig = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!sig.is_empty()).then_some(sig)
    }

    /// True if the 28x28 area at (x, y) (reference pixels) holds an icon rather than a flat slot colour.
    pub(crate) fn cell_occupied(&self, x: i32, y: i32) -> bool {
        let (px, py) = self.scale((x, y));
        let side = (28.0 * self.w / REF_W).round() as i32;
        let out = Command::new("import")
            .args(["-window", &self.id, "-crop", &format!("{side}x{side}+{px}+{py}"), "+repage", "-colorspace", "Gray", "-format", "%[fx:standard_deviation]", "info:"])
            .envs(x_env())
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().map_or(true, |d| d > ICON_MIN_DEV),
            Err(_) => true,
        }
    }

    /// True if the sell-list row `row` holds an item.
    fn row_occupied(&self, row: u32) -> bool {
        let (x, y) = self.scale((ICON_X, ICON_Y + SLOT_PITCH * row as i32));
        let out = Command::new("import")
            .args(["-window", &self.id, "-crop", &format!("{}x{}+{x}+{y}", (28.0 * self.w / REF_W).round() as i32, (28.0 * self.h / REF_H).round() as i32), "+repage", "-colorspace", "Gray", "-format", "%[fx:standard_deviation]", "info:"])
            .envs(x_env())
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().map_or(false, |d| d > ICON_MIN_DEV),
            Err(_) => false,
        }
    }
}

/// Inventory window: where the item grid starts and how big a cell is (reference pixels).
const GRID_X: i32 = 81;
const GRID_Y: i32 = 195;
const GRID_PITCH: i32 = 36;
const INV_TABS: [(&str, (i32, i32)); 4] = [("equip", (79, 162)), ("use", (112, 162)), ("setup", (146, 162)), ("etc", (178, 162))];

static USING_ITEM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// When a requested cell turned out to be empty: do not try (or open the inventory) again until this time (ms since epoch).
static EMPTY_UNTIL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// How long an empty cell is believed to stay empty.
const EMPTY_BACKOFF_MS: u64 = 60_000;

/// Parse "inv:<tab>:<row>:<col>" (0-based row/col in that inventory tab).
pub fn parse_item_spec(spec: &str) -> Option<(&'static str, u32, u32)> {
    let mut it = spec.strip_prefix("inv:")?.split(':');
    let name = it.next()?;
    let tab = INV_TABS.iter().find(|(n, _)| *n == name)?.0;
    let (row, col): (u32, u32) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    (it.next().is_none() && row < 7 && col < 5).then_some((tab, row, col))
}

impl Win {
    /// The inventory window is drawn in near-white; three white probes on its frame mean it is open.
    fn inventory_open(&self) -> bool {
        [(100, 133), (200, 133), (150, 392)].iter().all(|&p| {
            let (x, y) = self.scale(p);
            let out = Command::new("import").args(["-window", &self.id, "-crop", &format!("6x6+{x}+{y}"), "+repage", "-colorspace", "Gray", "-format", "%[fx:mean]", "info:"]).envs(x_env()).output();
            out.ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok()).map_or(false, |m| m > 0.93)
        })
    }
}

/// Use (double-click) one inventory cell, e.g. a potion, opening and closing the inventory around it.
/// Runs on its own thread so the decision loop never waits; overlapping requests are dropped.
/// The potion cell was found empty within the last minute: the character is out of that item.
pub fn item_cell_empty() -> bool {
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    now_ms < EMPTY_UNTIL.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn use_item_async(spec: &str) {
    use std::sync::atomic::Ordering;
    let Some((tab, row, col)) = parse_item_spec(spec) else {
        warn!("bad item spec '{spec}' (want inv:<tab>:<row>:<col>)");
        return;
    };
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    if now_ms < EMPTY_UNTIL.load(Ordering::SeqCst) {
        return; // that cell was empty a moment ago: no potions, nothing to open the inventory for
    }
    if USING_ITEM.swap(true, Ordering::SeqCst) {
        return;
    }
    let spec = spec.to_string();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let _ui = UI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let win = Win::find()?;
            let opened_here = !win.inventory_open();
            if opened_here {
                xdo(&["key", "i"])?;
                pause(350);
            }
            let tab_pos = INV_TABS.iter().find(|(n, _)| *n == tab).unwrap().1;
            win.click(tab_pos)?;
            pause(150);
            let (x, y) = win.scale((GRID_X + GRID_PITCH * col as i32, GRID_Y + GRID_PITCH * row as i32));
            // Only use a cell that holds an item (an empty one is a flat colour); otherwise remember it for a while.
            if !win.cell_occupied(GRID_X + GRID_PITCH * col as i32 - 14, GRID_Y + GRID_PITCH * row as i32 - 14) {
                warn!("inventory cell {spec} is empty: nothing to use (not trying again for a minute)");
                EMPTY_UNTIL.store(now_ms + EMPTY_BACKOFF_MS, Ordering::SeqCst);
                if opened_here {
                    xdo(&["key", "i"])?;
                }
                return Ok(());
            }
            xdo(&["mousemove", "--window", &win.id, &x.to_string(), &y.to_string(), "sleep", "0.1", "click", "--repeat", "2", "--delay", "70", "1"])?;
            pause(200);
            if opened_here {
                xdo(&["key", "i"])?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            warn!("using {spec} failed: {e}");
        }
        USING_ITEM.store(false, Ordering::SeqCst);
    });
}

/// The quick slots in the lower right (reference-window centres) and the X key each one answers to.
/// Measured 2026-10-25: top row Shift / Ins / Hm / Pup, bottom row Ctrl / Del / End / Pdn.
pub const QUICKSLOTS: [(&str, &str, (i32, i32)); 8] = [
    ("shift", "Shift_L", (985, 738)),
    ("ins", "Insert", (1018, 738)),
    ("home", "Home", (1052, 738)),
    ("pgup", "Prior", (1087, 738)),
    ("ctrl", "Control_L", (983, 777)),
    ("del", "Delete", (1018, 777)),
    ("end", "End", (1053, 777)),
    ("pgdn", "Next", (1088, 777)),
];

/// An icon on a quick slot: dark or coloured pixels below the slot's key label. An empty slot is near-white with grey
/// key text, so a plain variance test reads every slot as full (measured 2026-10-25: empty slots 0-22 such pixels in
/// this box, the Ctrl attack icon 260).
const SLOT_FILLED_MIN: usize = 80;

pub(crate) fn icon_pixel((r, g, b): (u8, u8, u8)) -> bool {
    let (mx, mn) = (r.max(g).max(b) as i32, r.min(g).min(b) as i32);
    let light = (mx + mn) as f64 / 510.0;
    light < 0.72 || (mx - mn > 38 && light > 0.2 && light < 0.8)
}

impl Win {
    /// Is something on the quick slot centred at `pos` (reference pixels)?
    pub(crate) fn slot_filled(&self, pos: (i32, i32)) -> bool {
        crate::quest::Shot::take(self, (pos.0 - 11, pos.1 - 3, 22, 14)).map_or(true, |s| s.count_all(icon_pixel) >= SLOT_FILLED_MIN)
    }
}

/// Put an inventory item ("inv:use:0:3") on a quick slot ("ins") so a key press uses it: open the inventory, click the
/// item once (it sticks to the cursor), click the slot, close the inventory. Press-and-drag does not register in
/// this client; two separate clicks do. Returns the key to press. Blocks; holds the UI lock.
pub fn bind_to_quickslot(spec: &str, slot: &str) -> Result<String, String> {
    let (tab, row, col) = parse_item_spec(spec).ok_or_else(|| format!("bad item spec '{spec}' (want inv:<tab>:<row>:<col>)"))?;
    let &(_, key, pos) = QUICKSLOTS.iter().find(|(n, _, _)| *n == slot).ok_or_else(|| format!("unknown quick slot '{slot}'"))?;
    let _ui = UI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let win = Win::find()?;
    let opened_here = !win.inventory_open();
    if opened_here {
        xdo(&["key", "i"])?;
        pause(400);
    }
    let tab_pos = INV_TABS.iter().find(|(n, _)| *n == tab).unwrap().1;
    win.click(tab_pos)?;
    pause(300);
    let cell = (GRID_X + GRID_PITCH * col as i32, GRID_Y + GRID_PITCH * row as i32);
    let close = |w: &Win| if opened_here { xdo(&["key", "i"]).map(|_| ()) } else { Ok(()) };
    if !win.cell_occupied(cell.0 - 14, cell.1 - 14) {
        close(&win)?;
        return Err(format!("inventory cell {spec} is empty"));
    }
    win.click(cell)?;
    pause(400);
    win.click(pos)?;
    pause(500);
    close(&win)?;
    pause(300);
    if !win.slot_filled(pos) {
        return Err(format!("the {slot} quick slot is still empty after placing {spec}"));
    }
    Ok(key.to_string())
}

impl Win {
    /// Hash of a 28x28 inventory cell's pixels (top-left in reference pixels): it changes when the item there does.
    fn cell_signature(&self, x: i32, y: i32) -> Option<String> {
        let (px, py) = self.scale((x, y));
        let side = (28.0 * self.w / REF_W).round() as i32;
        let out = Command::new("import").args(["-window", &self.id, "-crop", &format!("{side}x{side}+{px}+{py}"), "+repage", "-format", "%#", "info:"]).envs(x_env()).output().ok()?;
        let sig = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!sig.is_empty()).then_some(sig)
    }
}

/// Equip the item in an Equip-tab cell ("inv:equip:0:0") by double-clicking it. True when the cell changed (the item
/// went on, or the one it replaced came back in its place); false when nothing happened (the level or stats are too low
/// for it, or the cell was empty). Blocks; holds the UI lock.
pub fn equip(spec: &str) -> Result<bool, String> {
    let (tab, row, col) = parse_item_spec(spec).ok_or_else(|| format!("bad item spec '{spec}' (want inv:equip:<row>:<col>)"))?;
    if tab != "equip" {
        return Err(format!("{spec} is not in the Equip tab"));
    }
    let _ui = UI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let win = Win::find()?;
    let opened_here = !win.inventory_open();
    if opened_here {
        xdo(&["key", "i"])?;
        pause(400);
    }
    let tab_pos = INV_TABS.iter().find(|(n, _)| *n == tab).unwrap().1;
    win.click(tab_pos)?;
    pause(300);
    let cell = (GRID_X + GRID_PITCH * col as i32, GRID_Y + GRID_PITCH * row as i32);
    let close = || if opened_here { xdo(&["key", "i"]).map(|_| ()) } else { Ok(()) };
    if !win.cell_occupied(cell.0 - 14, cell.1 - 14) {
        close()?;
        return Ok(false);
    }
    let before = win.cell_signature(cell.0 - 14, cell.1 - 14);
    let (x, y) = win.scale(cell);
    xdo(&["mousemove", "--window", &win.id, &x.to_string(), &y.to_string(), "sleep", "0.1", "click", "--repeat", "2", "--delay", "70", "1"])?;
    pause(900);
    // Move the cursor off the cell: its tooltip would change the pixels too.
    let (mx, my) = win.scale((700, 140));
    xdo(&["mousemove", "--window", &win.id, &mx.to_string(), &my.to_string()])?;
    pause(300);
    let after = win.cell_signature(cell.0 - 14, cell.1 - 14);
    close()?;
    Ok(before.is_some() && before != after)
}

pub(crate) fn x_env() -> Vec<(String, String)> {
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
    let auth = std::env::var("XAUTHORITY").unwrap_or_default();
    vec![("DISPLAY".into(), display), ("XAUTHORITY".into(), auth)]
}

pub(crate) fn xdo(args: &[&str]) -> Result<String, String> {
    let out = Command::new("xdotool").args(args).envs(x_env()).output().map_err(|e| format!("xdotool {args:?}: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Only one UI automation (shop, quest dialog, item use, stat points) may click at a time.
pub(crate) static UI_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn pause(ms: u64) {
    sleep(Duration::from_millis(ms));
}

fn row_y(base_x: i32, row: u32) -> (i32, i32) {
    (base_x, FIRST_SLOT_Y + SLOT_PITCH * row as i32)
}

/// Hashes of the visible buy-list rows' icons.
fn buy_rows(win: &Win) -> Vec<Option<String>> {
    (0..VISIBLE_SLOTS).map(|r| win.cell_signature(BUY_ICON_X, ICON_Y + SLOT_PITCH * r as i32)).collect()
}

/// Rows one scroll moved the list by: the new top row is the old row `k`. None when the list did not move (its end)
/// or the shift cannot be told.
fn scroll_shift(before: &[Option<String>], after: &[Option<String>]) -> Option<u32> {
    if before == after || after.first().map_or(true, |a| a.is_none()) {
        return None;
    }
    (1..before.len()).find(|&k| before[k] == after[0] && before[k..].iter().zip(after.iter()).all(|(a, b)| a == b)).map(|k| k as u32)
}

/// Bring buy-list row `slot` into view with the mouse wheel and return its visible row. The list starts at the top
/// (the shop was just opened). How far one wheel step scrolls is measured from the icons, not assumed.
fn show_buy_row(win: &Win, slot: u32, top: &mut u32) -> Result<u32, String> {
    while slot >= *top + VISIBLE_SLOTS {
        let before = buy_rows(win);
        let (x, y) = win.scale(BUY_LIST_CENTER);
        xdo(&["mousemove", "--window", &win.id, &x.to_string(), &y.to_string(), "click", "5"])?;
        pause(300);
        let after = buy_rows(win);
        let k = scroll_shift(&before, &after).ok_or_else(|| format!("could not scroll the shop list to row {slot} (stuck at top row {top})"))?;
        *top += k;
    }
    slot.checked_sub(*top).ok_or_else(|| format!("row {slot} is above the scrolled list"))
}

/// Run one shop trip against the already open map: the character must be in the shop's map.
/// `inventory` is the last known (tab, slot, item) triples, used only to protect `keep_items` from sale.
pub fn run(job: &ShopJob, inventory: &[(u8, u16, Option<u32>)]) -> Result<ShopReport, String> {
    if job.keep >= VISIBLE_SLOTS {
        return Err(format!("keep must be below {VISIBLE_SLOTS}"));
    }
    for t in &job.sell {
        tab_pos(t).ok_or_else(|| format!("unknown sell tab '{t}'"))?;
    }
    let _ui = UI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let win = Win::find()?;
    let mut report = ShopReport::default();

    win.click((job.npc[0], job.npc[1]))?;
    pause(1400);
    // Evidence for filling in buy rows: the shop's list as it opened (NEON_DEBUG_DIR, like failed quest clicks).
    if let Ok(dir) = std::env::var("NEON_DEBUG_DIR") {
        let path = format!("{dir}/shop-{}-{}.png", job.npc[0], job.npc[1]);
        let _ = Command::new("import").args(["-window", &win.id, &path]).envs(x_env()).output();
        info!("shop: list saved to {path}");
    }

    for tab in &job.sell {
        win.click(tab_pos(tab).unwrap())?;
        pause(600);
        let mut sales = 0;
        if job.keep_items.is_empty() || tab_number(tab).is_none() {
            while win.row_occupied(job.keep) && sales < MAX_SALES {
                win.click(row_y(SELL_SLOT_X, job.keep))?;
                pause(250);
                win.click(SELL_BUTTON)?;
                pause(500);
                win.click(DIALOG_OK)?; // the quantity box is pre-filled with the whole stack
                pause(700);
                sales += 1;
            }
        } else {
            let tab_no = tab_number(tab).unwrap();
            let mut slots: Vec<(u16, Option<u32>)> = inventory.iter().filter(|(t, _, _)| *t == tab_no).map(|&(_, slot, item)| (slot, item)).collect();
            slots.sort_by_key(|&(slot, _)| slot);
            let items: Vec<Option<u32>> = slots.into_iter().map(|(_, item)| item).collect();
            let mut sold = vec![false; items.len()];
            while sales < MAX_SALES {
                let Some((idx, row)) = next_sale(&items, job.keep, &job.keep_items, &sold) else { break };
                if !win.row_occupied(row) {
                    break; // the UI disagrees with what we expected to be there; stop rather than sell the wrong item
                }
                win.click(row_y(SELL_SLOT_X, row))?;
                pause(250);
                win.click(SELL_BUTTON)?;
                pause(500);
                win.click(DIALOG_OK)?;
                pause(700);
                sold[idx] = true;
                sales += 1;
            }
        }
        if sales >= MAX_SALES {
            warn!("shop: stopped selling '{tab}' after {MAX_SALES} sales");
        }
        info!("shop: sold {sales} stacks from the {tab} tab");
        report.sold += sales;
    }

    if !job.buy.is_empty() {
        // Selling switches the left panel to "Buy Back"; the buy orders refer to the shop's own list.
        win.click(ALL_TAB)?;
        pause(500);
    }
    let mut top = 0;
    for b in &job.buy {
        if b.slot >= MAX_BUY_ROW || b.qty == 0 || b.qty > 9999 {
            warn!("shop: skipping bad buy order {b:?}");
            continue;
        }
        let row = match show_buy_row(&win, b.slot, &mut top) {
            Ok(r) => r,
            Err(e) => {
                warn!("shop: {e}; not buying list row {}", b.slot);
                continue;
            }
        };
        // Ask for the full amount; if the meso counter does not move (the shop says "You don't have enough Mesos"),
        // close that notice and ask for three quarters as much, down to one.
        let mut qty = b.qty;
        loop {
            let before = win.mesos_signature();
            win.click(row_y(BUY_SLOT_X, row))?;
            pause(250);
            win.click(BUY_BUTTON)?;
            pause(500);
            xdo(&["key", "ctrl+a"])?;
            xdo(&["type", "--delay", "60", &qty.to_string()])?;
            pause(200);
            win.click(DIALOG_OK)?;
            pause(800);
            let after = win.mesos_signature();
            if before.is_none() || before != after {
                info!("shop: bought {qty} of list row {}", b.slot);
                report.bought += qty;
                break;
            }
            // The notice sits where the quantity box was; its OK is a little higher.
            win.click(NOT_ENOUGH_OK)?;
            pause(500);
            if qty == 1 {
                warn!("shop: could not afford even one of list row {}", b.slot);
                break;
            }
            qty = (qty * 3 / 4).max(1);
            warn!("shop: not enough mesos for list row {}; trying {qty}", b.slot);
        }
    }

    win.click(LEAVE_BUTTON)?;
    pause(500);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frameless_window_is_the_game_view_of_the_reference_frame() {
        // Desktop: the 1422x900 frame is the reference itself.
        let f = Win::sized("1".into(), 1422.0, 900.0);
        assert_eq!(f.scale((700, 520)), (700, 520));
        // Headless display: the window is the 1366x768 game view at (28, 97) of the reference frame.
        let v = Win::sized("2".into(), 1366.0, 768.0);
        assert_eq!(v.scale((700, 520)), (672, 423));
        assert_eq!(v.size((95, 16)), (95, 16), "sizes are not shifted");
    }

    #[test]
    fn job_parses_with_defaults() {
        let j: ShopJob = serde_json::from_str(r#"{"npc":[452,518]}"#).unwrap();
        assert_eq!(j, ShopJob { npc: [452, 518], sell: vec![], keep: 0, keep_items: vec![], buy: vec![] });
        let j: ShopJob = serde_json::from_str(r#"{"npc":[1,2],"sell":["etc"],"keep":1,"keep_items":[2000003],"buy":[{"slot":0,"qty":50}]}"#).unwrap();
        assert_eq!(j.buy[0].qty, 50);
        assert_eq!(j.keep_items, vec![2000003]);
    }

    #[test]
    fn unknown_tab_is_rejected_before_touching_the_game() {
        let j = ShopJob { npc: [0, 0], sell: vec!["bank".into()], keep: 0, keep_items: vec![], buy: vec![] };
        assert!(run(&j, &[]).unwrap_err().contains("unknown sell tab"));
        let j = ShopJob { npc: [0, 0], sell: vec![], keep: 9, keep_items: vec![], buy: vec![] };
        assert!(run(&j, &[]).is_err());
    }

    #[test]
    fn tab_names_map_to_server_tab_numbers() {
        assert_eq!(tab_number("equip"), Some(1));
        assert_eq!(tab_number("use"), Some(2));
        assert_eq!(tab_number("setup"), Some(3));
        assert_eq!(tab_number("etc"), Some(4));
        assert_eq!(tab_number("bank"), None);
    }

    #[test]
    fn keep_by_position_matches_the_old_fixed_row_behaviour() {
        // Five stacks, none identified: keep the first two by position, sell the rest in order.
        let items = vec![None, None, None, None, None];
        let mut sold = vec![false; items.len()];
        for want_idx in [2, 3, 4] {
            let (idx, row) = next_sale(&items, 2, &[], &sold).unwrap();
            assert_eq!((idx, row), (want_idx, 2), "always offered at the fixed 'keep' row until it empties");
            sold[idx] = true;
        }
        assert_eq!(next_sale(&items, 2, &[], &sold), None);
    }

    #[test]
    fn keep_items_protects_an_id_wherever_it_sits() {
        // A quest item (4000005) sits in the middle of an otherwise sellable etc tab.
        let items = vec![Some(1032001), Some(4000005), Some(1032002), Some(4000005)];
        let mut sold = vec![false; items.len()];
        let (idx, row) = next_sale(&items, 0, &[4000005], &sold).unwrap();
        assert_eq!((idx, row), (0, 0));
        sold[idx] = true;
        // The kept item at idx 1 still occupies row 1; the next sellable one is idx 2, now row 1.
        let (idx, row) = next_sale(&items, 0, &[4000005], &sold).unwrap();
        assert_eq!((idx, row), (2, 1));
        sold[idx] = true;
        // Both quest-item stacks remain; nothing left to sell.
        assert_eq!(next_sale(&items, 0, &[4000005], &sold), None);
    }

    #[test]
    fn item_specs_parse() {
        assert_eq!(parse_item_spec("inv:use:0:2"), Some(("use", 0, 2)));
        assert_eq!(parse_item_spec("inv:etc:1:4"), Some(("etc", 1, 4)));
        assert_eq!(parse_item_spec("inv:use:0:9"), None);
        assert_eq!(parse_item_spec("inv:bank:0:0"), None);
        assert_eq!(parse_item_spec("F2"), None);
        assert_eq!(parse_item_spec("inv:use:0:2:1"), None);
    }

    #[test]
    fn slot_icons_are_dark_or_coloured_pixels() {
        assert!(!icon_pixel((240, 240, 245)), "slot background");
        assert!(!icon_pixel((205, 205, 212)), "key label");
        assert!(icon_pixel((176, 120, 140)), "the pink Ctrl attack icon");
        assert!(icon_pixel((60, 160, 60)), "a green skill icon");
        assert!(icon_pixel((40, 40, 40)), "outline");
    }

    #[test]
    fn a_scroll_is_measured_from_the_icons() {
        let rows = |v: &[&str]| v.iter().map(|s| Some(s.to_string())).collect::<Vec<_>>();
        let before = rows(&["a", "b", "c", "d", "e", "f"]);
        assert_eq!(scroll_shift(&before, &rows(&["b", "c", "d", "e", "f", "g"])), Some(1));
        assert_eq!(scroll_shift(&before, &rows(&["d", "e", "f", "g", "h", "i"])), Some(3));
        assert_eq!(scroll_shift(&before, &before), None, "the end of the list");
        assert_eq!(scroll_shift(&before, &rows(&["x", "y", "z", "u", "v", "w"])), None, "cannot tell: never guessed");
    }

    #[test]
    fn tabs_and_rows_are_laid_out_in_reference_pixels() {
        assert_eq!(tab_pos("etc"), Some((871, 400)));
        assert_eq!(row_y(SELL_SLOT_X, 2), (800, 512));
    }
}
