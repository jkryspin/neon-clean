//! neon-ce - typed memory workbench for MapleStory Classic World.
//!
//! `neon-ce serve` starts the engine (loopback JSON-RPC + discovery file).
//! `neon-ce mcp` is the stdio MCP bridge opencode launches.
//! `neon-ce call <method> [json]` is a one-shot client for scripts and smoke tests.

mod api;
mod ct;
mod freeze;
mod gui;
mod mcp;
mod proc;
mod region;
mod scan;
mod session;
mod state;
mod value;

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

fn usage() -> ! {
    eprintln!(
        "usage:
  neon-ce serve [--pid N] [--port 8790]
  neon-ce mcp
  neon-ce status
  neon-ce ps
  neon-ce (no args) | neon-ce gui
  neon-ce call <method> [json-params|-]"
    );
    std::process::exit(2)
}

fn game_pid() -> Option<u32> {
    proc::find_game_pid()
}

#[allow(dead_code)]
fn game_pid_old() -> Option<u32> {
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry).is_err() {
            let _ = CloseHandle(snap);
            return None;
        }
        loop {
            let name = String::from_utf16_lossy(
                &entry.szExeFile[..entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(0)],
            );
            if name.eq_ignore_ascii_case("MapleStory.exe") {
                let _ = CloseHandle(snap);
                return Some(entry.th32ProcessID);
            }
            if Process32NextW(snap, &mut entry).is_err() {
                break;
            }
        }
        let _ = CloseHandle(snap);
        let _ = (GetModuleBaseNameW, MAX_PATH, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION);
        None
    }
}

fn arg_after(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");

    match cmd {
        "serve" => serve(&args),
        "mcp" => mcp::run(),
        "status" => {
            let v = mcp::call_once("status", json!({}))?;
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        "ps" => {
            println!("{}", serde_json::to_string_pretty(&json!({"pid": game_pid()}))?);
            Ok(())
        }
        "call" => {
            let method = args.get(1).ok_or_else(|| anyhow::anyhow!("missing method"))?;
            let params: Value = match args.get(2).map(String::as_str) {
                Some("-") => {
                    use std::io::Read;
                    let mut raw = String::new();
                    std::io::stdin().read_to_string(&mut raw)?;
                    serde_json::from_str(raw.trim()).context("params must be JSON")?
                }
                Some(raw) => serde_json::from_str(raw).context("params must be JSON")?,
                None => json!({}),
            };
            let v = mcp::call_once(method, params)?;
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }

        _ => usage(),
    }
}

fn serve(args: &[String]) -> Result<()> {
    if !proc::is_elevated() {
        eprintln!("warning: not elevated - OpenProcess/WriteProcessMemory will likely fail");
    }
    let pid = match arg_after(args, "--pid") {
        Some(p) => p.parse::<u32>().context("--pid must be a number")?,
        None => game_pid().context("MapleStory.exe is not running (or pass --pid)")?,
    };
    let port = arg_after(args, "--port")
        .map(|p| p.parse::<u16>())
        .transpose()
        .context("--port must be a number")?
        .unwrap_or(8790);

    let engine = state::Engine::attach(pid)?;
    if !engine.alive() {
        bail!("pid {pid} is not alive");
    }
    let srv = api::serve(engine.clone(), port)?;
    session::Session {
        pid,
        port: srv.port,
        token: srv.token.clone(),
        started_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    }
    .write()?;
    println!(
        "neon-ce listening on 127.0.0.1:{} for pid {} - session file {}",
        srv.port,
        pid,
        session::path().display()
    );

    let watch = engine.clone();
    std::thread::spawn(move || {
        while watch.alive() {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        eprintln!("target exited, stopping engine");
        session::Session::remove();
        std::process::exit(0);
    });

    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

#[allow(dead_code)]
fn _arc(_: Arc<()>) {}
