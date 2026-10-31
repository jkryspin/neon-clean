//! egui front end. Talks to the in-process engine only; the same API is what the MCP
//! bridge forwards to, so anything on screen is reproducible from a tool call.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use serde_json::{json, Value};

use crate::proc::{self, Process};
use crate::region::Scope;
use crate::session::Session;
use crate::state::Engine;
use crate::value::{Cmp, VType};

const TYPES: &[&str] = &["4 bytes", "byte", "2 bytes", "8 bytes", "float", "double", "string", "utf16", "aob"];
const CMPS: &[&str] = &["eq", "ne", "lt", "gt", "between", "increased", "decreased", "changed", "unchanged", "sameasfirst"];
const SCOPES: &[&str] = &["heap", "private", "all", "module", "image", "mapped"];

struct Row {
    addr: u64,
    value: String,
}

pub struct Gui {
    engine: Option<Arc<Engine>>,
    pid: String,
    vtype: String,
    scope: String,
    module: String,
    align: usize,
    cmp: String,
    value: String,
    value2: String,
    session: Option<String>,
    rows: Vec<Row>,
    count: usize,
    passes: u32,
    busy: bool,
    toast: String,
    status: String,
    freezes: Vec<Value>,
    hp: Option<f32>,
    max_seen: f32,
    max_hp: u32,
    last_poll: Instant,
    last_bars: Instant,
    auto: bool,
}

impl Default for Gui {
    fn default() -> Gui {
        Gui {
            engine: None,
            pid: String::new(),
            vtype: "4 bytes".into(),
            scope: "heap".into(),
            module: "MapleStory.exe".into(),
            align: 4,
            cmp: "between".into(),
            value: String::new(),
            value2: String::new(),
            session: None,
            rows: Vec::new(),
            count: 0,
            passes: 0,
            busy: false,
            toast: String::new(),
            status: "not attached".into(),
            freezes: Vec::new(),
            hp: None,
            max_seen: 1.0,
            max_hp: 162,
            last_poll: Instant::now(),
            last_bars: Instant::now(),
            auto: true,
        }
    }
}

impl Gui {
    fn scope(&self) -> String {
        if self.scope == "module" {
            format!("module:{}", self.module.trim())
        } else {
            self.scope.clone()
        }
    }

    fn attach(&mut self) {
        let pid = if self.pid.trim().is_empty() {
            proc::find_game_pid()
        } else {
            self.pid.trim().parse().ok()
        };
        let Some(pid) = pid else {
            self.toast = "MapleStory.exe is not running".into();
            return;
        };
        match Engine::attach(pid) {
            Ok(engine) => {
                match crate::api::serve(engine.clone(), 8790) {
                    Ok(srv) => {
                        let _ = Session {
                            pid,
                            port: srv.port,
                            token: srv.token,
                            started_ms: 0,
                        }
                        .write();
                        self.status = format!("attached pid {pid}, api on 127.0.0.1:{}", srv.port);
                    }
                    Err(e) => self.status = format!("api failed: {e}"),
                }
                self.engine = Some(engine);
                self.pid = pid.to_string();
                self.toast = "attached".into();
            }
            Err(e) => self.toast = format!("attach failed: {e}"),
        }
    }

    fn poll(&mut self) {
        let Some(engine) = &self.engine else { return };
        if !engine.alive() {
            self.engine = None;
            self.status = "target exited".into();
            return;
        }
        self.freezes = engine
            .freeze_list()
            .get("entries")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let Some(id) = self.session.clone() else { return };
        if let Ok(v) = engine.results(&id, 0, 400) {
            self.count = v["count"].as_u64().unwrap_or(0) as usize;
            self.passes = v["passes"].as_u64().unwrap_or(0) as u32;
            self.rows = v["rows"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|r| Row {
                            addr: r["addr"].as_u64().unwrap_or(0),
                            value: r["value"].as_str().unwrap_or("").to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
    }

    fn hp_bars(&mut self) {
        let Some(pid) = self.engine.as_ref().map(|e| e.pid()) else { return };
        if let Some((hp, _mp)) = read_bars(pid) {
            let hp = hp as f32;
            if hp > 0.0 {
                self.max_seen = self.max_seen.max(hp);
                self.hp = Some(hp / self.max_seen);
            }
        }
    }

    fn ui_scan(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("type");
            egui::ComboBox::from_id_salt("vtype")
                .selected_text(&self.vtype)
                .show_ui(ui, |ui| {
                    for t in TYPES {
                        ui.selectable_value(&mut self.vtype, t.to_string(), *t);
                    }
                });
            ui.label("scope");
            egui::ComboBox::from_id_salt("scope")
                .selected_text(&self.scope)
                .show_ui(ui, |ui| {
                    for s in SCOPES {
                        ui.selectable_value(&mut self.scope, s.to_string(), *s);
                    }
                });
            if self.scope == "module" {
                ui.text_edit_singleline(&mut self.module);
            }
            ui.label("align");
            for a in [1usize, 2, 4, 8] {
                ui.selectable_value(&mut self.align, a, a.to_string());
            }
        });
        ui.horizontal(|ui| {
            ui.label("compare");
            egui::ComboBox::from_id_salt("cmp")
                .selected_text(&self.cmp)
                .show_ui(ui, |ui| {
                    for c in CMPS {
                        ui.selectable_value(&mut self.cmp, c.to_string(), *c);
                    }
                });
            let needs = Cmp::parse(&self.cmp).map(|c| c.needs_arg()).unwrap_or(false);
            ui.label("value");
            ui.add_enabled(needs, egui::TextEdit::singleline(&mut self.value).desired_width(90.0));
            if self.cmp == "between" {
                ui.label("and");
                ui.add(egui::TextEdit::singleline(&mut self.value2).desired_width(90.0));
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Scan").clicked() {
                self.start_scan();
            }
            if ui.button("Next scan").clicked() {
                self.next_scan();
            }
            if ui.button("Undo").clicked() {
                if let (Some(e), Some(id)) = (&self.engine, &self.session) {
                    let _ = e.session_undo(id);
                }
            }
            if ui.button("Reset").clicked() {
                if let (Some(e), Some(id)) = (&self.engine, &self.session) {
                    let _ = e.session_reset(id);
                    self.toast = "cancelled/reset".into();
                }
            }
            if ui.button("Cancel").clicked() {
                if let (Some(e), Some(id)) = (&self.engine, &self.session) {
                    let _ = e.session_cancel(id);
                    self.toast = "cancelled".into();
                }
            }
            ui.separator();
            ui.label(format!("{} hits, pass {}", self.count, self.passes));
            // value from the HP bar, for the common case
            if let Some(p) = self.hp {
                let v = (p * self.max_hp as f32).round() as u32;
                if ui.button(format!("use HP ({v})")).clicked() {
                    self.value = v.to_string();
                    self.vtype = "4 bytes".into();
                    self.cmp = "eq".into();
                }
            }
        });
    }

    fn start_scan(&mut self) {
        let Some(e) = &self.engine else {
            self.toast = "attach first".into();
            return;
        };
        match e.create_session(
            &self.vtype,
            &self.scope(),
            self.align,
            &self.cmp,
            non_empty(&self.value),
            non_empty(&self.value2),
        ) {
            Ok(v) => {
                self.session = v["session"].as_str().map(str::to_string);
                self.rows.clear();
                self.busy = true;
                self.toast = "scan started".into();
            }
            Err(err) => self.toast = format!("scan: {err}"),
        }
    }

    fn next_scan(&mut self) {
        let (Some(e), Some(id)) = (&self.engine, &self.session) else {
            self.toast = "scan first".into();
            return;
        };
        match e.next_scan(&id.clone(), &self.cmp, non_empty(&self.value)) {
            Ok(v) => self.toast = format!("kept {}", v["left"]),
            Err(err) => self.toast = format!("next: {err}"),
        }
    }

    fn ui_results(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("results").max_height(260.0).show(ui, |ui| {
            egui::Grid::new("rows").striped(true).show(ui, |ui| {
                ui.label("address");
                ui.label("value");
                ui.label("");
                ui.end_row();
                for row in &self.rows {
                    ui.monospace(format!("{:X}", row.addr));
                    ui.monospace(&row.value);
                    if ui.small_button("freeze").clicked() {
                        if let Some(e) = &self.engine {
                            let _ = e.freeze_add(row.addr, &self.vtype, &row.value, 100);
                        }
                    }
                    ui.end_row();
                }
            });
        });
    }

    fn ui_freezes(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(format!("{} frozen", self.freezes.len()));
            if ui.button("clear all").clicked() {
                if let Some(e) = &self.engine {
                    e.freeze_clear();
                }
            }
            if ui.button("save table").clicked() {
                self.save_table();
            }
        });
        egui::ScrollArea::vertical().id_salt("freezes").max_height(120.0).show(ui, |ui| {
            egui::Grid::new("frozen").striped(true).show(ui, |ui| {
                for f in self.freezes.clone() {
                    let id = f["id"].as_u64().unwrap_or(0);
                    ui.monospace(f["addr_hex"].as_str().unwrap_or("?"));
                    ui.monospace(format!("{} = {}", f["type"].as_str().unwrap_or(""), f["value"].as_str().unwrap_or("")));
                    ui.label(format!("w{} e{}", f["writes"], f["errors"]));
                    if ui.small_button("remove").clicked() {
                        if let Some(e) = &self.engine {
                            let _ = e.freeze_remove(id);
                        }
                    }
                    ui.end_row();
                }
            });
        });
    }

    fn save_table(&mut self) {
        let Some(e) = &self.engine else { return };
        let Some(path) = rfd_like_path() else { return };
        let entries: Vec<crate::ct::Entry> = self
            .freezes
            .iter()
            .enumerate()
            .map(|(i, f)| crate::ct::Entry {
                id: i as u32 + 1,
                description: format!("frozen {}", f["addr_hex"].as_str().unwrap_or("?")),
                vtype: f["type"].as_str().unwrap_or("4 bytes").into(),
                module: None,
                static_offset: f["addr"].as_u64().unwrap_or(0),
                offsets: Vec::new(),
                value: f["value"].as_str().unwrap_or("0").into(),
                active: true,
            })
            .collect();
        let table = crate::ct::Table { path: path.clone(), entries };
        let _ = e;
        match crate::ct::save(&table, std::path::Path::new(&path)) {
            Ok(()) => self.toast = format!("saved {path}"),
            Err(err) => self.toast = format!("save: {err}"),
        }
    }
}

fn non_empty(s: &str) -> Option<&str> {
    (!s.trim().is_empty()).then_some(s)
}

fn rfd_like_path() -> Option<String> {
    Some("run/hp-table.ct".to_string())
}

impl eframe::App for Gui {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.auto && self.engine.is_none() {
            self.auto = false;
            self.attach();
        }
        if self.last_poll.elapsed() > Duration::from_millis(700) {
            self.last_poll = Instant::now();
            self.poll();
            self.busy = false;
        }
        if self.last_bars.elapsed() > Duration::from_millis(300) {
            self.last_bars = Instant::now();
            self.hp_bars();
            ctx.request_repaint_after(Duration::from_millis(300));
        }

        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("neon-ce");
                ui.label("pid");
                ui.add(egui::TextEdit::singleline(&mut self.pid).desired_width(70.0));
                if ui.button("Attach").clicked() {
                    self.attach();
                }
                if ui.button("Find game").clicked() {
                    if let Some(p) = proc::find_game_pid() {
                        self.pid = p.to_string();
                    }
                }
                ui.separator();
                if let Some(p) = self.hp {
                    let v = (p * self.max_hp as f32).round() as u32;
                    ui.label(format!("HP ~{v} ({:.0}%)", p * 100.0));
                    ui.add(egui::ProgressBar::new(p).desired_width(120.0));
                    ui.label("max");
                    ui.add(egui::DragValue::new(&mut self.max_hp).range(1..=9999));
                } else {
                    ui.label("HP: ?");
                }
            });
            ui.label(&self.status);
        });

        egui::Panel::bottom("toast").show(ui, |ui| {
            ui.label(if self.toast.is_empty() { "ready" } else { &self.toast });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.engine.is_none() {
                ui.label("Attach to MapleStory.exe to start. Run elevated.");
                return;
            }
            self.ui_scan(ui);
            ui.separator();
            ui.columns(2, |cols| {
                cols[0].label("results");
                self.ui_results(&mut cols[0]);
                cols[1].label("freezes");
                self.ui_freezes(&mut cols[1]);
            });
        });
    }
}

/// HP/MP bar pixel widths for a pid, from the game window.
fn read_bars(pid: u32) -> Option<(u32, u32)> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, IsWindowVisible, ShowWindow, SW_SHOW,
    };

    struct Ctx {
        pid: u32,
        hit: Option<HWND>,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lparam: windows::Win32::Foundation::LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lparam.0 as *mut Ctx);
        let mut p = 0u32;
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut p));
        if p == ctx.pid {
            ctx.hit = Some(hwnd);
            return false.into();
        }
        true.into()
    }
    let mut ctx = Ctx { pid, hit: None };
    unsafe {
        let _ = EnumWindows(Some(cb), windows::Win32::Foundation::LPARAM(&mut ctx as *mut _ as isize));
    }
    let hwnd = ctx.hit?;
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r).ok()?;
        let x0 = r.left + 560;
        let x1 = r.left + 1040;
        let y0 = r.top + 1180;
        let y1 = (r.top + 1260).min(r.bottom);
        let w = x1 - x0;
        let h = y1 - y0;
        if w <= 0 || h <= 0 {
            return None;
        }
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let _ = BitBlt(mem, 0, 0, w, h, Some(screen), x0, y0, SRCCOPY);
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h;
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB.0;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        GetDIBits(mem, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);

        let mut best = (0u32, 0u32);
        for y in 0..h {
            let (mut red, mut blue) = (0u32, 0u32);
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let (b, g, rr) = (buf[i] as u32, buf[i + 1] as u32, buf[i + 2] as u32);
                if rr > 110 && rr > g + 25 && rr > b + 25 {
                    red += 1;
                } else if b > 110 && b > rr + 25 && b > g + 25 {
                    blue += 1;
                }
            }
            if red > best.0 {
                best = (red, blue);
            }
        }
        Some(best)
    }
}

pub fn run() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 680.0])
            .with_always_on_top(),
        ..Default::default()
    };
    eframe::run_native("neon-ce", opts, Box::new(|_cc| Ok(Box::<Gui>::default())))
}

#[allow(dead_code)]
fn _keep(_: &Process, _: &Scope, _: &VType, _: &Value) {}
