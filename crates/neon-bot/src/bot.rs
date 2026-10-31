//! Executor: consumes `GameState`s from the stream, asks the `Brain` what to
//! do, and carries the actions out (with human-like timing and idle breaks).

use crate::brain::{Action, Brain, Dir, Mode};
use crate::control::ControlWatcher;
use crate::input::InputController;
use crate::nav::Vert;
use crate::notify::{Kind, Notifier};
use crate::plan::{Plan, PlanRunner};
use crate::rules::{Effect, Rule, RuleEngine};
use crate::status::{StatusExtra, StatusWriter};
use neon_core::anti_detect::AntiDetect;
use neon_state::GameState;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Where key events go. `DryRun` only logs, so decisions can be checked
/// against live data without touching the game.
pub trait Keys {
    fn down(&mut self, key: &str) -> Result<(), String>;
    fn up(&mut self, key: &str) -> Result<(), String>;
    /// Click the "revive in town" button of the death dialog.
    fn click_revive(&mut self) -> Result<(), String>;
    /// Click a mouse button (1 left, 3 right) where the pointer is: a `mouse:left` / `mouse:right` macro.
    fn click_button(&mut self, button: u8) -> Result<(), String>;
    /// False for the dry run: shop trips click the real window.
    fn clicks_enabled(&self) -> bool {
        true
    }
}

impl Keys for InputController {
    fn down(&mut self, key: &str) -> Result<(), String> {
        self.key_down(key)
    }
    fn up(&mut self, key: &str) -> Result<(), String> {
        self.key_up(key)
    }
    fn click_revive(&mut self) -> Result<(), String> {
        // The dialog's button sits just left of centre, a little below the middle of the window
        // (0.492, 0.578 of the 1422x900 reference frame).
        crate::shop::Win::find()?.click((700, 520))
    }
    fn click_button(&mut self, button: u8) -> Result<(), String> {
        match button {
            3 => self.right_click(),
            _ => self.click(),
        }
    }
}

/// HP potions in the order to prefer: Red, Orange, White Potion (item ids).
const HP_POTIONS: [u32; 3] = [2000000, 2000001, 2000002];

pub struct DryRun;

impl Keys for DryRun {
    fn down(&mut self, key: &str) -> Result<(), String> {
        info!("[dry-run] key down {}", key);
        Ok(())
    }
    fn up(&mut self, key: &str) -> Result<(), String> {
        info!("[dry-run] key up {}", key);
        Ok(())
    }
    fn click_revive(&mut self) -> Result<(), String> {
        info!("[dry-run] click revive");
        Ok(())
    }
    fn click_button(&mut self, button: u8) -> Result<(), String> {
        info!("[dry-run] mouse button {button}");
        Ok(())
    }
    fn clicks_enabled(&self) -> bool {
        false
    }
}

/// The X button number for a `mouse:left` / `mouse:right` macro target.
fn mouse_button(key: &str) -> Option<u8> {
    match key.strip_prefix("mouse:")? {
        "left" => Some(1),
        "right" => Some(3),
        _ => None,
    }
}

/// A key event scheduled for later, so a "tap" never blocks the decision loop.
enum KeyEv {
    Down(String),
    Up(String),
}

/// Frames older than this are not acted on (the character has moved on since).
const MAX_FRAME_AGE_MS: u64 = 400;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct Bot<K: Keys> {
    /// Shared claim directory and our name in it; when the claims were last exchanged.
    party: Option<(std::path::PathBuf, String)>,
    party_synced: Option<Instant>,
    /// `--hp-key auto`: find the HP potion in the server's inventory and put it on the Ins quick slot; the item bound
    /// so far, and when binding was last tried.
    auto_hp: bool,
    hp_bound: Option<u32>,
    hp_bind_tried: Option<Instant>,
    /// Use slots named by a purchase (the server names a stack only when it is created), the last inventory seen, and
    /// a purchase waiting to name the slot that grew: (item, Use slots and counts before, when).
    inv_names: std::collections::HashMap<(u8, u16), u32>,
    last_inv: Vec<neon_state::InvSlot>,
    name_pending: Option<(u32, Vec<(u16, u32)>, Instant)>,
    /// Potions given as inventory cells (--hp-key / --mp-key inv:...) and their quick slots: put on the slot again after
    /// a shop trip that bought something (at start the cell may have been empty).
    pot_cells: Vec<(String, &'static str, bool)>,
    sched: Vec<(Instant, KeyEv)>,
    control: Option<ControlWatcher>,
    panic: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    notifier: Option<Notifier>,
    last_level: Option<u16>,
    plan: Option<PlanRunner>,
    rules: RuleEngine,
    trace: Option<std::io::BufWriter<std::fs::File>>,
    latency_ms: f64,
    stale_warned: bool,
    brain: Brain,
    keys: K,
    anti: AntiDetect,
    held: Option<Dir>,
    held_v: Option<Vert>,
    /// The basic attack key is held down (the brain's Attack(true)).
    held_attack: Option<String>,
    status: StatusWriter,
    /// Shadow simulation fed with the same inputs (`--sim-log`).
    shadow: Option<crate::sim::Shadow>,
    /// Inputs given since the last frame, for the shadow.
    sim_in: crate::sim::Input,
    last_max_hp: Option<u32>,
    /// (level, job) last seen, to notice level-ups and job advancements.
    last_progress: Option<(Option<u16>, Option<u16>)>,
}

fn dir_key(d: Dir) -> &'static str {
    match d {
        Dir::Left => "Left",
        Dir::Right => "Right",
    }
}

impl<K: Keys> Bot<K> {
    /// Share mob claims with the other bots through `dir` (see `party`), under this session's name.
    pub fn with_party(mut self, dir: Option<std::path::PathBuf>, name: &str) -> Self {
        self.party = dir.map(|d| (d, name.to_string()));
        self
    }

    /// Publish our target and take the peers' claims into the brain, a few times a second.
    fn sync_party(&mut self, state: &GameState) {
        let Some((dir, name)) = self.party.clone() else {
            return;
        };
        let Some(map) = state.player.map_id else {
            return;
        };
        if self
            .party_synced
            .map_or(false, |t| t.elapsed() < Duration::from_millis(300))
        {
            return;
        }
        self.party_synced = Some(Instant::now());
        let now = now_ms();
        let claim = crate::party::Claim {
            name: name.clone(),
            map,
            oid: self.brain.target_oid(),
            ts: now,
        };
        if let Err(e) = crate::party::publish(&dir, &claim) {
            warn!("party: cannot write the claim: {e}");
        }
        let peers = crate::party::read_peers(&dir, &name, map, now);
        if let Some(runner) = self.plan.as_mut() {
            runner.set_peer_maps(crate::party::peer_maps(&dir, &name, now));
        }
        self.brain
            .set_peer_claims(crate::party::blocked_oids(&name, claim.oid, &peers));
    }

    pub fn new(brain: Brain, keys: K, status: StatusWriter) -> Self {
        Self {
            party: None,
            party_synced: None,
            auto_hp: false,
            hp_bound: None,
            hp_bind_tried: None,
            inv_names: Default::default(),
            last_inv: vec![],
            name_pending: None,
            pot_cells: vec![],
            rules: RuleEngine::default(),
            control: None,
            panic: None,
            notifier: None,
            last_level: None,
            plan: None,
            trace: None,
            sched: vec![],
            latency_ms: 0.0,
            stale_warned: false,
            brain,
            keys,
            anti: AntiDetect::new(),
            held: None,
            held_v: None,
            held_attack: None,
            status,
            shadow: None,
            sim_in: Default::default(),
            last_max_hp: None,
            last_progress: None,
        }
    }

    /// Watch this file for pause/stop commands and live settings from the control panel.
    pub fn with_control(mut self, path: std::path::PathBuf) -> Self {
        self.control = Some(ControlWatcher::new(path));
        self
    }

    /// Stop immediately when this flag is raised (the global panic hotkey).
    /// `--hp-key auto`: bind whatever HP potion the inventory holds to Ins (see `bind_hp_from_inventory`).
    pub fn with_auto_hp(mut self, on: bool) -> Self {
        self.auto_hp = on;
        self
    }

    /// Put the best HP potion the server says we carry on the Ins quick slot, if that is not the item bound already.
    /// The quick slot remembers the item, not the cell: buying more later needs nothing.
    fn bind_hp_from_inventory(&mut self, state: &GameState) {
        if !self.auto_hp
            || !self.keys.clicks_enabled()
            || self
                .hp_bind_tried
                .map_or(false, |t| t.elapsed() < Duration::from_secs(30))
        {
            return;
        }
        // A purchase into a stack names its slot here (the server only reported the count).
        if let Some((item, before, at)) = self.name_pending.clone() {
            let grew = state.inventory.iter().find(|s| {
                s.tab == 2
                    && before
                        .iter()
                        .find(|(sl, _)| *sl == s.slot)
                        .map_or(true, |(_, q)| s.qty > *q)
            });
            if let Some(s) = grew {
                self.inv_names.insert((2, s.slot), item);
                self.name_pending = None;
            } else if at.elapsed() > Duration::from_secs(15) {
                self.name_pending = None;
            }
        }
        let named = |s: &neon_state::InvSlot| {
            s.item
                .or_else(|| self.inv_names.get(&(s.tab, s.slot)).copied())
        };
        let best = HP_POTIONS
            .iter()
            .find_map(|&id| {
                state
                    .inventory
                    .iter()
                    .find(|s| s.tab == 2 && named(s) == Some(id) && s.qty > 0)
            })
            .cloned();
        let Some(mut slot) = best else { return };
        slot.item = named(&slot);
        if self.hp_bound == slot.item {
            return;
        }
        self.hp_bind_tried = Some(Instant::now());
        self.release_all();
        let i = slot.slot as u32 - 1;
        let spec = format!("inv:use:{}:{}", i / 5, i % 5); // five cells per row
        match crate::shop::bind_to_quickslot(&spec, "ins") {
            Ok(k) => {
                self.hp_bound = slot.item;
                self.brain.config_mut().keys.hp_pot = k.clone();
                self.brain.pots_empty = false;
                self.announce(format!(
                    "HP potion {} (x{}, {spec}) put on the ins quick slot: pressing {k}",
                    slot.item.unwrap_or(0),
                    slot.qty
                ));
            }
            Err(e) => warn!("could not put the HP potion {spec} on the ins quick slot: {e}"),
        }
    }

    /// Potion inventory cells to put on quick slots after a shop trip: (cell spec, slot, is HP).
    pub fn with_pot_cells(mut self, cells: Vec<(String, &'static str, bool)>) -> Self {
        self.pot_cells = cells;
        self
    }

    /// After buying: put potions that are still pressed as inventory cells (their cell was empty at start) on their quick
    /// slot, so the next press is a single key.
    fn bind_bought_potions(&mut self) {
        for (spec, slot, hp) in self.pot_cells.clone() {
            let cfg = self.brain.config_mut();
            let key = if hp {
                &mut cfg.keys.hp_pot
            } else {
                &mut cfg.keys.mp_pot
            };
            if *key != spec {
                continue; // already on its quick slot
            }
            match crate::shop::bind_to_quickslot(&spec, slot) {
                Ok(k) => {
                    let msg = format!(
                        "bought potions ({spec}) put on the {slot} quick slot: pressing {k}"
                    );
                    *key = k;
                    self.announce(msg);
                }
                Err(e) => warn!("could not put {spec} on the {slot} quick slot: {e}"),
            }
        }
        self.brain.pots_empty = false;
    }

    pub fn with_panic(
        mut self,
        flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Self {
        self.panic = flag;
        self
    }

    pub fn with_notifier(mut self, n: Notifier) -> Self {
        self.notifier = Some(n);
        self
    }

    /// Record a notice in the event log and raise a Discord alert when it is one we report.
    fn announce(&mut self, msg: String) {
        let level = self.last_level;
        if let Some(n) = self.notifier.as_mut() {
            n.notice(&msg, level);
        }
        self.status.event(msg);
    }

    pub fn with_rules(mut self, rules: Vec<Rule>) -> Self {
        self.rules = RuleEngine::new(rules);
        self
    }

    /// Sell/buy at the NPC the plan step names. Blocks for several seconds with every key released.
    fn run_shop_job(&mut self, job: &crate::shop::ShopJob) {
        self.release_all();
        let mut bought = 0;
        if !self.keys.clicks_enabled() {
            self.announce("[dry-run] skipping the shop trip".to_string());
        } else {
            let inventory: Vec<(u8, u16, Option<u32>)> = self
                .last_inv
                .iter()
                .map(|s| (s.tab, s.slot, s.item))
                .collect();
            match crate::shop::run(job, &inventory) {
                Ok(r) => {
                    bought = r.bought;
                    if bought > 0 {
                        self.bind_bought_potions();
                        if let Some(b) = job.buy.iter().find(|b| HP_POTIONS.contains(&b.item)) {
                            let before = self
                                .last_inv
                                .iter()
                                .filter(|s| s.tab == 2)
                                .map(|s| (s.slot, s.qty))
                                .collect();
                            self.name_pending = Some((b.item, before, Instant::now()));
                            self.hp_bind_tried = None;
                        }
                    }
                    self.announce(format!(
                        "shop trip done: sold {} stacks, bought {} items",
                        r.sold, r.bought
                    ));
                }
                Err(e) => self.announce(format!("shop trip failed: {e}")),
            }
        }
        if let Some(runner) = self.plan.as_mut() {
            runner.shop_finished(bought);
        }
    }

    /// Run a calibrated click sequence (item enhancement). Blocks with every key released.
    fn run_ui_job(&mut self, job: &crate::ui::UiJob) {
        self.release_all();
        if !self.keys.clicks_enabled() {
            self.announce(format!(
                "[dry-run] skipping ui step {} ({} clicks)",
                job.name,
                job.sequence().len()
            ));
        } else {
            match crate::ui::run(job) {
                Ok(n) => self.announce(format!("ui step {}: {n} click(s) made", job.name)),
                Err(e) => self.announce(format!("ui step {} failed: {e}", job.name)),
            }
        }
        if let Some(runner) = self.plan.as_mut() {
            runner.ui_finished();
        }
    }

    /// Equip the inventory item the plan step names. Blocks briefly with every key released.
    fn run_equip_job(&mut self, spec: &str) {
        self.release_all();
        if !self.keys.clicks_enabled() {
            self.announce(format!("[dry-run] skipping equipping {spec}"));
        } else {
            match crate::shop::equip(spec) {
                Ok(true) => self.announce(format!("equipped {spec}")),
                Ok(false) => self.announce(format!(
                    "could not equip {spec} (empty cell, or level/stats too low): going on"
                )),
                Err(e) => self.announce(format!("equipping {spec} failed: {e}")),
            }
        }
        if let Some(runner) = self.plan.as_mut() {
            runner.equip_finished();
        }
    }

    /// Talk to the quest NPC the plan step names. Blocks for several seconds with every key released.
    /// False when the conversation failed: the plan must not go on as if the quest had been done.
    fn run_quest_job(&mut self, job: &crate::quest::QuestJob) {
        self.release_all();
        let (mut ran, mut accepted) = (true, true);
        if !self.keys.clicks_enabled() {
            self.announce("[dry-run] skipping the quest conversation".to_string());
        } else {
            match crate::quest::run(job) {
                Ok(r) => {
                    accepted = r.starts_with("accepted");
                    self.announce(format!(
                        "quest {} {}: {r}",
                        job.quest,
                        if job.complete { "hand-in" } else { "accept" }
                    ));
                }
                Err(e) => {
                    ran = false;
                    self.announce(format!("quest {} failed: {e}", job.quest));
                }
            }
        }
        if let Some(runner) = self.plan.as_mut() {
            runner.quest_finished(ran, accepted);
        }
    }

    /// Run this plan/schedule (also replaceable live from the control panel).
    pub fn with_plan(mut self, plan: Plan) -> Self {
        self.plan = Some(PlanRunner::new(plan));
        self
    }

    /// Apply a pending control command. Returns true when the run should end.
    fn poll_control(&mut self) -> bool {
        let Some(w) = self.control.as_mut() else {
            return false;
        };
        let Some(c) = w.poll() else { return false };
        c.settings.apply(self.brain.config_mut());
        if let Some(r) = c.rules {
            self.status.event(format!("{} rule(s) loaded", r.len()));
            self.rules = RuleEngine::new(r);
        }
        if let Some(p) = c.plan {
            if p.steps.is_empty() {
                self.plan = None;
                self.brain.set_schedule_pause(None);
                self.status.event("plan cleared");
            } else {
                self.status.event(format!(
                    "plan '{}' loaded ({} steps)",
                    p.name,
                    p.steps.len()
                ));
                self.plan = Some(PlanRunner::new(p));
            }
        }
        self.brain.set_manual_pause(c.paused);
        if c.paused {
            self.release_all();
        }
        self.status.event(match (c.stop, c.paused) {
            (true, _) => "stop requested from the control panel".to_string(),
            (_, true) => "paused from the control panel".to_string(),
            _ => "settings updated from the control panel".to_string(),
        });
        if c.stop {
            self.release_all();
            return true;
        }
        false
    }

    /// Write one line per decision tick (ts, position, target, what we did) to this file.
    pub fn with_shadow(mut self, shadow: crate::sim::Shadow) -> Self {
        self.shadow = Some(shadow);
        self
    }

    pub fn with_trace(mut self, path: &std::path::Path) -> Self {
        self.trace = std::fs::File::create(path)
            .ok()
            .map(std::io::BufWriter::new);
        self
    }

    /// Run until the stream closes, the session limit hits, or `stop` is set.
    pub fn run(&mut self, rx: Receiver<GameState>) -> Result<(), String> {
        info!("Bot running");
        let mut last_report = std::time::Instant::now();
        let mut latest: Option<GameState> = None;
        let mut last_frame = Instant::now();

        loop {
            if self
                .panic
                .as_ref()
                .map_or(false, |f| f.load(std::sync::atomic::Ordering::SeqCst))
            {
                warn!("PANIC key pressed: releasing everything and stopping");
                self.announce("stop requested: panic key pressed".to_string());
                self.finish(latest.as_ref());
                return Ok(());
            }
            if self.poll_control() {
                info!("Stopped from the control panel");
                self.finish(latest.as_ref());
                return Ok(());
            }
            let wait = self
                .sched
                .iter()
                .map(|(t, _)| t.saturating_duration_since(Instant::now()))
                .min()
                .unwrap_or(Duration::from_secs(2))
                .min(Duration::from_secs(2));
            match rx.recv_timeout(wait) {
                Ok(s) => {
                    latest = Some(s);
                    last_frame = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.flush_due();
                    // Stream went quiet: never leave a key held on stale info.
                    if last_frame.elapsed() > Duration::from_secs(2) {
                        warn!("No state for 2s; releasing keys");
                        self.release_all();
                        last_frame = Instant::now();
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.finish(latest.as_ref());
                    return Err("state stream closed".into());
                }
            }
            self.flush_due();
            // Only act on the freshest frame; skip any backlog.
            while let Ok(s) = rx.try_recv() {
                latest = Some(s);
            }
            let Some(state) = latest.as_ref() else {
                continue;
            };

            // End-to-end age of this frame: stream read -> pipe -> parse -> here.
            let age = now_ms().saturating_sub(state.ts_ms);
            self.latency_ms = if self.latency_ms == 0.0 {
                age as f64
            } else {
                self.latency_ms * 0.9 + age as f64 * 0.1
            };
            if age > MAX_FRAME_AGE_MS {
                if !self.stale_warned {
                    warn!("frames are {age} ms old; not acting on them");
                    self.status
                        .event(format!("stream lag {age} ms: holding still"));
                    self.stale_warned = true;
                }
                self.set_held(None);
                continue;
            }
            self.stale_warned = false;

            self.sync_party(state);
            self.bind_hp_from_inventory(state);
            self.last_inv = state.inventory.clone();
            if let Some(runner) = self.plan.as_mut() {
                let now = now_ms();
                runner.set_camera(state.camera);
                runner.set_kills(self.brain.stats.kills_by_mob.clone());
                runner.set_exp(state.player.level, state.player.exp);
                runner.set_quests(state.quests.clone());
                runner.set_player(
                    state
                        .player
                        .map_id
                        .map(|_| (state.player.x, state.player.y)),
                );
                runner.set_job(state.player.job);
                runner.set_items(state.items_gained.clone());
                runner.set_mesos(state.player.meso);
                runner.set_pots_empty(self.brain.pots_empty);
                let o = runner.tick(
                    now,
                    state.player.level,
                    state.player.map_id,
                    self.brain.stats.mobs_killed,
                    self.brain.config_mut(),
                );
                for n in o.notes {
                    info!("{}", n);
                    self.announce(n);
                }
                self.brain
                    .set_schedule_pause((!o.active).then(|| o.reason.clone()));
                if let Some(job) = o.shop {
                    self.run_shop_job(&job);
                }
                if let Some(job) = o.quest {
                    self.run_quest_job(&job);
                }
                if let Some(spec) = o.equip {
                    self.run_equip_job(&spec);
                }
                if let Some(job) = o.ui {
                    self.run_ui_job(&job);
                }
                if o.resupplied {
                    self.brain.pots_empty = false;
                }
                if let Some(why) = o.failed {
                    self.announce(format!("plan stopped: {why}"));
                    self.finish(Some(state));
                    return Ok(());
                }
                if o.finished {
                    info!("Plan finished; stopping");
                    self.announce("plan finished".to_string());
                    self.finish(Some(state));
                    return Ok(());
                }
            }
            self.last_level = state.player.level;
            for fx in self.rules.eval(state) {
                match fx {
                    Effect::Tap(k) => self.execute(Action::Tap(k)),
                    Effect::Pause => {
                        self.brain.set_manual_pause(true);
                        self.release_all();
                    }
                    Effect::Stop => {
                        self.announce("stop requested: a rule asked for it".to_string());
                        self.finish(Some(state));
                        return Ok(());
                    }
                    Effect::Notify(m) => self.announce(m),
                    Effect::Set(settings) => settings.apply(self.brain.config_mut()),
                }
            }
            if let Some(sh) = self.shadow.as_mut() {
                sh.observe(state, self.sim_in);
                self.status
                    .set_sim(sh.body().map(|b| crate::status::SimInfo {
                        x: b.x,
                        y: b.y,
                        grounded: b.grounded,
                        summary: sh.summary(),
                    }));
            }
            self.sim_in = crate::sim::Input::default();
            // A level-up (the server's level, or maximum HP rising with it) or a job advancement brings points to spend.
            crate::stats::observe(crate::stats::Progress {
                level: state.player.level,
                job: state.player.job,
                ap: state.player.available_ap,
                sp: state.player.sp,
            });
            if let Some(m) = state.player.max_hp {
                if self
                    .last_max_hp
                    .map_or(false, |prev| m > prev && m < prev + 200)
                {
                    crate::stats::request_assign();
                }
                self.last_max_hp = Some(m);
            }
            let progress = (state.player.level, state.player.job);
            if self.last_progress.is_some_and(|(l, j)| {
                (progress.0 > l && l.is_some()) || (progress.1 != j && progress.1.is_some())
            }) {
                crate::stats::request_assign();
            }
            self.last_progress = Some(progress);
            // Skills newly on quick slots join the fight (the build's attacks and buffs, replacing its earlier ones).
            if let (Some(b), Some(on)) = (crate::stats::build(), crate::stats::take_slotted()) {
                let (attacks, buffs) = b.fight_cfg(&on);
                let names: Vec<&str> = attacks
                    .iter()
                    .map(|a| a.label.as_str())
                    .chain(buffs.iter().map(|m| m.label.as_str()))
                    .map(|l| l.trim_start_matches("build:"))
                    .collect();
                self.announce(format!("skills in use: {}", names.join(", ")));
                let cfg = self.brain.config_mut();
                cfg.rotation.retain(|k| !k.label.starts_with("build:"));
                cfg.macros.retain(|m| !m.label.starts_with("build:"));
                cfg.rotation.extend(attacks);
                cfg.macros.extend(buffs);
            }
            if self.brain.config().keys.hp_pot.starts_with("inv:") && crate::shop::item_cell_empty()
            {
                self.brain.pots_empty = true;
            }
            let before = self.brain.stats.clone();
            let actions = self.brain.tick(state);
            if let Some(w) = self.trace.as_mut() {
                use std::io::Write;
                let tgt = self
                    .brain
                    .target_pos()
                    .map_or("-".to_string(), |(x, y)| format!("{x},{y}"));
                let acts: Vec<String> = actions
                    .iter()
                    .filter(|a| !matches!(a, Action::Notify(_)))
                    .map(|a| format!("{a:?}"))
                    .collect();
                let _ = writeln!(
                    w,
                    "{} pos={},{} gr={:?} tgt={} mobs={} | {} | {}",
                    state.ts_ms,
                    state.player.x,
                    state.player.y,
                    state.player.grounded,
                    tgt,
                    state.mobs.len(),
                    self.brain.decision,
                    acts.join(" ")
                );
                let _ = w.flush();
            }
            for a in actions {
                self.execute(a);
            }
            self.sim_in.dir = self.sim_in.dir.or(self.held);
            self.sim_in.climb |= self.held_v.is_some();
            let b = self.brain.stats.clone();
            if b.mobs_killed > before.mobs_killed {
                self.status.event(format!("kill #{}", b.mobs_killed));
            }
            if b.levels_gained > before.levels_gained {
                self.announce(format!("LEVEL UP -> {}", state.player.level.unwrap_or(0)));
            }
            if b.hp_pots > before.hp_pots {
                self.status.event("used HP potion");
            }
            if b.mp_pots > before.mp_pots {
                self.status.event("used MP potion");
            }
            if b.loot_presses > before.loot_presses {
                self.status.event("pressed loot key");
            }
            self.status.update(
                state,
                &self.brain.stats,
                self.brain.mode,
                true,
                self.latency_ms,
                StatusExtra {
                    pause_reason: &self.brain.pause_reason,
                    config: self.brain.config(),
                    plan: self.plan.as_ref().map(|p| p.summary()),
                    rules: self.rules.rules(),
                },
            );

            let max_deaths = self.brain.config().max_deaths;
            if max_deaths > 0 && self.brain.stats.deaths >= max_deaths {
                warn!("{} deaths: stopping", self.brain.stats.deaths);
                self.announce(format!(
                    "stop requested: {} deaths reached the limit of {}",
                    self.brain.stats.deaths, max_deaths
                ));
                self.finish(Some(state));
                return Ok(());
            }
            if let (Some(t), Some(l)) = (self.status_target(), state.player.level) {
                if l >= t {
                    info!("Reached level {}; stopping", l);
                    self.announce(format!("reached target level {}; stopping", l));
                    self.finish(Some(state));
                    return Ok(());
                }
            }

            if self.anti.session_should_end() {
                info!("Session limit reached; stopping");
                self.announce("session limit reached; stopping".to_string());
                self.finish(Some(state));
                return Ok(());
            }
            if self.anti.blackout_now() && self.brain.mode == Mode::Running {
                // Server maintenance (docs/authorization.md): stand still, keep draining the stream, resume afterwards.
                self.release_all();
                info!("Maintenance window: standing still until it ends");
                self.announce(
                    "server maintenance window; standing still until it ends".to_string(),
                );
                while self.anti.blackout_now() {
                    let _ = rx.recv_timeout(Duration::from_millis(500));
                }
                info!("Maintenance window over; resuming");
            }
            if self.anti.should_idle()
                && self.brain.mode == Mode::Running
                && self.brain.safe_to_idle(state)
            {
                self.release_all();
                let d = self.anti.start_idle();
                info!("Idle break for {:?}", d);
                // Watch the stream while idle: end the break the moment something threatens the character.
                let until = Instant::now() + d;
                let hp_at_start = state.player.hp_percent();
                let mut wrote = Instant::now();
                while Instant::now() < until {
                    match rx.recv_timeout(Duration::from_millis(250)) {
                        Ok(f) => {
                            // Keep the status fresh so the panel shows the bot as up (and why it stands still).
                            if wrote.elapsed() >= Duration::from_secs(1) {
                                wrote = Instant::now();
                                let why = format!(
                                    "idle break, {} s left",
                                    until.saturating_duration_since(Instant::now()).as_secs()
                                );
                                self.status.update(
                                    &f,
                                    &self.brain.stats,
                                    self.brain.mode,
                                    true,
                                    self.latency_ms,
                                    StatusExtra {
                                        pause_reason: &why,
                                        config: self.brain.config(),
                                        plan: self.plan.as_ref().map(|p| p.summary()),
                                        rules: self.rules.rules(),
                                    },
                                );
                            }
                            let hurt = matches!((hp_at_start, f.player.hp_percent()), (Some(a), Some(b)) if b + 5.0 < a);
                            let near = f.mobs.iter().any(|m| {
                                (m.x - f.player.x).abs() < 150 && (m.y - f.player.y).abs() < 80
                            });
                            if hurt || near {
                                info!(
                                    "idle break cut short: {}",
                                    if hurt { "HP dropped" } else { "mob close" }
                                );
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                self.anti.idle_complete();
                // Everything buffered during the break is stale.
                while rx.try_recv().is_ok() {}
            }

            if last_report.elapsed() > Duration::from_secs(30) {
                let b = &self.brain.stats;
                info!(
                    "attacks={} kills={} hp_pots={} mp_pots={} levels+={} ap={} maps={}",
                    b.attacks,
                    b.mobs_killed,
                    b.hp_pots,
                    b.mp_pots,
                    b.levels_gained,
                    b.ap_spent,
                    b.map_changes
                );
                last_report = std::time::Instant::now();
            }
        }
    }

    fn finish(&mut self, state: Option<&GameState>) {
        self.release_all();
        if let Some(p) = self.plan.as_ref() {
            self.status.set_plan(Some(p.summary()));
        }
        self.status
            .finish(state, &self.brain.stats, self.brain.mode);
    }

    fn status_target(&self) -> Option<u16> {
        self.status.target_level()
    }

    fn execute(&mut self, a: Action) {
        match a {
            Action::Notify(m) => {
                info!("{}", m);
                self.announce(m);
            }
            Action::Move(d) => self.set_held(d),
            Action::Attack(on) => self.set_attack(on),
            Action::Vert(v) => self.set_vert(v),
            Action::Revive => {
                if let Err(e) = self.keys.click_revive() {
                    warn!("revive click failed: {}", e);
                }
            }
            Action::Nudge(d, ms) => {
                self.sim_in.dir = Some(d);
                if self.held.is_none() && self.keys.down(dir_key(d)).is_ok() {
                    self.sched.push((
                        Instant::now() + Duration::from_millis(ms),
                        KeyEv::Up(dir_key(d).into()),
                    ));
                }
            }
            Action::DropDown => {
                self.sim_in.jump = true;
                self.sim_in.down = true;
                let jump = self.brain_jump_key();
                if self.keys.down("Down").is_ok() {
                    let hold = self.anti.human_key_hold();
                    let t = Instant::now() + Duration::from_millis(70);
                    self.sched.push((t, KeyEv::Down(jump.clone())));
                    self.sched.push((t + hold, KeyEv::Up(jump)));
                    self.sched.push((
                        t + hold + Duration::from_millis(60),
                        KeyEv::Up("Down".into()),
                    ));
                    self.anti.record_action();
                }
            }
            Action::Tap(k) if k.starts_with("inv:") => {
                if self.keys.clicks_enabled() {
                    crate::shop::use_item_async(&k);
                } else {
                    info!("[dry-run] use item {k}");
                }
            }
            Action::Tap(k) if mouse_button(&k).is_some() => {
                if let Err(e) = self.keys.click_button(mouse_button(&k).unwrap_or(1)) {
                    warn!("{}", e);
                    return;
                }
                self.anti.record_action();
            }
            Action::Tap(k) => {
                if k == self.brain_jump_key() {
                    self.sim_in.jump = true;
                }
                let hold = self.anti.human_key_hold();
                // Pressing a key that is still down from the last tap would be swallowed.
                if self
                    .sched
                    .iter()
                    .any(|(_, e)| matches!(e, KeyEv::Up(x) if *x == k))
                {
                    return;
                }
                if let Err(e) = self.keys.down(&k) {
                    warn!("{}", e);
                    return;
                }
                self.sched.push((Instant::now() + hold, KeyEv::Up(k)));
                self.anti.record_action();
            }
        }
    }

    /// Run every scheduled key event whose time has come.
    fn flush_due(&mut self) {
        let now = Instant::now();
        let mut i = 0;
        while i < self.sched.len() {
            if self.sched[i].0 <= now {
                let (_, ev) = self.sched.remove(i);
                let r = match &ev {
                    KeyEv::Down(k) => self.keys.down(k),
                    KeyEv::Up(k) => self.keys.up(k),
                };
                r.unwrap_or_else(|e| warn!("{}", e));
            } else {
                i += 1;
            }
        }
    }

    fn set_held(&mut self, d: Option<Dir>) {
        if self.held == d {
            return;
        }
        if let Some(old) = self.held.take() {
            self.keys
                .up(dir_key(old))
                .unwrap_or_else(|e| warn!("{}", e));
        }
        if let Some(new) = d {
            match self.keys.down(dir_key(new)) {
                Ok(()) => self.held = Some(new),
                Err(e) => warn!("{}", e),
            }
        }
    }

    fn set_vert(&mut self, v: Option<Vert>) {
        if self.held_v == v {
            return;
        }
        let name = |v: Vert| match v {
            Vert::Up => "Up",
            Vert::Down => "Down",
        };
        if let Some(old) = self.held_v.take() {
            self.keys.up(name(old)).unwrap_or_else(|e| warn!("{}", e));
        }
        if let Some(new) = v {
            match self.keys.down(name(new)) {
                Ok(()) => self.held_v = Some(new),
                Err(e) => warn!("{}", e),
            }
        }
    }

    fn brain_jump_key(&self) -> String {
        self.brain.config().keys.jump.clone()
    }

    fn release_all(&mut self) {
        // Keys we pressed for a tap must come up now; presses not yet made are cancelled.
        for (_, ev) in std::mem::take(&mut self.sched) {
            if let KeyEv::Up(k) = ev {
                self.keys.up(&k).unwrap_or_else(|e| warn!("{}", e));
            }
        }
        self.set_held(None);
        self.set_vert(None);
        self.set_attack(false);
        // Always let go of the attack key too, held or not by our count: a Ctrl left down (seen after a plan finished)
        // keeps the character swinging in place, unable to walk.
        let k = self.brain.config().keys.attack.clone();
        let _ = self.keys.up(&k);
    }

    /// Hold or let go of the basic attack key.
    fn set_attack(&mut self, on: bool) {
        match (on, self.held_attack.take()) {
            (true, None) => {
                let k = self.brain.config().keys.attack.clone();
                match self.keys.down(&k) {
                    Ok(()) => self.held_attack = Some(k),
                    Err(e) => warn!("{}", e),
                }
            }
            (true, held) => self.held_attack = held,
            (false, Some(k)) => self.keys.up(&k).unwrap_or_else(|e| warn!("{}", e)),
            (false, None) => {}
        }
    }
}

impl<K: Keys> Drop for Bot<K> {
    fn drop(&mut self) {
        self.release_all();
    }
}

#[cfg(test)]
mod mouse_macro_tests {
    use super::mouse_button;

    #[test]
    fn mouse_targets_map_to_x_buttons_and_keys_do_not() {
        assert_eq!(mouse_button("mouse:left"), Some(1));
        assert_eq!(mouse_button("mouse:right"), Some(3));
        assert_eq!(mouse_button("mouse:middle"), None);
        assert_eq!(mouse_button("F5"), None);
    }
}
