//! Workbench shell: live data, entities, map, bot, cheat engine, packets, logs.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::gui::Cheat;
use crate::live::{Live, Reader};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Live,
    Entities,
    Map,
    Bot,
    Cheat,
    Packets,
    Logs,
}

pub struct Workbench {
    cheat: Cheat,
    tab: Tab,
    pid: String,
    reader: Option<Reader>,
    bot: Option<neon_bot::runner::Session>,
    logs: Arc<Mutex<Vec<String>>>,
    api: Arc<crate::web::State>,
    mobs: String,
    home_map: String,
    auto_attach: bool,
    status: String,
    last_feed: Instant,
    registry_filter: String,
}

impl Workbench {
    pub fn new(initial_pid: Option<u32>, api: Arc<crate::web::State>) -> Workbench {
        let mut wb = Workbench {
            cheat: Cheat::default(),
            tab: Tab::Live,
            pid: initial_pid.map(|p| p.to_string()).unwrap_or_default(),
            reader: None,
            bot: None,
            logs: Arc::new(Mutex::new(Vec::new())),
            api,
            mobs: "2,3,5".into(),
            home_map: "10001030".into(),
            auto_attach: initial_pid.is_none(),
            status: String::new(),
            last_feed: Instant::now(),
            registry_filter: String::new(),
        };
        if initial_pid.is_some() {
            wb.attach();
        }
        wb
    }

    pub fn logs(&self) -> Arc<Mutex<Vec<String>>> {
        self.logs.clone()
    }

    pub fn live(&self) -> Option<Live> {
        self.reader.as_ref().map(|r| r.snapshot())
    }

    fn log(&self, line: impl Into<String>) {
        let mut logs = self.logs.lock().unwrap();
        logs.push(line.into());
        if logs.len() > 2000 {
            logs.remove(0);
        }
    }

    fn attach(&mut self) {
        let pid = self
            .pid
            .trim()
            .parse::<u32>()
            .ok()
            .or_else(neon_win::process::find_game_pid);
        let Some(pid) = pid else {
            self.status = "MapleStory.exe is not running".into();
            return;
        };
        let data = data_dir();
        match Reader::start(pid, data) {
            Ok(reader) => {
                self.pid = pid.to_string();
                self.reader = Some(reader);
                self.status = format!("reading pid {pid}");
                self.log(format!("attached pid {pid}"));
                if self.tab == Tab::Live {
                    self.tab = Tab::Live;
                }
            }
            Err(e) => {
                self.status = format!("attach failed: {e}");
                self.log(self.status.clone());
            }
        }
    }

    fn start_bot(&mut self) {
        if self.reader.is_none() {
            self.attach();
        }
        if self.reader.is_none() {
            return;
        }
        let mobs: Vec<u32> = self
            .mobs
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .collect();
        let root = data_dir()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let status = root.join("neon-status.json");
        let control = root.join("neon-control.json");
        match neon_bot::runner::Session::start(
            status,
            control,
            data_dir().join("maps"),
            mobs,
            self.home_map.parse().ok(),
            "Pause",
        ) {
            Ok(session) => {
                self.log("bot started");
                self.bot = Some(session);
                self.tab = Tab::Bot;
            }
            Err(e) => {
                self.log(format!("bot failed: {e}"));
                self.status = e;
            }
        }
    }

    fn stop_bot(&mut self) {
        if let Some(mut b) = self.bot.take() {
            b.stop();
            self.log("bot stopped");
        }
    }

    fn feed(&mut self) {
        if self.last_feed.elapsed() < Duration::from_millis(200) {
            return;
        }
        self.last_feed = Instant::now();
        let Some(reader) = &self.reader else { return };
        let state = reader.latest_state.lock().unwrap().take();
        if let (Some(bot), Some(state)) = (&self.bot, state) {
            bot.feed(state);
        }
    }
}

impl eframe::App for Workbench {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.auto_attach && self.reader.is_none() {
            self.auto_attach = false;
            self.attach();
        }
        self.feed();
        if let Some(live) = self.live() {
            *self.api.live.lock().unwrap() = Some(live);
        }
        *self.api.logs.lock().unwrap() = self.logs.lock().unwrap().clone();
        ui.ctx().request_repaint_after(Duration::from_millis(250));

        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("neon");
                ui.separator();
                for (tab, name) in [
                    (Tab::Live, "Live"),
                    (Tab::Entities, "Entities"),
                    (Tab::Map, "Map"),
                    (Tab::Bot, "Bot"),
                    (Tab::Cheat, "Cheat"),
                    (Tab::Packets, "Packets"),
                    (Tab::Logs, "Logs"),
                ] {
                    if ui.selectable_label(self.tab == tab, name).clicked() {
                        self.tab = tab;
                    }
                }
                ui.separator();
                ui.label("pid");
                ui.add(egui::TextEdit::singleline(&mut self.pid).desired_width(70.0));
                if ui.button("Attach").clicked() {
                    self.attach();
                }
                if ui.button("Find game").clicked() {
                    if let Some(p) = neon_win::process::find_game_pid() {
                        self.pid = p.to_string();
                    }
                }
            });
            ui.label(&self.status);
        });

        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Live => self.ui_live(ui),
            Tab::Entities => self.ui_entities(ui),
            Tab::Map => self.ui_map(ui),
            Tab::Bot => self.ui_bot(ui),
            Tab::Cheat => self.cheat.ui(ui),
            Tab::Packets => self.ui_packets(ui),
            Tab::Logs => self.ui_logs(ui),
        });
    }
}

impl Workbench {
    fn ui_live(&mut self, ui: &mut egui::Ui) {
        let Some(snap) = self.reader.as_ref().map(|r| r.snapshot()) else {
            ui.label("Not attached. Run neon-loader, or enter a pid and press Attach.");
            return;
        };
        egui::Grid::new("live").striped(true).show(ui, |ui| {
            let rows = [
                ("HP", snap.hp.map(|v| v.to_string()).unwrap_or("?".into())),
                ("MP", snap.mp.map(|v| v.to_string()).unwrap_or("?".into())),
                (
                    "Level",
                    snap.level.map(|v| v.to_string()).unwrap_or("?".into()),
                ),
                (
                    "Position",
                    snap.x
                        .zip(snap.y)
                        .map(|(x, y)| format!("{x},{y}"))
                        .unwrap_or("?".into()),
                ),
                (
                    "Map",
                    snap.map_id.map(|v| v.to_string()).unwrap_or("?".into()),
                ),
                (
                    "Grounded",
                    snap.grounded.map(|v| v.to_string()).unwrap_or("?".into()),
                ),
                (
                    "Camera",
                    snap.camera
                        .map(|(x, y)| format!("{x},{y}"))
                        .unwrap_or("?".into()),
                ),
                ("Footholds", snap.footholds.to_string()),
                (
                    "Contacts",
                    format!("{} ({} rects)", snap.contacts.len(), snap.rects),
                ),
                ("Sweeps", snap.sweep_done.to_string()),
            ];
            for (k, v) in rows {
                ui.label(k);
                ui.monospace(v);
                ui.end_row();
            }
        });
        if let Some(err) = &snap.error {
            ui.colored_label(egui::Color32::LIGHT_RED, err);
        }
        ui.separator();
        ui.label("Entity source: collision-rectangle signature scan. Classified roots land in data/entities.json.");
    }

    fn ui_entities(&mut self, ui: &mut egui::Ui) {
        let Some(snap) = self.reader.as_ref().map(|r| r.snapshot()) else {
            ui.label("Not attached.");
            return;
        };
        ui.label(format!("{} contacts near the player", snap.contacts.len()));
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("contacts").striped(true).show(ui, |ui| {
                ui.strong("x");
                ui.strong("y");
                ui.end_row();
                for c in &snap.contacts {
                    ui.monospace(c.x.to_string());
                    ui.monospace(c.y.to_string());
                    ui.end_row();
                }
            });
        });
    }

    fn ui_map(&mut self, ui: &mut egui::Ui) {
        let Some(snap) = self.reader.as_ref().map(|r| r.snapshot()) else {
            ui.label("Not attached.");
            return;
        };
        let (Some(px), Some(py)) = (snap.x, snap.y) else {
            ui.label("Position not readable.");
            return;
        };
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter();
        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(18, 14, 28));
        let span = 1400.0;
        let to_screen = |x: i32, y: i32| {
            let sx = rect.center().x + ((x - px) as f32 / span) * rect.width();
            let sy = rect.center().y + ((y - py) as f32 / span) * rect.width();
            egui::pos2(sx, sy)
        };
        if let Some(map) = &self.reader {
            let _ = map;
        }
        for c in &snap.contacts {
            painter.circle_filled(to_screen(c.x, c.y), 3.0, egui::Color32::LIGHT_RED);
        }
        painter.circle_filled(to_screen(px, py), 5.0, egui::Color32::LIGHT_GREEN);
        painter.text(
            rect.left_top() + egui::vec2(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            format!(
                "map {:?} · {} contacts · {px},{py}",
                snap.map_id,
                snap.contacts.len()
            ),
            egui::FontId::monospace(12.0),
            egui::Color32::GRAY,
        );
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    fn ui_bot(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.bot.is_some() {
                if ui.button("Stop bot").clicked() {
                    self.stop_bot();
                }
            } else if ui.button("Start bot").clicked() {
                self.start_bot();
            }
            ui.label("mobs");
            ui.add(egui::TextEdit::singleline(&mut self.mobs).desired_width(120.0));
            ui.label("home map");
            ui.add(egui::TextEdit::singleline(&mut self.home_map).desired_width(100.0));
        });
        ui.separator();
        if let Some(bot) = &self.bot {
            ui.label(format!("running · {}", bot.status_path.display()));
            if let Some(err) = bot.error() {
                ui.colored_label(egui::Color32::LIGHT_RED, err);
            }
        } else {
            ui.label("stopped");
        }
        if let Some(snap) = self.reader.as_ref().map(|r| r.snapshot()) {
            ui.separator();
            ui.label(format!(
                "HP {:?} · MP {:?} · level {:?} · contacts {}",
                snap.hp,
                snap.mp,
                snap.level,
                snap.contacts.len()
            ));
        }
    }

    fn ui_packets(&mut self, ui: &mut egui::Ui) {
        use neon_protocol::{Registry, Status};
        let Ok(registry) = Registry::builtin() else {
            return;
        };
        ui.label("Registry (packet capture is optional; entity data is read from memory)");
        ui.add(
            egui::TextEdit::singleline(&mut self.registry_filter)
                .hint_text("filter name or opcode")
                .desired_width(240.0),
        );
        let filter = self.registry_filter.to_ascii_lowercase();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("ops").striped(true).show(ui, |ui| {
                for op in registry.iter() {
                    let name = format!("{:?}", op);
                    if !filter.is_empty() && !name.to_ascii_lowercase().contains(&filter) {
                        continue;
                    }
                    let status = match op.status {
                        Status::Verified => "verified",
                        Status::Decoded => "decoded",
                        Status::Structure => "structure",
                        _ => "observed",
                    };
                    ui.monospace(format!("{:#06x}", op.id));
                    ui.label(op.name.as_deref().unwrap_or("?"));
                    ui.label(status);
                    ui.end_row();
                }
            });
        });
    }

    fn ui_logs(&mut self, ui: &mut egui::Ui) {
        let logs = self.logs.lock().unwrap();
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in logs.iter() {
                    ui.monospace(line);
                }
            });
    }
}

pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NEON_DATA") {
        return PathBuf::from(dir);
    }
    let mut dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));
    while let Some(d) = dir {
        if d.join("data").join("chains.json").exists() {
            return d.join("data");
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    PathBuf::from("data")
}
