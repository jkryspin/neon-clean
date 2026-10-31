//! Launcher window: one visible step per launch-chain action.

use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui;

use crate::launch::{self, Event, Options, State, STEPS};

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([860.0, 620.0])
            .with_min_inner_size([700.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Neon Loader",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

struct App {
    steps: Vec<(State, String)>,
    logs: Vec<String>,
    auto_open: bool,
    kill_existing: bool,
    running: bool,
    rx: Option<Receiver<Event>>,
    tx: Option<Sender<Event>>,
}

impl App {
    fn new() -> App {
        App {
            steps: STEPS
                .iter()
                .map(|_| (State::Pending, String::new()))
                .collect(),
            logs: Vec::new(),
            auto_open: true,
            kill_existing: true,
            running: false,
            rx: None,
            tx: None,
        }
    }

    fn start(&mut self) {
        self.steps = STEPS
            .iter()
            .map(|_| (State::Pending, String::new()))
            .collect();
        self.logs.clear();
        self.running = true;
        let (tx, rx) = channel();
        self.tx = Some(tx.clone());
        self.rx = Some(rx);
        launch::start(
            tx,
            Options {
                auto_open_ce: self.auto_open,
                kill_existing: self.kill_existing,
            },
        );
    }

    fn drain(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::Step(i, state, detail) => {
                    if let Some(slot) = self.steps.get_mut(i) {
                        *slot = (state, detail);
                    }
                }
                Event::Log(line) => {
                    self.logs.push(line);
                    if self.logs.len() > 500 {
                        self.logs.remove(0);
                    }
                }
                Event::Done(ok) => {
                    self.running = false;
                    self.logs.push(if ok {
                        "done".into()
                    } else {
                        "stopped with errors".into()
                    });
                }
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain();
        if self.running {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::Panel::top("head").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Neon Loader");
                ui.label("patch · mock · inject · verify");
            });
            ui.horizontal(|ui| {
                let start = ui
                    .add_enabled(!self.running, egui::Button::new("Launch and attach"))
                    .clicked();
                if start {
                    self.start();
                }
                ui.checkbox(&mut self.auto_open, "open neon-ce when verified");
                ui.checkbox(&mut self.kill_existing, "close a running client");
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, name) in STEPS.iter().enumerate() {
                    let (state, detail) = &self.steps[i];
                    ui.horizontal(|ui| {
                        let (mark, color) = match state {
                            State::Pending => ("·", egui::Color32::GRAY),
                            State::Running => ("…", egui::Color32::LIGHT_BLUE),
                            State::Ok => ("✓", egui::Color32::LIGHT_GREEN),
                            State::Fail => ("✗", egui::Color32::LIGHT_RED),
                            State::Skip => ("–", egui::Color32::DARK_GRAY),
                        };
                        ui.colored_label(color, mark);
                        ui.strong(*name);
                        if !detail.is_empty() {
                            ui.label(detail);
                        }
                    });
                }
                ui.separator();
                ui.label("Log");
                egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                    for line in self.logs.iter().rev().take(120).rev() {
                        ui.monospace(line);
                    }
                });
            });
        });
    }
}
