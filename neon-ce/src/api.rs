//! Loopback JSON-RPC over HTTP, bearer-token protected.
//!
//! Deliberately tiny: one request per connection, `Content-Length` bodies, no keep-alive
//! and no TLS - only 127.0.0.1 ever talks to it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::region::Scope;
use crate::state::Engine;

pub struct Server {
    pub port: u16,
    pub token: String,
}

fn random_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut x = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (std::process::id() as u64) << 32;
    let mut out = String::new();
    for _ in 0..4 {
        // xorshift, plenty for a local capability token
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push_str(&format!("{x:016x}"));
    }
    out
}

pub fn serve(engine: Arc<Engine>, port: u16) -> anyhow::Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    let token = random_token();
    let srv = Server { port, token: token.clone() };
    let me = engine.clone();
    let tok = token.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut stream) = conn else { continue };
            let me = me.clone();
            let tok = tok.clone();
            std::thread::spawn(move || {
                let _ = handle(&mut stream, &me, &tok);
            });
        }
    });
    Ok(srv)
}

fn handle(stream: &mut std::net::TcpStream, engine: &Engine, token: &str) -> anyhow::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut len = 0usize;
    let mut auth = String::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        let low = h.to_ascii_lowercase();
        if let Some(v) = low.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
        if low.starts_with("authorization:") {
            auth = h.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
        }
    }
    if path == "/health" {
        let body = json!({"ok": true, "pid": engine.pid()}).to_string();
        return respond(stream, 200, &body);
    }
    if !auth.starts_with("Bearer ") || auth[7..] != *token {
        return respond(stream, 401, &json!({"error": "bad token"}).to_string());
    }
    let mut body = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut body)?;
    }
    let req: Value = serde_json::from_slice(&body)?;
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let name = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let result = dispatch(engine, name, &params);
    let out = match result {
        Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}),
        Err(e) => json!({"jsonrpc": "2.0", "id": id,
                         "error": {"code": -32000, "message": e.to_string()}}),
    };
    respond(stream, 200, &out.to_string())
}

fn respond(stream: &mut std::net::TcpStream, code: u16, body: &str) -> anyhow::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn num(p: &Value, key: &str) -> Option<u64> {
    // Numbers are decimal (that is how results are emitted), strings are hex.
    p.get(key)
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| parse_hex(s))))
}

fn parse_hex(s: &str) -> Option<u64> {
    let t = s.trim();
    let hex = t.strip_prefix("0x").unwrap_or(t);
    u64::from_str_radix(hex, 16).ok()
}

fn text<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(|v| v.as_str())
}

fn dispatch(engine: &Engine, name: &str, p: &Value) -> anyhow::Result<Value> {
    let addr = || -> anyhow::Result<u64> {
        num(p, "addr").ok_or_else(|| anyhow::anyhow!("missing/invalid addr"))
    };
    Ok(match name {
        "status" => engine.status(),
        "modules" => engine.modules(),
        "region" => engine.region_of(addr()?),
        "regions_debug" => {
            let scope = Scope::parse(text(p, "scope").unwrap_or("all"))?;
            engine.regions_debug(&scope)
        }
        "regions" => {
            let scope = Scope::parse(text(p, "scope").unwrap_or("all"))?;
            engine.regions(&scope, num(p, "limit").unwrap_or(200) as usize)
        }
        "read" => engine.read(addr()?, text(p, "type").unwrap_or("4 bytes"))?,
        "write" => engine.write(
            addr()?,
            text(p, "type").unwrap_or("4 bytes"),
            text(p, "value").ok_or_else(|| anyhow::anyhow!("missing value"))?,
        )?,
        "scan_create" => engine.create_session(
            text(p, "type").ok_or_else(|| anyhow::anyhow!("missing type"))?,
            text(p, "scope").unwrap_or("heap"),
            num(p, "alignment").unwrap_or(4) as usize,
            text(p, "cmp").unwrap_or("eq"),
            text(p, "value"),
            text(p, "value2"),
        )?,
        "hunt_start" => engine.hunt_start(
            num(p, "hp_addr").ok_or_else(|| anyhow::anyhow!("missing hp_addr"))?,
            text(p, "type").unwrap_or("4 bytes"),
            text(p, "scope").ok_or_else(|| anyhow::anyhow!("missing scope"))?,
            num(p, "settle_ms").unwrap_or(250),
            num(p, "fall_ms").unwrap_or(400),
        )?,
        "hunt_state" => engine.hunt_state(),
        "hunt_stop" => engine.hunt_stop(),
        "scan_reseed" => engine.session_reseed(
            text(p, "session").ok_or_else(|| anyhow::anyhow!("missing session"))?,
            text(p, "cmp").unwrap_or("changed"),
            text(p, "value"),
            text(p, "value2"),
        )?,
        "scan_next" => engine.next_scan(
            text(p, "session").ok_or_else(|| anyhow::anyhow!("missing session"))?,
            text(p, "cmp").unwrap_or("eq"),
            text(p, "value"),
        )?,
        "scan_results" => engine.results(
            text(p, "session").ok_or_else(|| anyhow::anyhow!("missing session"))?,
            num(p, "offset").unwrap_or(0) as usize,
            num(p, "limit").unwrap_or(100) as usize,
        )?,
        "scan_reset" => engine.session_reset(text(p, "session").unwrap_or(""))?,
        "scan_cancel" => engine.session_cancel(text(p, "session").unwrap_or(""))?,
        "scan_undo" => engine.session_undo(text(p, "session").unwrap_or(""))?,
        "scan_drop" => engine.session_drop(text(p, "session").unwrap_or(""))?,
        "freeze_add" => engine.freeze_add(
            addr()?,
            text(p, "type").unwrap_or("4 bytes"),
            text(p, "value").ok_or_else(|| anyhow::anyhow!("missing value"))?,
            num(p, "interval_ms").unwrap_or(100),
        )?,
        "freeze_list" => engine.freeze_list(),
        "freeze_update" => engine.freeze_update(
            num(p, "id").ok_or_else(|| anyhow::anyhow!("missing id"))?,
            text(p, "value"),
            p.get("active").and_then(|v| v.as_bool()),
        )?,
        "freeze_remove" => engine.freeze_remove(num(p, "id").ok_or_else(|| anyhow::anyhow!("missing id"))?)?,
        "freeze_clear" => engine.freeze_clear(),
        "ct_load" => engine.ct_load(
            text(p, "path").ok_or_else(|| anyhow::anyhow!("missing path"))?,
            p.get("apply").and_then(|v| v.as_bool()).unwrap_or(false),
        )?,
        "ct_save" => {
            let path = text(p, "path").ok_or_else(|| anyhow::anyhow!("missing path"))?;
            let table = crate::ct::load(std::path::Path::new(
                text(p, "from").ok_or_else(|| anyhow::anyhow!("missing from"))?,
            ))?;
            crate::ct::save(&table, std::path::Path::new(path))?;
            json!({"saved": path})
        }
        other => return Err(anyhow::anyhow!("unknown method '{other}'")),
    })
}
