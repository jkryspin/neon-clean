fn main() {
    let path = std::env::args().nth(1).expect("path");
    match neon_protocol::Registry::parse(&std::fs::read_to_string(path).unwrap()) {
        Ok(r) => println!("ok, {} entries", r.len()),
        Err(e) => println!("{e}"),
    }
}
