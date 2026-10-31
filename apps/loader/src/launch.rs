//! B1 launch chain: clean stock client, real anti-cheat, stealth inject, verify.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use neon_win::process::{self, Kind};

use crate::config;

pub const STEPS: &[&str] = &[
    "Elevation",
    "Clean client",
    "Real anti-cheat",
    "Start launcher",
    "Trigger PLAY",
    "Client started",
    "Inject neon-hack",
    "Injection verified",
    "Anti-cheat state",
    "Open workbench",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Pending,
    Running,
    Ok,
    Fail,
    Skip,
}

#[derive(Clone, Debug)]
pub enum Event {
    Step(usize, State, String),
    Log(String),
    Done(bool),
}

pub struct Options {
    pub auto_open_ce: bool,
    pub kill_existing: bool,
}

pub fn start(tx: Sender<Event>, options: Options) {
    thread::spawn(move || {
        let ok = run(&tx, &options).is_ok();
        let _ = tx.send(Event::Done(ok));
    });
}

fn step(tx: &Sender<Event>, idx: usize, state: State, detail: impl Into<String>) {
    let _ = tx.send(Event::Step(idx, state, detail.into()));
}

fn log(tx: &Sender<Event>, text: impl Into<String>) {
    let _ = tx.send(Event::Log(text.into()));
}

fn ok(tx: &Sender<Event>, idx: usize, detail: impl Into<String>) -> Result<()> {
    step(tx, idx, State::Ok, detail);
    Ok(())
}

fn fail(tx: &Sender<Event>, idx: usize, detail: impl Into<String>) -> Result<()> {
    step(tx, idx, State::Fail, detail);
    Ok(())
}

fn run(tx: &Sender<Event>, options: &Options) -> Result<()> {
    let app = PathBuf::from(config::APP_DIR);
    let exe = app.join("MapleStory.exe");

    step(tx, 0, State::Running, "checking token");
    if !process::is_elevated() {
        let msg = "not elevated: run as administrator";
        fail(tx, 0, msg)?;
        bail!(msg);
    }
    ok(tx, 0, "elevated")?;

    step(tx, 1, State::Running, "restoring a clean client");
    if process::is_running("MapleStory.exe") {
        if !options.kill_existing {
            let msg = "MapleStory.exe is already running; close it first";
            fail(tx, 1, msg)?;
            bail!(msg);
        }
        let _ = process::kill("MapleStory.exe");
        wait_until(
            || !process::is_running("MapleStory.exe"),
            Duration::from_secs(10),
        )?;
    }
    for name in [
        "nexon_launcher.exe",
        "nexon_client.exe",
        "nexon_runtime.exe",
    ] {
        let _ = process::kill(name);
    }
    thread::sleep(Duration::from_secs(2));
    let py = config::python();
    let patch = config::patch_script();
    let out = Command::new(&py)
        .args([
            patch.to_string_lossy().as_ref(),
            "--restore",
            exe.to_string_lossy().as_ref(),
        ])
        .output()
        .context("run patch.py --restore")?;
    log(tx, String::from_utf8_lossy(&out.stdout));
    let hash = neon_win::hash::sha256(&exe)?;
    if hash != config::CLEAN_HASH {
        let msg = format!("client is not the known clean build ({hash})");
        fail(tx, 1, &msg)?;
        bail!(msg);
    }
    ok(tx, 1, "known clean build")?;

    step(tx, 2, State::Running, "restoring the real anti-cheat DLLs");
    for name in ["grap64.dll", "nmcogame64.dll"] {
        let dst = app.join(name);
        let orig = app.join(format!("{name}.orig"));
        if orig.exists() {
            if let Err(e) = std::fs::copy(&orig, &dst) {
                let msg = format!("restore {name}: {e}");
                fail(tx, 2, &msg)?;
                bail!(msg);
            }
        }
        let restored = match (std::fs::read(&dst), std::fs::read(&orig)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        if !restored {
            let msg = format!("{name} is still mocked (no .orig)");
            fail(tx, 2, &msg)?;
            bail!(msg);
        }
        log(tx, format!("real {name} in place"));
    }
    let _ = Command::new("sc").args(["start", "BlackCat1"]).output();
    let _ = Command::new("sc").args(["start", "NGS"]).output();
    ok(tx, 2, "real DLLs and driver requested")?;

    step(tx, 3, State::Running, "starting the launcher");
    Command::new(config::LAUNCHER)
        .spawn()
        .context("start nexon_launcher.exe")?;
    wait_until(
        || process::is_running("nexon_client.exe"),
        Duration::from_secs(90),
    )?;
    thread::sleep(Duration::from_secs(20));
    ok(tx, 3, "launcher ready")?;

    step(tx, 4, State::Running, "clicking PLAY");
    open_url(config::PLAY_URL)?;
    ok(tx, 4, "PLAY sent")?;

    step(tx, 5, State::Running, "waiting for the client");
    let mut started = false;
    for attempt in 1..=3 {
        if attempt > 1 {
            log(
                tx,
                format!("client not up yet (attempt {attempt}): relaunching"),
            );
            let _ = process::kill("nexon_client.exe");
            let _ = Command::new(config::LAUNCHER).spawn();
            let _ = wait_until(
                || process::is_running("nexon_client.exe"),
                Duration::from_secs(60),
            );
            thread::sleep(Duration::from_secs(20));
            let _ = open_url(config::PLAY_URL);
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            if process::is_running("MapleStory.exe") {
                started = true;
                break;
            }
            thread::sleep(Duration::from_secs(1));
        }
        if started {
            break;
        }
    }
    if !started {
        let msg = "client did not start";
        fail(tx, 5, msg)?;
        bail!(msg);
    }
    let pid = process::find_pid("MapleStory.exe").context("client pid vanished")?;
    ok(tx, 5, format!("client pid {pid}"))?;

    step(tx, 6, State::Running, "injecting neon-hack");
    let hack = config::payload("hack/neon-hack.dll");
    let injector = config::payload("probe/inject-stealth.exe");
    let out = Command::new(&injector)
        .args([&pid.to_string(), hack.to_string_lossy().as_ref()])
        .output()
        .context("run inject-stealth.exe")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    log(tx, &text);
    if !out.status.success() {
        let msg = format!("injector failed: {}", text.trim());
        fail(tx, 6, &msg)?;
        bail!(msg);
    }
    let base = text
        .lines()
        .find_map(|l| l.split(" at ").nth(1))
        .and_then(|v| {
            u64::from_str_radix(v.split_whitespace().next()?.trim_start_matches("0x"), 16).ok()
        });
    let Some(base) = base else {
        let msg = "injector did not report a base";
        fail(tx, 6, msg)?;
        bail!(msg);
    };
    ok(tx, 6, format!("payload at {base:#x}"))?;

    step(tx, 7, State::Running, "verifying the mapping");
    let proc_handle = match process::Process::open(pid) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("OpenProcess: {e:#}");
            fail(tx, 7, &msg)?;
            bail!(msg);
        }
    };
    let header_pages = proc_handle.regions(base, base + 0x1000, true);
    let text_pages = proc_handle.regions(base + 0x1000, base + 0x2000, true);
    let Some(header_page) = header_pages.first() else {
        let msg = "payload base is not mapped";
        fail(tx, 7, msg)?;
        bail!(msg);
    };
    let unlinked = proc_handle.module_by_name("neon-hack.dll").is_none();
    let header = proc_handle.read(base, 2).unwrap_or_default();
    let wiped = header == [0, 0];
    let private = header_page.kind == Kind::Private;
    let header_ro = header_page.protect == windows::Win32::System::Memory::PAGE_READONLY.0;
    let rx = text_pages
        .first()
        .map(|p| p.protect == windows::Win32::System::Memory::PAGE_EXECUTE_READ.0)
        .unwrap_or(false);
    let detail = format!(
        "private={private} unlinked={unlinked} headers_wiped={wiped} header_ro={header_ro} text_rx={rx}"
    );
    if !(private && unlinked && wiped && header_ro && rx) {
        fail(tx, 7, &detail)?;
        bail!("injection verification failed: {detail}");
    }
    ok(tx, 7, detail)?;

    step(tx, 8, State::Running, "reading anti-cheat state");
    let loaded: Vec<String> = proc_handle.modules.iter().map(|m| m.name.clone()).collect();
    let ac = neon_win::system::ac_state(&app, &loaded);
    log(tx, serde_json::to_string(&ac).unwrap_or_default());
    if ac.grap_loaded && ac.nmco_loaded && !ac.grap_mocked && !ac.nmco_mocked {
        ok(tx, 8, "real GRAP and NGS loaded")?;
    } else {
        step(
            tx,
            8,
            State::Skip,
            "real anti-cheat DLLs not both confirmed",
        );
    }

    step(tx, 9, State::Running, "opening neon-ce");
    if options.auto_open_ce {
        let ce = config::ce_exe();
        if ce.exists() {
            Command::new(&ce)
                .args(["--pid", &pid.to_string()])
                .spawn()
                .context("start neon-ce")?;
            ok(tx, 9, ce.display().to_string())?;
        } else {
            step(tx, 9, State::Skip, format!("{} not built", ce.display()));
        }
    } else {
        step(tx, 9, State::Skip, "auto-open disabled");
    }
    log(tx, format!("ready: pid {pid}, payload base {base:#x}"));
    Ok(())
}

fn open_url(url: &str) -> Result<()> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &format!("Start-Process '{url}'")])
        .output()
        .context("Start-Process url")?;
    if !out.status.success() {
        bail!(
            "Start-Process url failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

fn wait_until(mut f: impl FnMut() -> bool, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if f() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    bail!("timed out after {:?}", timeout)
}
