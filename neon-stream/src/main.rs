//! Neon Stream
//!
//! Reads the game's memory through static-root pointer chains (chains.json) and
//! emits one `neon_state::GameState` JSON line per tick.
//!
//! Resource use is bounded on purpose: fixed small reads per tick, and the
//! optional mob scan walks a fixed number of bytes per tick. Run it niced:
//!   nice -n 19 neon-stream ...

mod chains;
mod foothold;
mod fuse;
mod net;
mod outgoing;
mod recv;
mod send;
mod aes;
mod rects;
mod stackpos;
mod scanner;

use chains::{Chains, Mem};
use clap::Parser;
use neon_state::{Capabilities, Drop as FieldDrop, GameState, Mob, OtherPlayer, Player};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "neon-stream")]
#[command(about = "Neon MapleStory live data streamer")]
struct Args {
    /// PID of MapleStory process (auto-detect if not provided)
    #[arg(long)]
    pid: Option<u32>,

    /// Where the largest HP/MP seen are remembered across restarts (default: seen-max.json next to the chains).
    /// One file per character: another character's maximum makes the bot think it is nearly dead.
    #[arg(long)]
    seen_file: Option<std::path::PathBuf>,

    #[arg(long, short)]
    verbose: bool,

    /// Experimental: read the position by voting among thread-stack slots (see stackpos.rs) instead of the fixed chain
    #[arg(long)]
    stack_pos: bool,

    /// Static-root chains file
    #[arg(long, default_value = "neon-calib/chains.json")]
    chains: PathBuf,

    /// "jsonl" (for the bot) or "text" (for humans)
    #[arg(long, default_value = "jsonl")]
    format: String,

    /// Tick interval in ms (position is a 16-byte read, so 20 ms is cheap)
    #[arg(long, default_value = "20")]
    interval: u64,

    /// Incoming opcode that means "lie detector active" (hex, e.g. 0x1a3); repeatable. Unknown until one is seen:
    /// run with --opcode-log, trigger/await a check, and look for an opcode that suddenly appears.
    #[arg(long = "ld-opcode", value_parser = parse_hex_u16)]
    ld_opcodes: Vec<u16>,

    /// Write a histogram of every incoming opcode (count and sizes) to this JSON file every few seconds
    #[arg(long)]
    opcode_log: Option<std::path::PathBuf>,

    /// Print what every chain currently reads, then exit. Run this first after
    /// a game restart or client patch.
    #[arg(long)]
    verify: bool,

    /// Mob source: "off" or "rects" (collision-rect signature scan). With --net the rects are scanned anyway while the
    /// network's mob positions are stale (we control the mobs), and give those mobs their live positions.
    #[arg(long, default_value = "off")]
    mobs: String,

    /// Bytes of heap the mob scan may walk per tick (KiB)
    #[arg(long, default_value = "2048")]
    scan_kb_per_tick: u64,

    /// Bytes of heap the foothold (map geometry) scan may walk per tick (KiB)
    #[arg(long, default_value = "8192")]
    foothold_kb_per_tick: u64,

    /// Disable the foothold scan (no map geometry, no entity filtering)
    #[arg(long)]
    no_footholds: bool,

    /// Read mobs and other players from the game's network traffic (needs `sudo -n tcpdump`).
    /// Nothing is injected or modified in the game.
    #[arg(long)]
    net: bool,

    /// Replay a pcap capture through the network decoder, print a summary, and exit
    #[arg(long)]
    net_replay: Option<PathBuf>,

    /// tcpdump interface for --net
    #[arg(long, default_value = "any")]
    net_iface: String,

    /// Enable outgoing packet injection (experimental)
    #[arg(long)]
    outgoing: bool,

    /// Initial IV for outgoing packets (hex, 8 chars, e.g. 2defc2e1)
    /// Default: 2defc2e1 (from handshake bytes 9..12)
    #[arg(long)]
    outgoing_iv: Option<String>,

    /// Send a test packet (opcode in hex, e.g. 0001 for ping)
    #[arg(long, value_parser = parse_hex_u16)]
    send_test: Option<u16>,
}

fn main() {
    let args = Args::parse();
    if args.verbose {
        std::env::set_var("RUST_LOG", "debug");
    }
    // Logs go to stderr so stdout stays a clean data stream.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let text = std::fs::read_to_string(&args.chains).unwrap_or_else(|e| {
        eprintln!("cannot read {:?}: {}", args.chains, e);
        std::process::exit(1);
    });
    let chains = Chains::from_json(&text).unwrap_or_else(|e| {
        eprintln!("bad chains file: {}", e);
        std::process::exit(1);
    });

    if let Some(path) = &args.net_replay {
        replay_pcap(path);
        return;
    }

    let pid = args.pid.unwrap_or_else(find_maplestory_pid);
    tracing::info!("Using PID {}", pid);
    let scanner = scanner::MemoryScanner::new(pid).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });

    if args.verify {
        for line in chains.verify(&scanner) {
            println!("{}", line);
        }
        return;
    }

    let mut tracker = match args.mobs.as_str() {
        _ if args.net && args.mobs != "off-all" => Some(rects::RectTracker::new(scanner.rw_anon_regions().unwrap_or_default())),
        "rects" => {
            let regions = scanner.rw_anon_regions().unwrap_or_default();
            tracing::info!("mob scan: {} regions, {} KiB/tick", regions.len(), args.scan_kb_per_tick);
            Some(rects::RectTracker::new(regions))
        }
        _ => None,
    };

    let stdout = std::io::stdout();
    let mut rt = Runtime::default();
    rt.seen_path = args.seen_file.clone().or_else(|| args.chains.parent().map(|d| d.join("seen-max.json")));
    rt.use_stack = args.stack_pos;
    rt.map_dir = args.chains.parent().map(|d| d.join("maps"));
    if let Some(dir) = &rt.map_dir {
        rt.map.known = std::fs::read_dir(dir)
            .map(|rd| rd.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".json")?.parse().ok()).collect())
            .unwrap_or_default();
        tracing::info!("{} known map ids", rt.map.known.len());
    }
    if let Some(prev) = rt.seen_path.as_deref().and_then(SeenMax::load) {
        tracing::info!("seeding max HP/MP from {:?}: {:?}", rt.seen_path, prev);
        rt.seen = prev;
    }
    let mut fh = FootholdState { enabled: !args.no_footholds, ..Default::default() };
    let mut netctx: Option<NetCtx> = if args.net { Some(start_net_live(&args.net_iface, NetOpts { packets: None, own_pid: Some(pid), alert_opcodes: args.ld_opcodes.iter().map(|&o| (o, net::AlertKind::LieDetector)).collect(), opcode_log: args.opcode_log.clone() })) } else { None };

    // Outgoing packet builder
    let mut outgoing_builder: Option<outgoing::PacketBuilder> = if args.outgoing {
        let iv = if let Some(iv_str) = &args.outgoing_iv {
            let bytes = hex::decode(iv_str).unwrap_or_else(|_| {
                eprintln!("invalid IV hex, using default");
                vec![0x2d, 0xef, 0xc2, 0xe1]
            });
            if bytes.len() == 4 {
                bytes.try_into().unwrap()
            } else {
                [0x2d, 0xef, 0xc2, 0xe1]
            }
        } else {
            // Try to read from memory, fall back to default
            outgoing::read_iv_from_memory(pid).unwrap_or([0x2d, 0xef, 0xc2, 0xe1])
        };
        tracing::info!("outgoing packets enabled, initial IV: {:02x?}", iv);
        Some(outgoing::PacketBuilder::new(iv))
    } else {
        None
    };

    // Send a test packet if requested
    if let Some(opcode) = args.send_test {
        if let Some(builder) = outgoing_builder.as_mut() {
            let packet = builder.build(opcode, &[]);
            tracing::info!("test packet: opcode={:04x}, len={}, iv={:02x?}", packet.opcode, packet.data.len(), packet.iv_used);
            eprintln!("Packet bytes: {}", packet.data.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
        }
    }
    // The inventory as last known: the server only reports slots that change.
    if let Some(n) = netctx.as_mut() {
        n.tracker.inventory = rt.seen.inventory.iter().map(|&(t, sl, it, q)| ((t, sl), (it, q))).collect();
    }
    let foothold_budget = args.foothold_kb_per_tick * 1024;
    let mut seq = 0u64;
    loop {
        if !scanner.alive() {
            tracing::error!("game process {} exited", pid);
            std::process::exit(2);
        }
        seq += 1;
        let regions = || scanner.rw_anon_regions().unwrap_or_default();
        let state = scan_all(&scanner, &chains, &mut rt, &mut fh, &regions, foothold_budget, tracker.as_mut(), netctx.as_mut(), args.scan_kb_per_tick * 1024, seq);

        let mut out = stdout.lock();
        let written = match args.format.as_str() {
            "text" => write_text(&mut out, &state),
            _ => writeln!(out, "{}", state.to_line()),
        };
        // Consumer went away (broken pipe): stop instead of spinning forever.
        if written.and_then(|_| out.flush()).is_err() {
            break;
        }
        drop(out);
        std::thread::sleep(std::time::Duration::from_millis(args.interval));
    }
}

/// Accepts a changed value only after it has been seen `need` frames in a row.
/// The map-id word reads garbage while loading and occasionally in between, and a
/// false "map changed" restarts scans and makes the bot stop moving.
#[derive(Default)]
struct Debounce {
    /// Map ids that have a map file: anything else is garbage (dialogs and loading screens leave junk in the word).
    /// Empty = no map directory, then any id from 1 up is plausible.
    known: std::collections::HashSet<u32>,
    stable: Option<u32>,
    /// Candidate value, how many frames in a row, and when it was first seen.
    cand: Option<(u32, u32, u64)>,
}

impl Debounce {
    /// A new value must persist this many frames AND this long. Frame count alone is not enough:
    /// at 50 fps five frames is 100 ms, shorter than the garbage the loader passes through.
    const NEED: u32 = 5;
    const NEED_MS: u64 = 600;

    fn current(&self) -> Option<u32> {
        self.stable
    }

    fn feed(&mut self, v: Option<u32>, now_ms: u64) -> Option<u32> {
        // Maple Island's maps are 1..1021, so small ids are real; 0 and the 4.29e9 loading garbage are not. With map
        // files at hand only their ids count (an open dialog left 27 there long enough to pass the debounce).
        let plausible = v.filter(|m| (1..=999_999_999).contains(m) && (self.known.is_empty() || self.known.contains(m)));
        match plausible {
            None => self.cand = None, // garbage/unreadable: keep the last stable value
            Some(m) if Some(m) == self.stable => self.cand = None,
            Some(m) => {
                let (n, since) = match self.cand {
                    Some((c, n, t)) if c == m => (n + 1, t),
                    _ => (1, now_ms),
                };
                self.cand = Some((m, n, since));
                let first = self.stable.is_none() && n >= 2;
                if first || (n >= Self::NEED && now_ms.saturating_sub(since) >= Self::NEED_MS) {
                    self.stable = Some(m);
                    self.cand = None;
                }
            }
        }
        self.stable
    }
}

/// Highest HP/MP seen at a given level, remembered across runs so the estimate
/// is right even when the stream starts while HP is low.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
struct SeenMax {
    level: u16,
    max_hp: u32,
    max_mp: u32,
    /// Quest states last reported by the server, kept across restarts (the server only sends changes).
    #[serde(default)]
    quests: std::collections::BTreeMap<u32, (u8, String)>,
    /// Job and mesos last reported by the server (it only sends them when they change).
    #[serde(default)]
    job: Option<u16>,
    #[serde(default)]
    meso: Option<u64>,
    /// Inventory slots the server reported: (tab, slot, item if known, quantity).
    #[serde(default)]
    inventory: Vec<(u8, u16, Option<u32>, u32)>,
}

impl SeenMax {
    fn load(path: &std::path::Path) -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    fn save(&self, path: &std::path::Path) {
        if let Ok(t) = serde_json::to_string(self) {
            let _ = std::fs::write(path, t);
        }
    }

    /// Fold a reading in. A different level starts fresh (max grows on level-up).
    /// Returns true when the stored values changed.
    fn observe(&mut self, level: Option<u16>, hp: Option<u32>, mp: Option<u32>) -> bool {
        let mut changed = false;
        if let Some(l) = level {
            if self.level != l {
                *self = SeenMax { level: l, quests: std::mem::take(&mut self.quests), job: self.job, meso: self.meso, inventory: std::mem::take(&mut self.inventory), ..Default::default() };
                changed = true;
            }
        }
        if let Some(h) = hp.filter(|h| *h > self.max_hp) { self.max_hp = h; changed = true; }
        if let Some(m) = mp.filter(|m| *m > self.max_mp) { self.max_mp = m; changed = true; }
        changed
    }
}

/// Highest HP/MP seen this run, used as max when no max-field root exists.
#[derive(Default)]
struct Runtime {
    /// When the remembered mesos were last written to the --seen-file.
    meso_saved_ms: u64,
    seen: SeenMax,
    seen_path: Option<std::path::PathBuf>,
    map: Debounce,
    /// Stack-vote position source (see stackpos.rs); only with `--stack-pos`.
    use_stack: bool,
    stack: stackpos::StackPos,
    map_dir: Option<std::path::PathBuf>,
    ground_map: Option<u32>,
    /// Last good position and when we saw it; bridges brief read dropouts.
    last_pos: Option<((i32, i32), u64)>,
    /// When HP first read zero (None while HP is above zero): a death needs it to persist.
    zero_hp_since: Option<u64>,
    /// Network mobs joined with the memory rects (fuse.rs), and the floors of the current map that rects must stand on.
    fuser: fuse::Fuser,
    floors: Option<(u32, Vec<neon_state::Foothold>, bool)>,
}

/// Dead only when HP has read zero continuously for this long: a single garbage read must not
/// look like a death.
const DEAD_AFTER_MS: u64 = 400;

/// Update the zero-HP timer and say whether the character is dead.
fn dead_state(since: &mut Option<u64>, hp: Option<u32>, hp_ok: bool, now_ms: u64) -> Option<bool> {
    match (hp, hp_ok) {
        (Some(0), true) => {
            let t = *since.get_or_insert(now_ms);
            Some(now_ms.saturating_sub(t) >= DEAD_AFTER_MS)
        }
        (Some(_), true) => {
            *since = None;
            Some(false)
        }
        _ => None, // unreadable: do not guess, and do not reset a running timer
    }
}

/// Raises when the server has been silent for `limit_ms`: the game normally never goes that quiet
/// (pings, other players, mobs), so silence means a dropped connection or a frozen client.
struct StallDetector {
    last_count: u64,
    last_change_ms: Option<u64>,
    limit_ms: u64,
}

impl StallDetector {
    fn new(limit_ms: u64) -> Self {
        Self { last_count: 0, last_change_ms: None, limit_ms }
    }

    /// `count` is the running number of framed server packets.
    fn update(&mut self, count: u64, now_ms: u64) -> bool {
        if self.last_change_ms.is_none() || count != self.last_count {
            self.last_count = count;
            self.last_change_ms = Some(now_ms);
            return false;
        }
        now_ms.saturating_sub(self.last_change_ms.unwrap_or(now_ms)) >= self.limit_ms
    }
}

/// Map geometry state: re-scanned whenever the map id changes.
#[derive(Default)]
struct FootholdState {
    enabled: bool,
    scanner: Option<foothold::FootholdScanner>,
    list: Vec<neon_state::Foothold>,
    /// Geometry is attached to exactly one frame after each completed sweep.
    send_once: bool,
    map_id: Option<u32>,
}

/// How long a stale position may stand in for a failed read.
const POS_HOLD_MS: u64 = 150;

fn write_text(out: &mut impl Write, s: &GameState) -> std::io::Result<()> {
    let p = &s.player;
    writeln!(out, "=== #{} ===", s.seq)?;
    writeln!(out, "Position: ({}, {})  valid={}", p.x, p.y, s.caps.position)?;
    writeln!(out, "Level: {:?}  HP: {:?}/{:?}  MP: {:?}/{:?}", p.level, p.hp, p.max_hp, p.mp, p.max_mp)?;
    writeln!(out, "Mobs: {}  (caps: {:?})", s.mobs.len(), s.caps)?;
    writeln!(out)
}

fn find_maplestory_pid() -> u32 {
    let output = std::process::Command::new("pgrep")
        .args(["-f", "MapleStory.exe"])
        .output()
        .expect("Failed to run pgrep");
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Ok(pid) = line.trim().parse::<u32>() {
            // pgrep -f also matches shells whose command line merely mentions the name.
            let ok = std::fs::read(format!("/proc/{}/cmdline", pid))
                .map(|c| {
                    let c = String::from_utf8_lossy(&c).replace('\0', " ");
                    c.trim_start().starts_with("C:\\") && c.contains("MapleStory.exe")
                })
                .unwrap_or(false);
            if ok {
                return pid;
            }
        }
    }
    eprintln!("MapleStory not found");
    std::process::exit(1);
}

#[allow(clippy::too_many_arguments)]
fn scan_all(
    mem: &dyn Mem,
    chains: &Chains,
    rt: &mut Runtime,
    fh: &mut FootholdState,
    regions: &dyn Fn() -> Vec<(u64, u64)>,
    foothold_budget: u64,
    tracker: Option<&mut rects::RectTracker>,
    netctx: Option<&mut NetCtx>,
    scan_budget: u64,
    seq: u64,
) -> GameState {
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    if rt.use_stack && rt.map_dir.is_some() && rt.map.current() != rt.ground_map {
        rt.ground_map = rt.map.current();
        let g = rt.ground_map.zip(rt.map_dir.as_deref()).and_then(|(m, d)| stackpos::Ground::load(d, m));
        let need = g.is_none() && rt.ground_map.is_some() && rt.map_dir.as_deref().map_or(false, |d| d.is_dir());
        rt.stack.set_ground(g, need);
    }
    if rt.use_stack && !rt.stack.has_region() {
        rt.stack.set_region(stackpos::pick_region(&regions()));
    }
    let hint = rt.last_pos.map(|(p, _)| p);
    let voted = if rt.use_stack { rt.stack.read(mem, ts_ms, hint) } else { None };
    // The fixed chain reads a call frame that goes stale (it returned a wandering UI value in one map), so it only
    // serves when there is no map geometry to validate the stack vote against.
    // The i32 read (stack vote or fixed chain), then the client's own f32 position when it agrees with it: the f32
    // is a frame fresher, the i32 is the check that it is the player at all.
    let i32_pos = voted.or_else(|| if rt.use_stack && rt.stack.validates() { None } else { chains.position_primary(mem) });
    // Where the i32 read is dead (it reads (0, 0) inside the Lith Harbor Department Store) the verified f32 chain
    // stands alone; the brain still pauses if a position sits off the map's floors.
    // The alternate i32 source is not used: it sat at (8, 0) in a headless client and was taken whenever the primary
    // flickered. With nothing readable the last position is held for a moment.
    let pos = match chains.position_f32(mem, i32_pos).or(i32_pos).or_else(|| chains.position_f32_unchecked(mem)) {
        Some(p) => {
            rt.last_pos = Some((p, ts_ms));
            Some(p)
        }
        None => rt.last_pos.filter(|(_, t)| ts_ms.saturating_sub(*t) <= POS_HOLD_MS).map(|(p, _)| p),
    };
    // The memory word flickers (a fifth of reads are junk) and took 10-27 s to settle on a new map; the set-field packet
    // names the map the moment we enter it. Memory only until the first one arrives.
    let mem_map = rt.map.feed(chains.flag_stable(mem, "map_id"), ts_ms);
    let mut map_id = mem_map;

    // Passive network source: authoritative mob/player lists with ids and names.
    let mut net_view: Option<(Vec<Mob>, Vec<OtherPlayer>, Vec<FieldDrop>)> = None;
    let mut party: Vec<String> = vec![];
    let mut net_stats = net::NetStats::default();
    let mut items_gained = Default::default();
    let mut lie_detector = false;
    let mut connection_stalled = false;
    if let Some(n) = netctx {
        n.pump(ts_ms, mem_map);
        map_id = n.tracker.map.or(mem_map);
        // Quest changes from the server join the remembered ones (kept per character, saved with --seen-file).
        let mut changed = false;
        for (id, q) in &n.tracker.quests {
            if rt.seen.quests.get(id) != Some(q) {
                rt.seen.quests.insert(*id, q.clone());
                changed = true;
            }
        }
        if changed {
            if let Some(path) = &rt.seen_path {
                rt.seen.save(path);
            }
        }
        net_stats = n.tracker.stats.clone();
        // Inventory: saved with the same pacing as the mesos below.
        let inv: Vec<(u8, u16, Option<u32>, u32)> = n.tracker.inventory.iter().map(|(&(t, sl), &(it, q))| (t, sl, it, q)).collect();
        if inv != rt.seen.inventory {
            rt.seen.inventory = inv;
            if ts_ms.saturating_sub(rt.meso_saved_ms) >= 10_000 {
                rt.meso_saved_ms = ts_ms;
                if let Some(path) = &rt.seen_path {
                    rt.seen.save(path);
                }
            }
        }
        // Mesos change with every pickup: saved at most every 10 s (and at once the first time they are known), so a
        // restart knows them; shop steps with a meso reserve buy nothing while they are unknown.
        let meso = net_stats.meso.or(rt.seen.meso);
        if meso != rt.seen.meso {
            let first = rt.seen.meso.is_none();
            rt.seen.meso = meso;
            if first || ts_ms.saturating_sub(rt.meso_saved_ms) >= 10_000 {
                rt.meso_saved_ms = ts_ms;
                if let Some(path) = &rt.seen_path {
                    rt.seen.save(path);
                }
            }
        }
        let job = net_stats.job.and_then(|j| u16::try_from(j).ok());
        if job.is_some() && job != rt.seen.job {
            rt.seen.job = job;
            if let Some(path) = &rt.seen_path {
                rt.seen.save(path);
            }
        }
        items_gained = n.tracker.pickups.clone();
        lie_detector = n.tracker.lie_detector_active(ts_ms);
        party = n.tracker.party.iter().map(|p| p.name.clone()).collect();
        connection_stalled = n.stall.update(n.packets.load(std::sync::atomic::Ordering::Relaxed), ts_ms);
        if n.ready() {
            net_view = Some((n.mobs(ts_ms), n.others(), n.drops()));
        }
    }

    // The level comes from the network (stat updates carry it on every level-up), else the last level seen there for
    // this character (kept in the --seen-file). The memory "level" word reads 7 for every character.
    let level = net_stats.level.map(|l| l as u16).or((rt.seen.level > 0).then_some(rt.seen.level));
    // HP/MP from the server's stat updates (exact, sent on every change: hits, potions, regeneration, level-ups);
    // memory only until the first one arrives after the stream starts.
    let hp = net_stats.hp.or_else(|| chains.stat(mem, "hp"));
    let mp = net_stats.mp.or_else(|| chains.stat(mem, "mp"));
    let mut max_hp = chains.stat(mem, "max_hp").or(net_stats.max_hp);
    let mut max_mp = chains.stat(mem, "max_mp").or(net_stats.max_mp);
    let mut estimated = false;
    // Only trust the remembered max for the level we are actually at.
    if rt.seen.observe(level, hp, mp) {
        if let Some(path) = &rt.seen_path {
            rt.seen.save(path);
        }
    }
    if hp.is_some() && max_hp.is_none() && rt.seen.max_hp > 0 {
        max_hp = Some(rt.seen.max_hp);
        estimated = true;
    }
    if mp.is_some() && max_mp.is_none() && rt.seen.max_mp > 0 {
        max_mp = Some(rt.seen.max_mp);
        estimated = true;
    }
    // Reject torn reads: current must not exceed a real max.
    let hp_ok = matches!((hp, max_hp), (Some(c), Some(m)) if m > 0 && c <= m);
    let mp_ok = matches!((mp, max_mp), (Some(c), Some(m)) if m > 0 && c <= m);

    let grounded = chains.flag01(mem, "grounded");
    // Map geometry: restart the sweep on a map change, then walk it a bit each tick.
    if fh.enabled {
        if let Some(m) = map_id {
            if fh.map_id != Some(m) {
                fh.map_id = Some(m);
                fh.list.clear();
                fh.send_once = false;
                match fh.scanner.as_mut() {
                    Some(sc) => sc.reset(regions(), Some(m)),
                    None => {
                        let mut sc = foothold::FootholdScanner::new(regions());
                        sc.map_id = Some(m);
                        fh.scanner = Some(sc);
                    }
                }
                tracing::info!("map {} -> scanning footholds", m);
            }
        }
        if let Some(sc) = fh.scanner.as_mut() {
            if let Some(list) = sc.tick(mem, foothold_budget) {
                let ok = pos.map_or(true, |(x, y)| neon_state::on_foothold(&list, x, y, 8.0));
                tracing::info!("footholds: {} segments (player on one: {})", list.len(), ok);
                fh.list = list;
                fh.send_once = true;
            }
        }
    }

    let (x, y) = pos.unwrap_or((0, 0));
    // We control the mobs when the network has them but sends no moves: their live positions come from the rects. A
    // stream started inside a map has no network mobs at all (the server lists them on entry): then the rects are the
    // mobs, without ids, on a map that has spawns.
    if let (Some(map), Some(_)) = (map_id, &tracker) {
        if rt.floors.as_ref().map(|f| f.0) != Some(map) {
            let (floors, spawns) = rt.map_dir.as_deref().map(|d| map_floors(d, map)).unwrap_or_default();
            rt.floors = Some((map, floors, spawns));
        }
    }
    let stale = net_view.as_ref().map_or(false, |(m, _, _)| {
        m.iter().any(|m| m.age_ms.map_or(false, |a| a > fuse::FRESH_MS)) || (m.is_empty() && rt.floors.as_ref().map_or(false, |f| f.2))
    });
    let tracker = match (tracker, pos, &mut net_view) {
        (Some(t), Some(p), Some((net_mobs, others, _))) if stale => {
            let rs = t.tick(mem, scan_budget);
            let floors = rt.floors.as_ref().map(|f| &f.1[..]).unwrap_or_default();
            // No floors loaded: no way to tell creatures from UI rects, so keep the network's (stale) view.
            if t.sweeps_completed >= 1 && !floors.is_empty() {
                let feet: Vec<(i32, i32)> = rects::to_mobs(&rs, p, floors)
                    .into_iter()
                    .filter(|f| !others.iter().any(|o| (o.x - f.0).abs() <= 20 && (o.y - f.1).abs() <= 20))
                    .collect();
                *net_mobs = if net_mobs.is_empty() {
                    feet.into_iter().map(|(x, y)| Mob { x, y, id: None, hp_percent: None, oid: None, age_ms: Some(0) }).collect()
                } else {
                    rt.fuser.fuse(std::mem::take(net_mobs), &feet, ts_ms)
                };
            }
            None
        }
        (t, ..) => t,
    };
    let (mobs, mobs_ready): (Vec<Mob>, bool) = match (tracker.filter(|_| net_view.is_none()), pos) {
        (Some(t), Some(p)) => {
            let rs = t.tick(mem, scan_budget);
            let m = rects::to_mobs(&rs, p, &fh.list)
                .into_iter()
                .map(|(x, y)| Mob { x, y, id: None, hp_percent: None, oid: None, age_ms: None })
                .collect();
            (m, t.sweeps_completed >= 1)
        }
        _ => (vec![], false),
    };
    // The network view wins over the hitbox scan when it is live.
    let (mobs, mobs_ready, others, drops, net_ready) = match net_view {
        Some((m, o, d)) => (m, true, o, d, true),
        None => (mobs, mobs_ready, vec![], vec![], false),
    };

    GameState {
        alerts: neon_state::Alerts { lie_detector, connection_stalled },
        seq,
        ts_ms,
        caps: Capabilities {
            position: pos.is_some(),
            map_id: map_id.is_some(),
            level: level.is_some(),
            hp_mp: hp_ok && mp_ok,
            mobs: mobs_ready,
            players: net_ready,
            drops: net_ready,
            exp: chains.stat(mem, "exp").is_some() || net_stats.exp.is_some(),
            ap: chains.stat(mem, "ap").is_some() || net_stats.ap.is_some(),
            max_estimated: estimated,
            grounded: grounded.is_some(),
        },
        player: Player {
            x,
            y,
            map_id,
            level,
            hp: hp.filter(|_| hp_ok),
            max_hp: max_hp.filter(|_| hp_ok),
            mp: mp.filter(|_| mp_ok),
            max_mp: max_mp.filter(|_| mp_ok),
            exp: chains.stat(mem, "exp").map(u64::from).or(net_stats.exp.map(u64::from)),
            exp_needed: level.and_then(neon_state::exp_to_next),
            available_ap: chains.stat(mem, "ap").or(net_stats.ap),
            meso: net_stats.meso.or(rt.seen.meso),
            job: net_stats.job.and_then(|j| u16::try_from(j).ok()).or(rt.seen.job),
            sp: net_stats.sp,
            grounded,
            dead: dead_state(&mut rt.zero_hp_since, hp, hp_ok, ts_ms),
            ..Default::default()
        },
        mobs,
        others,
        party,
        drops,
        footholds: if std::mem::take(&mut fh.send_once) { fh.list.clone() } else { vec![] },
        // The camera is the view's top-left corner, so the player must be inside the 1366x768 view it implies. Headless
        // clients read (0, 0) there, which put NPC clicks off-screen; without a sane camera the loader estimates one.
        items_gained,
        inventory: rt.seen.inventory.iter().map(|&(tab, slot, item, qty)| neon_state::InvSlot { tab, slot, item, qty }).collect(),
        quests: rt.seen.quests.iter().map(|(id, (status, progress))| neon_state::QuestRecord { id: *id, status: *status, progress: progress.clone() }).collect(),
        camera: chains.camera(mem).filter(|&(cx, cy)| pos.map_or(false, |(px, py)| (0..1366).contains(&(px - cx)) && (0..768).contains(&(py - cy)))),
    }
}

/// Floor segments of a map from its data file (neon-calib/maps/<id>.json), and whether mobs spawn there; empty and
/// false when there is no file.
fn map_floors(dir: &std::path::Path, map: u32) -> (Vec<neon_state::Foothold>, bool) {
    #[derive(serde::Deserialize)]
    struct File {
        footholds: Vec<neon_state::Foothold>,
        #[serde(default)]
        mob_spawns: Vec<serde_json::Value>,
    }
    std::fs::read_to_string(dir.join(format!("{map}.json")))
        .ok()
        .and_then(|t| serde_json::from_str::<File>(&t).ok())
        .map(|f| (f.footholds.into_iter().filter(|f| f.x1 != f.x2).collect(), !f.mob_spawns.is_empty()))
        .unwrap_or_default()
}

// ------------------------------------------------------------ network source

enum NetMsg {
    Events(Vec<net::NetEvent>),
    NewFlow,
}

struct NetCtx {
    /// Framed server packets so far (written by the reader thread).
    packets: std::sync::Arc<std::sync::atomic::AtomicU64>,
    stall: StallDetector,
    tracker: net::NetTracker,
    rx: std::sync::mpsc::Receiver<NetMsg>,
    child: Option<std::process::Child>,
    last_map: Option<u32>,
    seen_spawn: bool,
    warned_exit: bool,
}

impl NetCtx {
    fn pump(&mut self, now_ms: u64, map: Option<u32>) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                // A new TCP connection means a new channel or server hop.
                NetMsg::NewFlow => self.tracker.clear(),
                NetMsg::Events(evs) => {
                    for e in evs {
                        if matches!(e, net::NetEvent::MobSpawn { .. }) {
                            self.seen_spawn = true;
                        }
                        self.tracker.apply(&e, now_ms);
                    }
                }
            }
        }
        if let (Some(m), prev) = (map, self.last_map) {
            if prev != Some(m) {
                if prev.is_some() {
                    self.tracker.retain_recent(now_ms, 3_000);
                }
                self.last_map = Some(m);
            }
        }
        self.tracker.expire_players(now_ms, 15_000);
        self.tracker.expire_drops(now_ms, 90_000);
        if let Some(c) = self.child.as_mut() {
            if !self.warned_exit && matches!(c.try_wait(), Ok(Some(_))) {
                tracing::error!("tcpdump exited: is passwordless `sudo -n tcpdump` available? network data stopped");
                self.warned_exit = true;
            }
        }
    }

    /// Only report once we have actually seen the world (a mob spawn or plenty of traffic).
    fn ready(&self) -> bool {
        self.seen_spawn || self.tracker.events > 50
    }

    fn mobs(&self, now_ms: u64) -> Vec<Mob> {
        self.tracker
            .mobs
            .iter()
            .map(|(oid, m)| {
                let (x, y) = m.predicted(now_ms);
                Mob { x, y, id: (m.template != 0).then_some(m.template), hp_percent: None, oid: Some(*oid), age_ms: Some(now_ms.saturating_sub(m.last_ms).min(u32::MAX as u64) as u32) }
            })
            .collect()
    }

    fn drops(&self) -> Vec<FieldDrop> {
        // Only our own loot: drops owned by our character id (learnt from drop owners; none until it is known).
        self.tracker.own_drops().into_iter().map(|d| FieldDrop { x: d.x, y: d.y, is_meso: None }).collect()
    }

    fn others(&self) -> Vec<OtherPlayer> {
        self.tracker
            .players
            .values()
            .filter_map(|p| p.pos.map(|(x, y)| OtherPlayer { name: p.name.clone(), x, y }))
            .collect()
    }
}

impl Drop for NetCtx {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
        }
    }
}

/// Feed a pcap byte stream through the decoder, forwarding events.
/// Extras for the network reader thread.
#[derive(Default, Clone)]
struct NetOpts {
    packets: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    /// Keep only this game process's connection (several clients on one machine share the capture).
    own_pid: Option<u32>,
    alert_opcodes: std::collections::HashMap<u16, net::AlertKind>,
    opcode_log: Option<std::path::PathBuf>,
}

fn parse_hex_u16(s: &str) -> Result<u16, String> {
    let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u16::from_str_radix(t, 16).map_err(|e| format!("not a hex opcode: {e}"))
}

fn pump_pcap<R: std::io::Read>(r: R, tx: std::sync::mpsc::Sender<NetMsg>, opts: NetOpts) -> (u64, u64) {
    let mut reader = match net::PcapReader::new(r) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("network capture: {}", e);
            return (0, 0);
        }
    };
    let mut dec = net::NetDecoder::default();
    dec.alert_opcodes = opts.alert_opcodes.clone();
    // NEON_DUMP_OPS=007e,0471 prints those opcodes; "all" prints every packet.
    dec.dump_ops = std::env::var("NEON_DUMP_OPS").unwrap_or_default().split(',').filter_map(|s| if s == "all" { Some(0xffff) } else { parse_hex_u16(s).ok() }).collect();
    let mut last_dump = std::time::Instant::now();
    let (mut own_ports, mut ports_read) = (Vec::new(), None::<std::time::Instant>);
    while let Ok(Some(pkt)) = reader.next_packet() {
        if let Some(pid) = opts.own_pid {
            // The client reconnects on a channel change, so look the ports up again every second.
            if ports_read.map_or(true, |t| t.elapsed() >= std::time::Duration::from_secs(1)) {
                ports_read = Some(std::time::Instant::now());
                own_ports = net::game_ports_of(pid);
            }
        }
        if let Some(path) = &opts.opcode_log {
            if last_dump.elapsed() >= std::time::Duration::from_secs(5) {
                last_dump = std::time::Instant::now();
                let mut v: Vec<_> = dec.ops.iter().collect();
                v.sort_by_key(|(k, _)| **k);
                let obj: serde_json::Map<String, serde_json::Value> = v.into_iter().map(|(k, st)| (format!("0x{k:04x}"), serde_json::to_value(st).unwrap_or_default())).collect();
                let _ = std::fs::write(path, serde_json::to_string_pretty(&obj).unwrap_or_default());
            }
        }
        let Some(seg) = net::parse_tcp(reader.linktype, &pkt) else { continue };
        // A port we do not know may be our client's new connection (login, channel change): look again at once (at
        // most every 50 ms) rather than lose its first packets, which carry the whole character at login.
        // our client's local port: the destination of inbound segments, the source of outbound ones
        let local = if net::NetDecoder::is_game_outbound(&seg) { seg.sport } else { seg.dport };
        if let Some(pid) = opts.own_pid {
            if !own_ports.contains(&local) && ports_read.map_or(true, |t| t.elapsed() >= std::time::Duration::from_millis(50)) {
                ports_read = Some(std::time::Instant::now());
                own_ports = net::game_ports_of(pid);
            }
        }
        // Unknown ports (unreadable /proc) keep everything, as with a single client.
        if !own_ports.is_empty() && !own_ports.contains(&local) {
            continue;
        }
        dec.cur_ts_us = reader.last_ts_us;
        let evs = dec.push(&seg);
        if let Some(p) = &opts.packets {
            p.store(dec.packets, std::sync::atomic::Ordering::Relaxed);
        }
        if std::mem::take(&mut dec.new_flow) && tx.send(NetMsg::NewFlow).is_err() {
            return (dec.packets, dec.resyncs());
        }
        if !evs.is_empty() && tx.send(NetMsg::Events(evs)).is_err() {
            return (dec.packets, dec.resyncs());
        }
    }
    (dec.packets, dec.resyncs())
}

fn start_net_live(iface: &str, opts: NetOpts) -> NetCtx {
    use std::process::{Command, Stdio};
    let (tx, rx) = std::sync::mpsc::channel();
    let mut child = Command::new("sudo")
        .args(["-n", "tcpdump", "-i", iface, "-nn", "-s", "0", "-U", "-w", "-"])
        .arg(format!("tcp and portrange {}-{}", net::PORT_MIN, net::PORT_MAX))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| {
            eprintln!("cannot start tcpdump via sudo: {}", e);
            std::process::exit(1);
        });
    let out = child.stdout.take().expect("piped stdout");
    let packets = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let opts = NetOpts { packets: Some(packets.clone()), ..opts };
    std::thread::spawn(move || {
        pump_pcap(out, tx, opts);
    });
    tracing::info!("network source: sudo tcpdump on {} (ports {}-{})", iface, net::PORT_MIN, net::PORT_MAX);
    NetCtx { packets, stall: StallDetector::new(15_000), tracker: net::NetTracker::default(), rx, child: Some(child), last_map: None, seen_spawn: false, warned_exit: false }
}

/// Replay a capture and print what the tracker ends up with (validation aid).
fn replay_pcap(path: &std::path::Path) {
    let f = std::fs::File::open(path).unwrap_or_else(|e| {
        eprintln!("cannot open {:?}: {}", path, e);
        std::process::exit(1);
    });
    let (tx, rx) = std::sync::mpsc::channel();
    let (packets, resyncs) = pump_pcap(std::io::BufReader::new(f), tx, NetOpts::default());
    let mut counts: std::collections::BTreeMap<&str, u64> = Default::default();
    let mut tracker = net::NetTracker::default();
    let (mut events, mut flows) = (0u64, 0u32);
    for m in rx.try_iter() {
        match m {
            NetMsg::NewFlow => {
                flows += 1;
                tracker.clear();
            }
            NetMsg::Events(evs) => {
                for e in evs {
                    events += 1;
                    *counts.entry(match e {
                        net::NetEvent::MobSpawn { .. } => "mob_spawn",
                        net::NetEvent::MobMove { .. } => "mob_move",
                        net::NetEvent::MobKill { .. } => "mob_kill",
                        net::NetEvent::PlayerSpawn { .. } => "player_spawn",
                        net::NetEvent::PlayerMove { .. } => "player_move",
                        net::NetEvent::DropSpawn { .. } => "drop_spawn",
                        net::NetEvent::DropRemove { .. } => "drop_remove",
                        net::NetEvent::MapEnter { .. } => "map_enter",
                        net::NetEvent::Quest { .. } => "quest",
                        net::NetEvent::Stats(_) => "stats",
                        net::NetEvent::Pickup { .. } => "pickup",
                        net::NetEvent::Inventory { .. } => "inventory",
                        net::NetEvent::InventoryAll(..) => "inventory_all",
                        net::NetEvent::Alert { .. } => "alert",
                        net::NetEvent::Party { .. } => "party",
                        net::NetEvent::PartyLeft { .. } => "party_left",
                    }).or_default() += 1;
                    tracker.apply(&e, events);
                }
            }
        }
    }
    let mut by: std::collections::BTreeMap<u32, u32> = Default::default();
    for m in tracker.mobs.values() {
        *by.entry(m.template).or_default() += 1;
    }
    let names: Vec<_> = tracker.players.values().filter_map(|p| p.name.clone()).take(12).collect();
    println!(
        "{}",
        serde_json::json!({
            "packets_framed": packets, "resyncs": resyncs, "event_counts": counts, "events": events, "connections": flows, "mobs_alive": tracker.mobs.len(),
            "mobs_by_template": by, "players": tracker.players.len(), "drops_tracked": tracker.drops.len(), "final_stats": format!("{:?}", tracker.stats), "sample_player_names": names,
        })
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seen_max_remembers_per_level_and_only_grows() {
        let mut s = SeenMax::default();
        assert!(s.observe(Some(7), Some(146), Some(77)));
        assert!(!s.observe(Some(7), Some(60), Some(30)), "lower readings change nothing");
        assert_eq!((s.level, s.max_hp, s.max_mp), (7, 146, 77));
        // starting low next run still gets the right max from the file
        let again = s.clone();
        assert_eq!(again.max_hp, 146);
        // a level-up resets: the old max no longer applies
        assert!(s.observe(Some(8), Some(60), Some(30)));
        assert_eq!((s.level, s.max_hp, s.max_mp), (8, 60, 30));
    }

    #[test]
    fn seen_max_roundtrips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("neon-seen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("seen-max.json");
        let s = SeenMax { level: 7, max_hp: 146, max_mp: 77, ..Default::default() };
        s.save(&path);
        assert_eq!(SeenMax::load(&path), Some(s));
        assert_eq!(SeenMax::load(&dir.join("missing.json")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn debounce_ignores_flicker_and_loading_garbage() {
        let mut d = Debounce::default();
        assert_eq!(d.feed(Some(10001000), 0), None, "first sighting is not enough");
        assert_eq!(d.feed(Some(10001000), 20), Some(10001000));
        // live flicker values seen during testing
        for (i, g) in [3276855, 37, 1019224408, 4294322787, 506334408].into_iter().enumerate() {
            assert_eq!(d.feed(Some(g), 100 + i as u64 * 20), Some(10001000), "garbage {g} must not change the map");
        }
        assert_eq!(d.feed(None, 300), Some(10001000));
    }

    #[test]
    fn debounce_accepts_a_real_map_change_only_after_it_persists() {
        let mut d = Debounce::default();
        d.feed(Some(10001000), 0);
        d.feed(Some(10001000), 20);
        // At 50 fps, five frames is only 80 ms: that must NOT be enough.
        for i in 0..6 {
            assert_eq!(d.feed(Some(10001020), 1000 + i * 20), Some(10001000), "too soon at frame {i}");
        }
        // Persisting for 600 ms flips it.
        assert_eq!(d.feed(Some(10001020), 1000 + 620), Some(10001020));
    }

    #[test]
    fn garbage_that_lasts_a_few_frames_does_not_change_the_map() {
        // This is the live failure: 506334408 held for ~100 ms while the area was loading.
        let mut d = Debounce::default();
        d.feed(Some(10001030), 0);
        d.feed(Some(10001030), 20);
        for i in 0..8 {
            assert_eq!(d.feed(Some(506334408), 500 + i * 20), Some(10001030));
        }
        d.feed(Some(10001030), 700);
        assert_eq!(d.feed(Some(10001030), 720), Some(10001030));
    }

    #[test]
    fn interrupted_candidate_resets() {
        let mut d = Debounce::default();
        d.feed(Some(10001000), 0);
        d.feed(Some(10001000), 20);
        for i in 0..3 {
            d.feed(Some(10001020), 100 + i * 100);
        }
        d.feed(None, 450); // flicker breaks the run
        for i in 0..4 {
            assert_eq!(d.feed(Some(10001020), 500 + i * 200), Some(10001000));
        }
    }

    #[test]
    fn death_needs_zero_hp_to_persist_not_one_bad_read() {
        let mut since = None;
        assert_eq!(dead_state(&mut since, Some(0), true, 1000), Some(false), "first zero read");
        assert_eq!(dead_state(&mut since, Some(37), true, 1020), Some(false), "a garbage zero then a real value");
        assert_eq!(dead_state(&mut since, Some(0), true, 2000), Some(false));
        assert_eq!(dead_state(&mut since, None, false, 2100), None, "unreadable frames neither kill nor revive");
        assert_eq!(dead_state(&mut since, Some(0), true, 2000 + DEAD_AFTER_MS), Some(true));
        assert_eq!(dead_state(&mut since, Some(50), true, 9000), Some(false), "revived");
    }

    #[test]
    fn stall_detector_fires_after_silence_and_clears_on_traffic() {
        let mut d = StallDetector::new(15_000);
        assert!(!d.update(100, 0), "first reading never alarms");
        assert!(!d.update(100, 14_999));
        assert!(d.update(100, 15_000), "15 s of the same count");
        assert!(d.update(100, 40_000), "stays raised while silent");
        assert!(!d.update(101, 40_020), "one packet clears it");
        assert!(!d.update(101, 50_000));
    }
}
