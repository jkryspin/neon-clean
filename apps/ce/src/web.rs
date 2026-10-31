//! Read-only HTTP/SSE surface for neon-web.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::live::Live;

pub struct State {
    pub live: Mutex<Option<Live>>,
    pub logs: Mutex<Vec<String>>,
}

impl State {
    pub fn new() -> Arc<State> {
        Arc::new(State {
            live: Mutex::new(None),
            logs: Mutex::new(Vec::new()),
        })
    }
}

pub fn serve(state: Arc<State>, host: &str, port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind((host, port))?;
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let state = state.clone();
            thread::spawn(move || {
                let _ = handle(conn, state);
            });
        }
    });
    Ok(())
}

fn handle(mut stream: TcpStream, state: Arc<State>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" {
            break;
        }
    }

    if path == "/api/events" {
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: keep-alive\r\n\r\n")?;
        loop {
            let body = state
                .live
                .lock()
                .unwrap()
                .as_ref()
                .map(|l| serde_json::to_string(l).unwrap_or_else(|_| "null".into()))
                .unwrap_or_else(|| "null".into());
            if stream
                .write_all(format!("data: {body}\n\n").as_bytes())
                .is_err()
            {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(500));
        }
        return Ok(());
    }

    let (code, body) = match path.as_str() {
        "/api/health" => (200, r#"{"ok":true}"#.to_string()),
        "/api/live" | "/api/state" => {
            let body = state
                .live
                .lock()
                .unwrap()
                .as_ref()
                .map(|l| serde_json::to_string(l).unwrap_or_else(|_| "null".into()))
                .unwrap_or_else(|| "null".into());
            (200, body)
        }
        "/api/logs" => {
            let logs = state.logs.lock().unwrap();
            (
                200,
                serde_json::to_string(&*logs).unwrap_or_else(|_| "[]".into()),
            )
        }
        "/" | "/index.html" => (404, r#"{"error":"run neon-web"}"#.to_string()),
        _ => (404, r#"{"error":"not found"}"#.to_string()),
    };
    stream.write_all(
        format!(
            "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
}
