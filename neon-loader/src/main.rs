//! Neon Loader - consumes the neon-stream state feed and drives the game.
//!
//!   neon-stream | neon-loader --stdin
//!   neon-loader --stream-cmd ./neon-stream/target/release/neon-stream

mod bot;
mod brain;
mod char_build;
mod control;
mod input;
mod nav;
mod party;
mod notify;
mod plan;
mod rules;
mod quest;
mod shop;
mod sim;
mod stats;
mod ui;
mod xkeys;
mod status;

use clap::Parser;
use neon_state::GameState;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;

#[derive(Parser, Debug)]
#[command(name = "neon-loader")]
#[command(about = "Neon MapleStory bot: drives the game from the neon-stream state feed")]
struct Args {
    /// Read JSONL game states from stdin
    #[arg(long, conflicts_with = "stream_cmd")]
    stdin: bool,

    /// Spawn this neon-stream binary and read its stdout
    #[arg(long)]
    stream_cmd: Option<String>,

    /// Extra args passed to the spawned stream (e.g. --config path)
    #[arg(long = "stream-arg")]
    stream_args: Vec<String>,

    /// Decide and log, but don't press any keys
    #[arg(long)]
    dry_run: bool,

    /// Enable debug logging
    #[arg(long)]
    debug: bool,

    #[arg(long)]
    no_kami: bool,
    #[arg(long)]
    no_autopot: bool,
    #[arg(long)]
    no_autoap: bool,
    /// Deprecated no-op: evasion is off unless --evasion is given.
    #[arg(long)]
    no_evasion: bool,
    /// Pause when a stranger is nearby (off by default).
    #[arg(long)]
    evasion: bool,
    #[arg(long)]
    no_loot: bool,
    /// Stop the run after this many deaths (0 = never)
    #[arg(long, default_value_t = 0)]
    max_deaths: u32,
    /// Click "revive in town" after a death (capped per hour)
    #[arg(long)]
    auto_revive: bool,
    /// Pickup key (xdotool name)
    #[arg(long)]
    loot_key: Option<String>,
    /// Stop fighting below this HP percent (default 30)
    #[arg(long)]
    flee_below: Option<f64>,
    /// Resume fighting above this HP percent (default 60)
    #[arg(long)]
    resume_above: Option<f64>,

    /// Only fight these mob ids, comma separated (e.g. 2,3,4,5,6,7). Default: anything.
    #[arg(long, value_delimiter = ',')]
    mobs: Vec<u32>,
    /// Replay a recorded --trace-file through the shadow simulation: TRACE MAP_ID (writes --sim-log)
    #[arg(long, num_args = 2, value_names = ["TRACE", "MAP"])]
    sim_replay: Option<Vec<String>>,
    /// Re-sync the shadow to the real character this often during --sim-replay (ms)
    #[arg(long)]
    sim_resync_ms: Option<u64>,
    /// Check the navigator's jump model against real jumps in a --trace-file: MAP TRACE... (prints the share of
    /// jumps whose predicted landing platform is the real one)
    #[arg(long, num_args = 2.., value_names = ["MAP", "TRACE"])]
    nav_check: Option<Vec<String>>,
    /// Put the HP/MP potions given as inventory cells (--hp-key/--mp-key inv:...) on quick slots at start and press
    /// those keys instead of opening the inventory: HP on Ins, MP on Home. --no-hotkeys keeps the inventory clicks.
    #[arg(long)]
    no_hotkeys: bool,
    /// Town to rest in when out of potions (e.g. 10000000 Lith Harbor)
    #[arg(long)]
    safe_map: Option<u32>,
    /// Fit the movement constants to recorded traces: MAP TRACE... (prints the best constants)
    #[arg(long, num_args = 2.., value_names = ["MAP", "TRACE"])]
    sim_fit: Option<Vec<String>>,
    /// Log the shadow movement simulation (always running; shown in the panel) one line per frame here (tools/simfit.py)
    #[arg(long)]
    sim_log: Option<std::path::PathBuf>,
    /// Write a per-tick decision trace here (debugging)
    #[arg(long)]
    trace_file: Option<std::path::PathBuf>,
    /// Only operate in these map ids, comma separated (e.g. 10001030 = Forest West of Henesys)
    #[arg(long, value_delimiter = ',')]
    only_map: Vec<u32>,
    /// File holding a Discord webhook URL (or set NEON_DISCORD_WEBHOOK)
    #[arg(long)]
    discord_webhook_file: Option<std::path::PathBuf>,
    /// Alerts to send, comma separated: level,death,lowhp,stranger,lie,stop,lag,disconnect
    #[arg(long, value_delimiter = ',', default_values_t = ["level".to_string(), "death".to_string(), "lie".to_string(), "stop".to_string(), "stranger".to_string(), "disconnect".to_string()])]
    discord_events: Vec<String>,
    /// Global hotkey that stops the bot at once (X key name, or "none")
    #[arg(long, default_value = "Pause")]
    panic_key: String,
    /// Directory shared with the other bots on this machine: each writes the mob it is after so no two fight the same one
    #[arg(long)]
    party_dir: Option<std::path::PathBuf>,
    /// Directory for per-session history logs (one JSON line every 30 s)
    #[arg(long, default_value = "history")]
    history_dir: std::path::PathBuf,
    /// JSON list of rules ("when X do Y") to run; also editable live from the control panel
    #[arg(long)]
    rules: Option<std::path::PathBuf>,
    /// JSON plan/schedule to run (steps: travel, grind, rest; optional daily windows)
    #[arg(long)]
    plan: Option<std::path::PathBuf>,
    /// Map to grind in; anywhere else the bot walks back to it through portals
    #[arg(long)]
    home_map: Option<u32>,
    /// Directory of per-map geometry JSON (from neon-calib/tools/wzmap.py)
    #[arg(long, default_value = "neon-calib/maps")]
    maps_dir: std::path::PathBuf,
    /// Quest definitions from the client data (neon-calib/tools/wzquests.py)
    #[arg(long, default_value = "neon-calib/quests.json")]
    quests: std::path::PathBuf,
    /// Write a live JSON status snapshot here for the web UI
    #[arg(long)]
    status_file: Option<std::path::PathBuf>,
    /// Session name: several bots can run side by side, each with its own status/control files
    #[arg(long, default_value = "main")]
    session: String,
    /// Control file watched for pause/stop/settings from the web UI
    #[arg(long)]
    control_file: Option<std::path::PathBuf>,
    /// Stop the bot (keys released) once this level is reached
    #[arg(long)]
    stop_at_level: Option<u16>,

    /// HP% below which to use an HP potion
    #[arg(long)]
    hp_below: Option<f64>,
    /// Key (or `inv:<tab>:<row>:<col>` to double-click an inventory cell) that drinks an HP potion
    #[arg(long)]
    hp_key: Option<String>,
    /// Use one inventory cell once (`inv:<tab>:<row>:<col>`) and exit: a way to check a potion position
    #[arg(long)]
    use_item: Option<String>,
    /// Same for MP potions
    #[arg(long)]
    mp_key: Option<String>,
    /// MP% below which to use an MP potion
    #[arg(long)]
    mp_below: Option<f64>,
    /// Attack key (xdotool name)
    #[arg(long)]
    attack_key: Option<String>,
    /// Jump key (xdotool name)
    #[arg(long)]
    jump_key: Option<String>,
    /// One-shot: print the platform route the nav plans: MAP FROM_X FROM_Y [TO_X TO_Y] (no target = the highest reachable platform)
    #[arg(long, num_args = 3..=5, allow_negative_numbers = true)]
    nav_route: Option<Vec<i32>>,
    /// Do not spend ability / skill points automatically (STR first, per the warrior guide) at start and after every level-up
    #[arg(long)]
    no_stats: bool,
    /// One-shot: spend the unspent ability and skill points now and exit
    #[arg(long)]
    assign_stats: bool,
    /// Character build (builds/warrior.json): per-level AP and SP, and skills to put on quick slots and use in fights.
    /// Without one every AP goes to STR and Beginner SP to Three Snails.
    #[arg(long)]
    build: Option<std::path::PathBuf>,
    /// With --assign-stats: the character's level and job (the build needs them; the running bot gets them from the stream)
    #[arg(long, requires = "assign_stats")]
    level: Option<u16>,
    #[arg(long, requires = "assign_stats")]
    job: Option<u16>,
    /// One-shot: answer the NPC dialog that is open right now (NEXT / ACCEPT / OK, never DECLINE) and exit
    #[arg(long)]
    quest_dialog: bool,
    /// Trusted player names (never trigger evasion)
    #[arg(long = "trust")]
    trusted: Vec<String>,
}

fn main() {
    let args = Args::parse();
    if let Some(v) = &args.nav_check {
        let map = v[0].parse::<u32>().unwrap_or_else(|_| die("MAP must be a map id"));
        let geom = nav::MapGeom::load(&args.maps_dir, map).unwrap_or_else(|| die("no geometry for that map"));
        let n = nav::Nav::new(&geom, nav::Tuning::default());
        let (mut ok, mut total, mut marginal) = (0, 0, 0);
        for t in &v[1..] {
            for j in nav::check_jumps(&n, &std::fs::read_to_string(t).unwrap_or_default()) {
                total += 1;
                marginal += j.marginal as u32;
                if j.matched {
                    ok += 1;
                } else {
                    println!("MISS {}", j.describe());
                }
            }
        }
        println!("landing platform match: {ok}/{total} = {:.1}% ({marginal} take-offs marginal: more than one possible landing)", if total > 0 { 100.0 * ok as f64 / total as f64 } else { 0.0 });
        return;
    }
    if let Some(v) = &args.sim_fit {
        let map = v[0].parse::<u32>().unwrap_or_else(|_| die("MAP must be a map id"));
        let jump = brain::Keymap::default().jump;
        match sim::fit(&v[1..], args.maps_dir.clone(), map, &jump, 1000) {
            Ok((p, before, after)) => {
                println!("score (avg error + 2 x landing error) {before:.2} -> {after:.2} px\n{p:#?}");
                for t in &v[1..] {
                    let text = std::fs::read_to_string(t).unwrap_or_default();
                    let mut sh = sim::Shadow::new(args.maps_dir.clone(), None);
                    sh.params = p;
                    sh.resync_ms = Some(1000);
                    if let Ok(sh) = sim::replay_text(&text, sh, map, &jump) {
                        println!("{t}: {}", sh.summary());
                    }
                }
            }
            Err(e) => die(&e),
        }
        return;
    }
    if let Some(v) = &args.sim_replay {
        let map = v[1].parse::<u32>().unwrap_or_else(|_| die("MAP must be a map id"));
        let out = args.sim_log.clone().unwrap_or_else(|| std::path::PathBuf::from("sim.log"));
        let jump = brain::Keymap::default().jump;
        let text = std::fs::read_to_string(&v[0]).unwrap_or_else(|e| die(&format!("{}: {e}", v[0])));
        let mut sh = sim::Shadow::new(args.maps_dir.clone(), Some(out.clone()));
        sh.resync_ms = args.sim_resync_ms;
        match sim::replay_text(&text, sh, map, &jump) {
            Ok(sh) => println!("{} -> {}", sh.summary(), out.display()),
            Err(e) => die(&e),
        }
        return;
    }
    if let Some(v) = &args.nav_route {
        let geom = nav::MapGeom::load(&args.maps_dir, v[0] as u32).unwrap_or_else(|| die("no geometry for that map"));
        let n = nav::Nav::new(&geom, nav::Tuning::default());
        let Some(from) = n.locate_within(v[1], v[2], 40) else { die("the start is not on a platform") };
        let goal = if v.len() >= 5 { n.locate_within(v[3], v[4], 40) } else { n.highest_reachable(from, v[1]) };
        let Some(goal) = goal else { die("no goal platform") };
        let (a, b, c, d) = n.segment(goal);
        println!("from segment {from} {:?} to segment {goal} ({a},{b})-({c},{d})", n.segment(from));
        let r = n.route(from, v[1], goal);
        if r.is_empty() && from != goal {
            println!("NO ROUTE");
        }
        if v.len() == 3 {
            let costs = n.costs_from(from, v[1]);
            println!("platforms by height (R = reachable from the start):");
            let mut all: Vec<usize> = (0..n.segment_count()).collect();
            all.sort_by_key(|&i| n.segment(i).1.min(n.segment(i).3));
            for i in all {
                let (a, b, c, d) = n.segment(i);
                println!("  {} seg {:>3} x {:>5}..{:>5}  y {:>5}..{:>5}", if costs[i].is_some() { "R" } else { "." }, i, a, c, b, d);
            }
        }
        for (i, s) in r.iter().enumerate() {
            println!("{:>3}. {:<22} start x {:>5} (y {:>5}) window {}..{}  ->  land x {:>5} (y {:>5})   rise {:>4}   seg {} -> {}", i + 1, s.kind, s.start, s.from_y, s.lo, s.hi, s.land, s.to_y, s.from_y - s.to_y, s.from, s.to);
        }
        return;
    }
    if let Some(p) = &args.build {
        match char_build::CharBuild::load(p) {
            Ok(b) => stats::set_build(b),
            Err(e) => die(&e),
        }
    }
    if args.assign_stats {
        stats::observe(stats::Progress { level: args.level, job: args.job, ap: None, sp: None });
        match stats::assign_all() {
            Ok(s) => println!("spent {} ability point(s), {} skill point(s); put on quick slots: {:?}", s.ap, s.sp, s.slotted),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.quest_dialog {
        match shop::Win::find().and_then(|w| quest::drive_dialog(&w, false)) {
            Ok(r) => println!("{r}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(spec) = &args.use_item {
        shop::use_item_async(spec);
        std::thread::sleep(std::time::Duration::from_millis(2500));
        return;
    }
    if args.debug {
        std::env::set_var("RUST_LOG", "debug");
    }
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
    ).init();

    let mut cfg = brain::BrainConfig::default();
    cfg.kami = !args.no_kami;
    cfg.auto_pot = !args.no_autopot;
    cfg.auto_ap = !args.no_autoap;
    cfg.evasion = args.evasion && !args.no_evasion;
    cfg.loot = !args.no_loot;
    cfg.auto_revive = args.auto_revive;
    cfg.max_deaths = args.max_deaths;
    if let Some(k) = args.loot_key { cfg.keys.loot = k; }
    if let Some(v) = args.flee_below { cfg.flee_hp_below = v; }
    if let Some(v) = args.resume_above { cfg.resume_hp_above = v; }
    if let Some(v) = args.hp_below { cfg.hp_pot_below = v; }
    cfg.safe_map = args.safe_map;
    cfg.mob_by_exp = load_mob_by_exp(&args.maps_dir.join("..").join("mob-info.json"));
    // `--hp-key auto`: press Ins, and the bot puts the HP potion it finds in the server's inventory there.
    let auto_hp = args.hp_key.as_deref() == Some("auto");
    if let Some(k) = args.hp_key { cfg.keys.hp_pot = if auto_hp { "Insert".into() } else { k }; }
    if let Some(k) = args.mp_key { cfg.keys.mp_pot = k; }
    // Potions on quick slots: one key press instead of opening the inventory every time.
    let mut pot_cells = vec![];
    if !args.dry_run && !args.no_hotkeys {
        for (key, slot, hp) in [(&cfg.keys.hp_pot, "ins", true), (&cfg.keys.mp_pot, "home", false)] {
            if key.starts_with("inv:") {
                pot_cells.push((key.clone(), slot, hp));
            }
        }
        for (key, slot, what) in [(&mut cfg.keys.hp_pot, "ins", "HP"), (&mut cfg.keys.mp_pot, "home", "MP")] {
            if key.starts_with("inv:") {
                match shop::bind_to_quickslot(key, slot) {
                    Ok(k) => {
                        tracing::info!("{what} potion {key} put on the {slot} quick slot: pressing {k}");
                        *key = k;
                    }
                    Err(e) => tracing::warn!("could not put the {what} potion on a quick slot ({e}); using inventory clicks"),
                }
            }
        }
    }
    if let Some(v) = args.mp_below { cfg.mp_pot_below = v; }
    if let Some(k) = args.attack_key { cfg.keys.attack = k; }
    if let Some(k) = args.jump_key { cfg.keys.jump = k; }
    if !args.mobs.is_empty() {
        cfg.allowed_mobs = Some(args.mobs.iter().copied().collect());
    }
    if !args.only_map.is_empty() {
        cfg.allowed_maps = Some(args.only_map.iter().copied().collect());
    }
    cfg.home_map = args.home_map;
    cfg.trusted_igns = args.trusted.iter().map(|s| s.to_lowercase()).collect();

    let (tx, rx) = mpsc::channel::<GameState>();
    let mut child = None;
    let reader: Box<dyn BufRead + Send> = if let Some(cmd) = &args.stream_cmd {
        let mut c = Command::new(cmd)
            .args(&args.stream_args)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| die(&format!("failed to spawn {}: {}", cmd, e)));
        let out = c.stdout.take().expect("piped stdout");
        child = Some(c);
        Box::new(BufReader::new(out))
    } else if args.stdin {
        Box::new(BufReader::new(std::io::stdin()))
    } else {
        die("pick a state source: --stdin or --stream-cmd <path>");
    };

    std::thread::spawn(move || {
        for line in reader.lines().map_while(Result::ok) {
            match GameState::from_line(&line) {
                Ok(s) => {
                    if tx.send(s).is_err() {
                        break;
                    }
                }
                Err(e) => tracing::warn!("bad state line: {}", e),
            }
        }
    });

    quest::configure(args.quests.clone(), args.maps_dir.clone());
    let plan: Option<plan::Plan> = args.plan.as_ref().map(|p| {
        let text = std::fs::read_to_string(p).unwrap_or_else(|e| die(&format!("cannot read plan {}: {e}", p.display())));
        serde_json::from_str(&text).unwrap_or_else(|e| die(&format!("invalid plan {}: {e}", p.display())))
    });
    let webhook = args
        .discord_webhook_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .or_else(|| std::env::var("NEON_DISCORD_WEBHOOK").ok());
    let events: Vec<notify::Kind> = args.discord_events.iter().filter_map(|n| notify::Kind::parse(n)).collect();
    let notifier = notify::Notifier::new(webhook, events, &args.session);
    if notifier.active() {
        tracing::info!("Discord alerts enabled");
    }
    let panic = if args.panic_key.eq_ignore_ascii_case("none") || args.dry_run { None } else { xkeys::spawn_panic_listener(&args.panic_key) };
    if panic.is_some() {
        tracing::info!("panic key: {} stops the bot immediately", args.panic_key);
    }
    let rules: Vec<rules::Rule> = args.rules.as_ref().map(|p| {
        let text = std::fs::read_to_string(p).unwrap_or_else(|e| die(&format!("cannot read rules {}: {e}", p.display())));
        serde_json::from_str(&text).unwrap_or_else(|e| die(&format!("invalid rules {}: {e}", p.display())))
    }).unwrap_or_default();
    let brain = brain::Brain::new(cfg).with_maps(args.maps_dir.clone());
    let suffix = if args.session == "main" { String::new() } else { format!(".{}", args.session) };
    let status_path = args.status_file.clone().unwrap_or_else(|| format!("neon-status{suffix}.json").into());
    let control_path = args.control_file.clone().unwrap_or_else(|| format!("neon-control{suffix}.json").into());
    let mut status = status::StatusWriter::new(status_path, args.maps_dir.clone(), args.dry_run, args.stop_at_level);
    status.set_session(&args.session);
    let status = status.with_history(args.history_dir.clone(), std::time::Duration::from_secs(30));
    if !args.dry_run && !args.no_stats {
        stats::spawn_watcher();
    }
    let result = if args.dry_run {
        tracing::info!("DRY RUN: no keys will be pressed");
        {
            let mut bot = bot::Bot::new(brain, bot::DryRun, status).with_control(control_path.clone()).with_notifier(notifier).with_panic(panic.clone()).with_party(args.party_dir.clone(), &args.session);
            if let Some(p) = &args.trace_file { bot = bot.with_trace(p); }
            bot = bot.with_shadow(sim::Shadow::new(args.maps_dir.clone(), args.sim_log.clone()));
            if let Some(p) = &plan { bot = bot.with_plan(p.clone()); }
            if !rules.is_empty() { bot = bot.with_rules(rules.clone()); }
            bot.run(rx)
        }
    } else {
        let mut ic = input::InputController::new().unwrap_or_else(|e| die(&e));
        ic.find_window("MapleStory").unwrap_or_else(|e| die(&e));
        // An NPC conversation left open by an interrupted run hides the map id, and nothing runs without it.
        if let Ok(w) = shop::Win::find() {
            if quest::end_chat(&w) {
                tracing::info!("closed an NPC dialog left open");
            }
        }
        {
            let mut bot = bot::Bot::new(brain, ic, status).with_control(control_path.clone()).with_notifier(notifier).with_panic(panic.clone()).with_party(args.party_dir.clone(), &args.session).with_pot_cells(pot_cells.clone()).with_auto_hp(auto_hp);
            if let Some(p) = &args.trace_file { bot = bot.with_trace(p); }
            bot = bot.with_shadow(sim::Shadow::new(args.maps_dir.clone(), args.sim_log.clone()));
            if let Some(p) = &plan { bot = bot.with_plan(p.clone()); }
            bot.run(rx)
        }
    };

    if let Some(mut c) = child {
        let _ = c.kill();
    }
    if let Err(e) = result {
        die(&e);
    }
}

/// EXP per kill -> mob template from neon-calib/mob-info.json, keeping only amounts that one mob alone pays.
fn load_mob_by_exp(path: &std::path::Path) -> std::collections::HashMap<u64, u32> {
    let mut by: std::collections::HashMap<u64, Vec<u32>> = Default::default();
    let v: serde_json::Value = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if let Some(mobs) = v.get("mobs").and_then(|m| m.as_object()) {
        for (id, m) in mobs {
            if let (Ok(id), Some(exp)) = (id.parse::<u32>(), m.get("exp").and_then(|e| e.as_u64())) {
                by.entry(exp).or_default().push(id);
            }
        }
    }
    by.into_iter().filter(|(_, ids)| ids.len() == 1).map(|(e, ids)| (e, ids[0])).collect()
}

fn die(msg: &str) -> ! {
    eprintln!("Error: {}", msg);
    std::process::exit(1);
}
