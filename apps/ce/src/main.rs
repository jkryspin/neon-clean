//! neon-ce - the Neon workbench.
//!
//! `neon-ce` (or `neon-ce gui`) opens the window. `serve` runs the read-only API
//! without a UI. `mcp` is the stdio MCP bridge and `call` a one-shot client.

mod api;
mod ct;
mod freeze;
mod gui;
mod live;
mod mcp;
mod proc;
mod region;
mod scan;
mod session;
mod state;
mod value;
mod web;
mod workbench;

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{json, Value};

const DEFAULT_PORT: u16 = 8790;

fn usage() -> ! {
    eprintln!(
        "usage:
  neon-ce [gui] [--pid N] [--port 8790] [--host 127.0.0.1]
  neon-ce serve [--pid N] [--port 8790]
  neon-ce mcp
  neon-ce status
  neon-ce ps
  neon-ce call <method> [json-params|-]"
    );
    std::process::exit(2)
}

fn arg_after(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn pid_arg(args: &[String]) -> Option<u32> {
    arg_after(args, "--pid").and_then(|p| p.parse().ok())
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
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"pid": proc::find_game_pid()}))?
            );
            Ok(())
        }
        "call" => {
            let method = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("missing method"))?;
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
        "" | "gui" => gui_main(&args),
        s if s.starts_with('-') => gui_main(&args),
        _ => usage(),
    }
}

fn gui_main(args: &[String]) -> Result<()> {
    if !proc::is_elevated() {
        eprintln!("warning: not elevated - OpenProcess/WriteProcessMemory will likely fail");
    }
    let port = arg_after(args, "--port")
        .map(|p| p.parse::<u16>())
        .transpose()
        .unwrap_or(Some(DEFAULT_PORT))
        .unwrap_or(DEFAULT_PORT);
    let host = arg_after(args, "--host").unwrap_or_else(|| "127.0.0.1".into());
    let api = web::State::new();
    web::serve(api.clone(), &host, port).context("bind API")?;
    let pid = pid_arg(args);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "neon-ce",
        options,
        Box::new(move |_cc| Ok(Box::new(workbench::Workbench::new(pid, api)))),
    )
    .map_err(|e| anyhow::anyhow!("window: {e}"))
}

fn serve(args: &[String]) -> Result<()> {
    if !proc::is_elevated() {
        eprintln!("warning: not elevated - OpenProcess will likely fail");
    }
    let pid = match pid_arg(args) {
        Some(p) => p,
        None => proc::find_game_pid().context("MapleStory.exe is not running (or pass --pid)")?,
    };
    let port = arg_after(args, "--port")
        .map(|p| p.parse::<u16>())
        .transpose()
        .context("--port must be a number")?
        .unwrap_or(DEFAULT_PORT);
    let api = web::State::new();
    web::serve(api.clone(), "127.0.0.1", port).context("bind API")?;
    let reader = live::Reader::start(pid, workbench::data_dir()).map_err(|e| anyhow::anyhow!(e))?;
    eprintln!("serving pid {pid} on http://127.0.0.1:{port}");
    loop {
        *api.live.lock().unwrap() = Some(reader.snapshot());
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

#[allow(dead_code)]
fn _unused() -> Arc<()> {
    Arc::new(())
}

#[allow(dead_code)]
fn _bail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(2)
}
