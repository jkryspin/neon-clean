//! cargo run --example replay -- <stamped packet log>   : replay a capture and print what the world looks like.
use neon_protocol::Registry;
use std::io::BufRead;

fn main() {
    let path = std::env::args().nth(1).expect("usage: replay <log>");
    let reg = Registry::builtin().expect("registry");
    let lines = std::io::BufReader::new(std::fs::File::open(path).expect("open"))
        .lines()
        .map_while(Result::ok);
    let (w, st) = neon_world::replay(&reg, lines);
    let s = w.snapshot();
    println!("{st:?}");
    println!(
        "map {:?}, {} mobs, {} players ({} named), {} drops",
        s.map,
        s.mobs.len(),
        s.players.len(),
        s.players.values().filter(|p| p.name.is_some()).count(),
        s.drops.len()
    );
    for (id, p) in s.players.iter().filter(|(_, p)| p.name.is_some()).take(8) {
        println!("  player {id}: {:?} at ({:?}, {:?})", p.name, p.x, p.y);
    }
}
