//! MCP server (stdio) that forwards to the running engine.
//!
//! opencode launches this binary; it finds the engine through the discovery file and
//! relays each tool call. If the engine is not running, calls fail with a clear message
//! instead of the client hanging.

use std::io::{BufRead, Read, Write};
use std::net::TcpStream;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::session::Session;

const PROTOCOL: &str = "2025-06-18";

struct Engine {
    port: u16,
    token: String,
}

impl Engine {
    fn find() -> Result<Engine> {
        let s = Session::load().ok_or_else(|| {
            anyhow!("neon-ce engine is not running (start `neon-ce serve` from the GUI)")
        })?;
        Ok(Engine {
            port: s.port,
            token: s.token,
        })
    }

    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port))
            .map_err(|_| anyhow!("engine on port {} is not reachable", self.port))?;
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
            .to_string();
        let head = format!(
            "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.token,
            body.len()
        );
        stream.write_all(head.as_bytes())?;
        stream.write_all(body.as_bytes())?;
        let mut raw = String::new();
        stream.read_to_string(&mut raw)?;
        let body = raw.split("\r\n\r\n").nth(1).ok_or_else(|| anyhow!("empty reply"))?;
        let v: Value = serde_json::from_str(body)?;
        if let Some(err) = v.get("error") {
            return Err(anyhow!(err["message"].as_str().unwrap_or("engine error").to_string()));
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

fn addr() -> Value {
    json!({"type": "string", "description": "hex address, e.g. 21A3F0C0"})
}

fn vtype() -> Value {
    json!({"type": "string",
           "enum": ["byte", "2 bytes", "4 bytes", "8 bytes", "float", "double", "string", "utf16", "aob"],
           "description": "value type"})
}

fn tools() -> Vec<Value> {
    vec![
        json!({"name": "ce_status", "description": "Attached process, module count, open sessions and active freezes.",
               "inputSchema": schema(json!({}), &[])}),
        json!({"name": "ce_modules", "description": "Loaded modules with base/size (for static addresses).",
               "inputSchema": schema(json!({}), &[])}),
        json!({"name": "ce_region", "description": "Region containing an address: base/size/protection.",
               "inputSchema": schema(json!({"addr": addr()}), &["addr"])}),
        json!({"name": "ce_regions", "description": "Scan regions for a scope.",
               "inputSchema": schema(json!({
                   "scope": {"type": "string", "enum": ["all", "private", "mapped", "image", "heap", "module:NAME"]},
                   "limit": {"type": "integer"}}), &[])}),
        json!({"name": "ce_read", "description": "Read a typed value.",
               "inputSchema": schema(json!({"addr": addr(), "type": vtype()}), &["addr"])}),
        json!({"name": "ce_write", "description": "Write a typed value once (logged for restore).",
               "inputSchema": schema(json!({"addr": addr(), "type": vtype(), "value": {"type": "string"}}),
                                     &["addr", "value"])}),
        json!({"name": "ce_scan_create", "description": "Start a scan. Comparisons: eq, ne, lt, gt, lte, gte, between, increased, decreased, changed, unchanged, sameasfirst. Unknown-value scans use changed/increased/decreased without `value`.",
               "inputSchema": schema(json!({
                   "type": vtype(),
                   "scope": {"type": "string", "description": "all|private|mapped|image|heap|module:NAME (default heap)"},
                   "alignment": {"type": "integer", "description": "1/2/4/8 (default 4)"},
                   "cmp": {"type": "string"},
                   "value": {"type": "string"},
                   "value2": {"type": "string"}}), &["type", "cmp"])}),
        json!({"name": "ce_hunt_start", "description": "Watches an HP address and, on every drop, keeps only values in `scope` that rise then fall (damage/i-frame shaped). Poll with ce_hunt_state.",
               "inputSchema": schema(json!({"hp_addr": addr(), "type": vtype(), "scope": {"type": "string"},
                                            "settle_ms": {"type": "integer"}, "fall_ms": {"type": "integer"}}),
                                     &["hp_addr", "scope"])}),
        json!({"name": "ce_hunt_state", "description": "Hunt progress and the most frequent survivors.", "inputSchema": schema(json!({}), &[])}),
        json!({"name": "ce_hunt_stop", "description": "Stop the hunt.", "inputSchema": schema(json!({}), &[])}),
        json!({"name": "ce_scan_reseed", "description": "Re-run the first scan in place with a new comparison (fast unknown-value hunting).",
               "inputSchema": schema(json!({"session": {"type": "string"}, "cmp": {"type": "string"},
                                            "value": {"type": "string"}, "value2": {"type": "string"}}),
                                     &["session", "cmp"])}),
        json!({"name": "ce_scan_next", "description": "Narrow an existing session with a new comparison.",
               "inputSchema": schema(json!({"session": {"type": "string"}, "cmp": {"type": "string"},
                                            "value": {"type": "string"}}), &["session", "cmp"])}),
        json!({"name": "ce_scan_results", "description": "Page through session results (addr + current value).",
               "inputSchema": schema(json!({"session": {"type": "string"}, "offset": {"type": "integer"},
                                            "limit": {"type": "integer"}}), &["session"])}),
        json!({"name": "ce_scan_reset", "description": "Clear a session.", "inputSchema": schema(json!({"session": {"type": "string"}}), &["session"])}),
        json!({"name": "ce_scan_cancel", "description": "Stop a running scan.", "inputSchema": schema(json!({"session": {"type": "string"}}), &["session"])}),
        json!({"name": "ce_scan_undo", "description": "Undo the last narrowing step.", "inputSchema": schema(json!({"session": {"type": "string"}}), &["session"])}),
        json!({"name": "ce_scan_drop", "description": "Forget a session.", "inputSchema": schema(json!({"session": {"type": "string"}}), &["session"])}),
        json!({"name": "ce_freeze_add", "description": "Freeze an address at a value (rewritten every interval_ms, default 100).",
               "inputSchema": schema(json!({"addr": addr(), "type": vtype(), "value": {"type": "string"},
                                            "interval_ms": {"type": "integer"}}), &["addr", "value"])}),
        json!({"name": "ce_freeze_list", "description": "List freezes with write/error counters.", "inputSchema": schema(json!({}), &[])}),
        json!({"name": "ce_freeze_update", "description": "Change a freeze value or enable/disable it.",
               "inputSchema": schema(json!({"id": {"type": "integer"}, "value": {"type": "string"},
                                            "active": {"type": "boolean"}}), &["id"])}),
        json!({"name": "ce_freeze_remove", "description": "Remove one freeze.", "inputSchema": schema(json!({"id": {"type": "integer"}}), &["id"])}),
        json!({"name": "ce_freeze_clear", "description": "Remove all freezes.", "inputSchema": schema(json!({}), &[])}),
        json!({"name": "ce_ct_load", "description": "Load a cheat table; with apply=true active entries are written and frozen.",
               "inputSchema": schema(json!({"path": {"type": "string"}, "apply": {"type": "boolean"}}), &["path"])}),
        json!({"name": "ce_ct_save", "description": "Copy a cheat table to a new path.",
               "inputSchema": schema(json!({"from": {"type": "string"}, "path": {"type": "string"}}), &["from", "path"])}),
    ]
}

fn forward(name: &str, args: &Value) -> Result<Value> {
    let engine = Engine::find()?;
    let method = name.strip_prefix("ce_").unwrap_or(name);
    engine.call(method, args.clone())
}

pub fn run() -> Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let reply = match method {
            "initialize" => Some(json!({"protocolVersion": PROTOCOL,
                                        "capabilities": {"tools": {"listChanged": false}},
                                        "serverInfo": {"name": "neon-ce", "version": env!("CARGO_PKG_VERSION")}})),
            "notifications/initialized" | "notifications/cancelled" => None,
            "ping" => Some(json!({})),
            "tools/list" => Some(json!({"tools": tools()})),
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                let args = req["params"].get("arguments").cloned().unwrap_or(json!({}));
                Some(match forward(name, &args) {
                    Ok(v) => json!({"content": [{"type": "text", "text": v.to_string()}]}),
                    Err(e) => json!({"content": [{"type": "text", "text": format!("error: {e}")}],
                                     "isError": true}),
                })
            }
            other => Some(json!({"error": {"code": -32601, "message": format!("unknown method {other}")}})),
        };
        if let (Some(id), Some(result)) = (id, reply) {
            let msg = if result.get("error").is_some() {
                json!({"jsonrpc": "2.0", "id": id, "error": result["error"]})
            } else {
                json!({"jsonrpc": "2.0", "id": id, "result": result})
            };
            writeln!(out, "{msg}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// One-shot helper used by the CLI (`neon-ce call ...`).
pub fn call_once(method: &str, params: Value) -> Result<Value> {
    Engine::find()?.call(method, params)
}

#[allow(dead_code)]
fn _keep(_: &mut dyn Read) {}
