//! Engine: owns the process handle, scan sessions, freeze list and write log.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::ct;
use crate::freeze::{self, Entry};
use crate::proc::Process;
use crate::region::Scope;
use crate::scan::{Memory, Session};
use crate::value::{Cmp, VType};

pub const MAX_WINDOW: usize = 5000;
pub const MAX_RESULTS_TOTAL: usize = 40_000_000;

#[derive(Clone)]
pub struct Engine {
    proc: Arc<Mutex<Process>>,
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<Session>>>>>,
    freezes: Arc<Mutex<Vec<Entry>>>,
    ids: Arc<AtomicU64>,
    log: Arc<Mutex<Option<File>>>,
    hunt: Arc<Mutex<Hunt>>,
    pub started: Instant,
}

#[derive(Default)]
pub struct Hunt {
    pub running: bool,
    pub hits: u64,
    pub rounds: u64,
    pub last_hp: u32,
    pub survivors: usize,
    pub session: String,
    pub counts: HashMap<u64, u32>,
    pub error: Option<String>,
}

fn run_dir() -> PathBuf {
    let mut p = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    p.push("run");
    let _ = std::fs::create_dir_all(&p);
    p
}

fn stamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Engine {
    pub fn attach(pid: u32) -> Result<Arc<Engine>> {
        let proc = Process::open(pid)?;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(run_dir().join("ce-writes.jsonl"))
            .ok();
        let e = Arc::new(Engine {
            proc: Arc::new(Mutex::new(proc)),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            freezes: Arc::new(Mutex::new(Vec::new())),
            ids: Arc::new(AtomicU64::new(1)),
            log: Arc::new(Mutex::new(log)),
            hunt: Arc::new(Mutex::new(Hunt::default())),
            started: Instant::now(),
        });
        e.spawn_freezer();
        Ok(e)
    }

    pub fn pid(&self) -> u32 {
        self.proc.lock().unwrap().pid
    }

    pub fn alive(&self) -> bool {
        self.proc.lock().unwrap().alive()
    }

    fn spawn_freezer(self: &Arc<Self>) {
        let me = self.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(20));
            let due: Vec<Entry> = {
                let mut list = me.freezes.lock().unwrap();
                freeze::due(&mut list, Instant::now())
            };
            if due.is_empty() {
                continue;
            }
            let pid = me.pid();
            let mut proc = me.proc.lock().unwrap();
            for e in due {
                let Some(bytes) = e.bytes() else { continue };
                let res = proc.write(e.addr, &bytes);
                {
                    let mut list = me.freezes.lock().unwrap();
                    if let Some(item) = list.iter_mut().find(|x| x.id == e.id) {
                        match &res {
                            Ok(()) => item.writes += 1,
                            Err(_) => item.errors += 1,
                        }
                    }
                }
                if res.is_ok() {
                    me.log_write(pid, e.addr, &bytes);
                }
            }
        });
    }

    fn log_write(&self, pid: u32, addr: u64, bytes: &[u8]) {
        if let Some(f) = self.log.lock().unwrap().as_mut() {
            let _ = writeln!(
                f,
                "{{\"ts\":{},\"pid\":{},\"addr\":{},\"hex\":\"{}\"}}",
                stamp(),
                pid,
                addr,
                bytes.iter().map(|b| format!("{b:02X}")).collect::<String>()
            );
        }
    }

    pub fn status(&self) -> Value {
        let proc = self.proc.lock().unwrap();
        let sessions = self.sessions.lock().unwrap();
        json!({
            "pid": proc.pid,
            "exe": proc.exe,
            "alive": proc.alive(),
            "modules": proc.modules.len(),
            "main_base": proc.main_base(),
            "sessions": sessions.len(),
            "frozen": self.freezes.lock().unwrap().iter().filter(|e| e.active).count(),
            "uptime_ms": self.started.elapsed().as_millis() as u64,
        })
    }

    pub fn regions(&self, scope: &Scope, limit: usize) -> Value {
        let proc = self.proc.lock().unwrap();
        let list: Vec<Value> = proc
            .scan_regions(scope)
            .into_iter()
            .take(limit)
            .map(|(b, s)| json!({"base": b, "size": s}))
            .collect();
        json!({"scope": scope.name(), "count": list.len(), "regions": list})
    }

    pub fn regions_debug(&self, scope: &Scope) -> Value {
        let proc = self.proc.lock().unwrap();
        let v = proc.scan_regions(scope);
        let (lo, hi) = match scope {
            Scope::Range(b, s) => (*b, b + *s),
            _ => (0x10000, 0x20000),
        };
        json!({"scope": format!("{scope:?}"), "n": v.len(),
               "raw_true": proc.regions(lo, hi, true).len(),
               "raw_false": proc.regions(lo, hi, false).len(),
               "raw_first": proc.regions(lo, hi, true).first().map(|r| (r.base, r.size, r.protect))})
    }

    /// Region containing an address' (base/size/protection) - used to scope a scan.
    pub fn region_of(&self, addr: u64) -> Value {
        let proc = self.proc.lock().unwrap();
        match proc.regions(addr, addr + 1, true).into_iter().next() {
            Some(r) => json!({"base": r.base, "size": r.size, "protect": r.protect,
                              "writable": r.writable, "kind": format!("{:?}", r.kind)}),
            None => json!({"base": addr, "size": 0x1000, "protect": 0, "writable": false}),
        }
    }

    pub fn modules(&self) -> Value {
        let proc = self.proc.lock().unwrap();
        let list: Vec<Value> = proc
            .modules
            .iter()
            .map(|m| json!({"name": m.name, "base": m.base, "size": m.size}))
            .collect();
        json!({"count": list.len(), "modules": list})
    }

    fn encode(&self, vtype: &str, value: &str) -> Result<(VType, Vec<u8>)> {
        let t = VType::parse(vtype)?;
        let b = t.encode(value)?;
        Ok((t, b))
    }

    pub fn read(&self, addr: u64, vtype: &str) -> Result<Value> {
        let t = VType::parse(vtype)?;
        let len = if t.width() == 0 { 64 } else { t.width() };
        let proc = self.proc.lock().unwrap();
        let bytes = proc
            .read(addr, len)
            .with_context(|| format!("read {addr:#x} ({len} bytes)"))?;
        Ok(json!({"addr": addr, "addr_hex": format!("{addr:X}"), "type": t.name(), "hex": hex(&bytes), "value": display(t, &bytes)}))
    }

    pub fn write(&self, addr: u64, vtype: &str, value: &str) -> Result<Value> {
        let (t, bytes) = self.encode(vtype, value)?;
        {
            let mut proc = self.proc.lock().unwrap();
            proc.write(addr, &bytes)
                .with_context(|| format!("write {addr:#x}"))?;
        }
        self.log_write(self.pid(), addr, &bytes);
        Ok(json!({"addr": addr, "addr_hex": format!("{addr:X}"), "type": t.name(), "hex": hex(&bytes)}))
    }

    pub fn create_session(
        &self,
        vtype: &str,
        scope: &str,
        alignment: usize,
        cmp: &str,
        value: Option<&str>,
        value2: Option<&str>,
    ) -> Result<Value> {
        let t = VType::parse(vtype)?;
        let c = Cmp::parse(cmp)?;
        let scope = Scope::parse(scope)?;
        let mut s = Session::new(self.pid(), t, scope, alignment, c);
        if c.needs_arg() {
            let v = value.context("comparison needs a value")?;
            s.value = Some(t.encode(v)?);
            if c == Cmp::Between {
                s.value2 = Some(t.encode(value2.context("between needs two values")?)?);
            }
        }
        if !s.value.is_some() && !c.needs_arg() && t.is_text() {
            bail!("string/aob scans need a value");
        }
        let s = Arc::new(Mutex::new(s));
        let id = format!("s{}_{}", stamp(), self.ids.fetch_add(1, Ordering::Relaxed));
        self.sessions.lock().unwrap().insert(id.clone(), s.clone());
        let me = self.clone();
        std::thread::spawn(move || {
            let proc = me.proc.lock().unwrap();
            let mut s = s.lock().unwrap();
            let _ = s.first_scan(&*proc);
        });
        Ok(json!({"session": id}))
    }

    fn session(&self, id: &str) -> Result<Arc<Mutex<Session>>> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .with_context(|| format!("no session '{id}'"))
    }

    /// Re-run the first scan in place with a new comparison (used by the fast
    /// seed/changed/decreased rhythm of the i-frame hunt).
    pub fn session_reseed(&self, id: &str, cmp: &str, value: Option<&str>, value2: Option<&str>) -> Result<Value> {
        let s = self.session(id)?;
        let c = Cmp::parse(cmp)?;
        let t = s.lock().unwrap().vtype;
        {
            let mut s = s.lock().unwrap();
            s.cmp = c;
            s.value = match value {
                Some(v) => Some(t.encode(v)?),
                None => None,
            };
            s.value2 = match value2 {
                Some(v) => Some(t.encode(v)?),
                None => None,
            };
        }
        let proc = self.proc.lock().unwrap();
        let mut s = s.lock().unwrap();
        s.first_scan(&*proc)?;
        Ok(json!({"session": id, "left": s.len()}))
    }

    pub fn next_scan(&self, id: &str, cmp: &str, value: Option<&str>) -> Result<Value> {
        let s = self.session(id)?;
        let c = Cmp::parse(cmp)?;
        let t = s.lock().unwrap().vtype;
        let mut extra = None;
        if c.needs_arg() {
            extra = Some(t.encode(value.context("comparison needs a value")?)?);
        }
        let proc = self.proc.lock().unwrap();
        let mut s = s.lock().unwrap();
        if extra.is_some() {
            s.value = extra;
        }
        let left = s.next_scan(&*proc, c)?;
        Ok(json!({"session": id, "left": left, "passes": s.passes}))
    }

    pub fn results(&self, id: &str, offset: usize, limit: usize) -> Result<Value> {
        let s = self.session(id)?;
        let s = s.lock().unwrap();
        let limit = limit.clamp(1, MAX_WINDOW);
        let rows: Vec<Value> = s
            .window(offset, limit)
            .into_iter()
            .map(|(a, v)| json!({"addr": a, "addr_hex": format!("{a:X}"), "hex": hex(&v), "value": display(s.vtype, &v)}))
            .collect();
        Ok(json!({
            "session": id,
            "count": s.len(),
            "passes": s.passes,
            "state": format!("{:?}", s.state).to_lowercase(),
            "offset": offset,
            "rows": rows,
        }))
    }

    pub fn session_reset(&self, id: &str) -> Result<Value> {
        let s = self.session(id)?;
        s.lock().unwrap().reset();
        Ok(json!({"session": id, "count": 0}))
    }

    pub fn session_cancel(&self, id: &str) -> Result<Value> {
        let s = self.session(id)?;
        s.lock().unwrap().cancel_handle().store(true, Ordering::Relaxed);
        Ok(json!({"session": id, "cancelled": true}))
    }

    pub fn session_undo(&self, id: &str) -> Result<Value> {
        let s = self.session(id)?;
        let ok = s.lock().unwrap().undo();
        Ok(json!({"session": id, "undone": ok}))
    }

    pub fn session_drop(&self, id: &str) -> Result<Value> {
        self.sessions.lock().unwrap().remove(id);
        Ok(json!({"dropped": id}))
    }

    /// Start the in-engine hunter: watches `hp_addr`, and whenever it drops runs a
    /// seed/changed/decreased cycle over `scope` so only damage-shaped fields survive.
    pub fn hunt_start(&self, hp_addr: u64, vtype: &str, scope: &str, settle_ms: u64, fall_ms: u64) -> Result<Value> {
        let t = VType::parse(vtype)?;
        let scope = Scope::parse(scope)?;
        if self.hunt.lock().unwrap().running {
            bail!("hunt already running");
        }
        let mut s = Session::new(self.pid(), t, scope, 4, Cmp::Eq);
        s.value = Some(t.encode("0")?);
        let s = Arc::new(Mutex::new(s));
        let sid = format!("h{}_{}", stamp(), self.ids.fetch_add(1, Ordering::Relaxed));
        self.sessions.lock().unwrap().insert(sid.clone(), s.clone());
        {
            let mut h = self.hunt.lock().unwrap();
            *h = Hunt { running: true, session: sid.clone(), ..Hunt::default() };
        }
        let me = self.clone();
        let settle = Duration::from_millis(settle_ms.max(20));
        let fall = Duration::from_millis(fall_ms.max(50));
        std::thread::spawn(move || {
            let mut last = {
                let proc = me.proc.lock().unwrap();
                proc.read(hp_addr, 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0)
            };
            loop {
                std::thread::sleep(Duration::from_millis(10));
                if !me.hunt.lock().unwrap().running {
                    break;
                }
                let cur = {
                    let proc = me.proc.lock().unwrap();
                    match proc.read(hp_addr, 4) {
                        Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                        None => continue,
                    }
                };
                if cur >= last {
                    last = cur;
                    me.hunt.lock().unwrap().last_hp = cur;
                    continue;
                }
                last = cur;
                {
                    let mut h = me.hunt.lock().unwrap();
                    h.hits += 1;
                    h.last_hp = cur;
                }
                // seed to 0, then keep what rose and fell back
                {
                    let proc = me.proc.lock().unwrap();
                    let mut ses = s.lock().unwrap();
                    if let Err(e) = ses.first_scan(&*proc) {
                        me.hunt.lock().unwrap().error = Some(e.to_string());
                        continue;
                    }
                }
                std::thread::sleep(settle);
                let n1 = {
                    let proc = me.proc.lock().unwrap();
                    let mut ses = s.lock().unwrap();
                    ses.next_scan(&*proc, Cmp::Changed).unwrap_or(0)
                };
                let _ = n1;
                std::thread::sleep(fall);
                let n2 = {
                    let proc = me.proc.lock().unwrap();
                    let mut ses = s.lock().unwrap();
                    ses.next_scan(&*proc, Cmp::Decreased).unwrap_or(0)
                };
                {
                    let mut h = me.hunt.lock().unwrap();
                    h.rounds += 1;
                    h.survivors = n2;
                    h.last_hp = cur;
                }
                if n2 > 0 && n2 <= 4000 {
                    let rows = {
                        let ses = s.lock().unwrap();
                        ses.window(0, 4000)
                    };
                    let mut h = me.hunt.lock().unwrap();
                    for (addr, _) in rows {
                        *h.counts.entry(addr).or_insert(0) += 1;
                    }
                }
            }
        });
        Ok(json!({"session": sid}))
    }

    pub fn hunt_state(&self) -> Value {
        let h = self.hunt.lock().unwrap();
        let mut top: Vec<(u64, u32)> = h.counts.iter().map(|(a, n)| (*a, *n)).collect();
        top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        let rows: Vec<Value> = top
            .iter()
            .take(30)
            .map(|(a, n)| json!({"addr": a, "addr_hex": format!("{a:X}"), "hits": n}))
            .collect();
        json!({"running": h.running, "hits": h.hits, "rounds": h.rounds,
               "survivors": h.survivors, "last_hp": h.last_hp,
               "session": h.session, "error": h.error, "top": rows})
    }

    pub fn hunt_stop(&self) -> Value {
        let mut h = self.hunt.lock().unwrap();
        h.running = false;
        let sid = h.session.clone();
        json!({"stopped": true, "session": sid})
    }
    pub fn freeze_add(&self, addr: u64, vtype: &str, value: &str, interval_ms: u64) -> Result<Value> {
        let (t, _) = self.encode(vtype, value)?;
        let id = self.ids.fetch_add(1, Ordering::Relaxed);
        let entry = Entry {
            id,
            addr,
            vtype: t.name().to_string(),
            value: value.to_string(),
            interval_ms: interval_ms.max(10),
            active: true,
            last: None,
            writes: 0,
            errors: 0,
        };
        // write once immediately so a freeze takes effect before the first tick
        {
            let bytes = entry.bytes().unwrap();
            let mut proc = self.proc.lock().unwrap();
            proc.write(addr, &bytes)?;
        }
        self.freezes.lock().unwrap().push(entry);
        Ok(json!({"id": id}))
    }

    pub fn freeze_list(&self) -> Value {
        let list = self.freezes.lock().unwrap();
        let rows: Vec<Value> = list
            .iter()
            .map(|e| {
                json!({"id": e.id, "addr": e.addr, "addr_hex": format!("{:X}", e.addr), "type": e.vtype, "value": e.value,
                       "interval_ms": e.interval_ms, "active": e.active,
                       "writes": e.writes, "errors": e.errors})
            })
            .collect();
        json!({"count": rows.len(), "entries": rows})
    }

    pub fn freeze_update(&self, id: u64, value: Option<&str>, active: Option<bool>) -> Result<Value> {
        let mut list = self.freezes.lock().unwrap();
        let e = list
            .iter_mut()
            .find(|e| e.id == id)
            .with_context(|| format!("no freeze id {id}"))?;
        if let Some(v) = value {
            e.vtype().encode(v)?;
            e.value = v.to_string();
        }
        if let Some(a) = active {
            e.active = a;
            e.last = None;
        }
        Ok(json!({"id": id, "value": e.value, "active": e.active}))
    }

    pub fn freeze_remove(&self, id: u64) -> Result<Value> {
        let mut list = self.freezes.lock().unwrap();
        let before = list.len();
        list.retain(|e| e.id != id);
        Ok(json!({"removed": before - list.len()}))
    }

    pub fn freeze_clear(&self) -> Value {
        let mut list = self.freezes.lock().unwrap();
        let n = list.len();
        list.clear();
        json!({"removed": n})
    }

    /// Load a cheat table; with `apply` the active entries are read and frozen.
    pub fn ct_load(&self, path: &str, apply: bool) -> Result<Value> {
        let table = ct::load(std::path::Path::new(path))?;
        let mut proc = self.proc.lock().unwrap();
        let mut out = Vec::new();
        for e in &table.entries {
            let mut row = json!({
                "id": e.id, "description": e.description, "type": e.vtype,
                "value": e.value, "active": e.active,
            });
            match ct::resolve(&proc, e) {
                Ok(addr) => {
                    row["addr"] = json!(addr);
                    let t = VType::parse(&e.vtype)?;
                    let len = t.width().max(1);
                    if let Some(bytes) = proc.read(addr, len) {
                        row["current"] = json!(display(t, &bytes));
                    }
                    if apply && e.active {
                        let bytes = t.encode(&e.value)?;
                        proc.write(addr, &bytes)?;
                        let id = self.ids.fetch_add(1, Ordering::Relaxed);
                        self.freezes.lock().unwrap().push(Entry {
                            id,
                            addr,
                            vtype: t.name().to_string(),
                            value: e.value.clone(),
                            interval_ms: 100,
                            active: true,
                            last: None,
                            writes: 0,
                            errors: 0,
                        });
                        row["frozen"] = json!(id);
                    }
                }
                Err(err) => row["error"] = json!(err.to_string()),
            }
            out.push(row);
        }
        Ok(json!({"path": table.path, "entries": out}))
    }

    pub fn ct_apply(&self, table: &ct::Table, ids: &[u32]) -> Result<Value> {
        let mut proc = self.proc.lock().unwrap();
        let mut applied = 0;
        for e in table.entries.iter().filter(|e| ids.contains(&e.id)) {
            let addr = ct::resolve(&proc, e)?;
            let t = VType::parse(&e.vtype)?;
            proc.write(addr, &t.encode(&e.value)?)?;
            applied += 1;
        }
        Ok(json!({"applied": applied}))
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

pub fn display(t: VType, b: &[u8]) -> String {
    match t {
        VType::U8 => b.first().map(|v| v.to_string()).unwrap_or_default(),
        VType::U16 if b.len() >= 2 => u16::from_le_bytes([b[0], b[1]]).to_string(),
        VType::U32 if b.len() >= 4 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]).to_string(),
        VType::U64 if b.len() >= 8 => u64::from_le_bytes(b[..8].try_into().unwrap()).to_string(),
        VType::F32 if b.len() >= 4 => format!("{}", f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        VType::F64 if b.len() >= 8 => format!("{}", f64::from_le_bytes(b[..8].try_into().unwrap())),
        VType::StrUtf16 => {
            let units: Vec<u16> = b
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|u| *u != 0)
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => b.iter().take_while(|x| **x != 0).map(|x| *x as char).collect(),
    }
}
