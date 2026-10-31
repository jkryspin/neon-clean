//! neon-loader: launch, patch, mock, inject, verify and open neon-ce.

mod config;
mod gui;
mod launch;

fn main() -> eframe::Result<()> {
    if std::env::args().any(|a| a == "--headless") {
        let (tx, rx) = std::sync::mpsc::channel();
        launch::start(
            tx,
            launch::Options {
                auto_open_ce: false,
                kill_existing: true,
            },
        );
        while let Ok(event) = rx.recv() {
            match event {
                launch::Event::Step(i, state, detail) => {
                    println!("{:<20} {:?} {}", launch::STEPS[i], state, detail);
                }
                launch::Event::Log(line) => println!("{line}"),
                launch::Event::Done(ok) => {
                    println!("{}", if ok { "done" } else { "failed" });
                    std::process::exit(if ok { 0 } else { 1 });
                }
            }
        }
        return Ok(());
    }
    gui::run()
}
