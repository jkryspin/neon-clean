//! Round-trip the whole registry against real captured packets: decode each one, re-encode the decoded fields
//! onto it, and require the result to be byte-identical.
//!
//!   cargo run --example rt_check -- run/view.jib01.log run/sessions/*.log
//!
//! This is the cheapest strong check that the registry's field offsets, sizes, types and endianness are right,
//! and it does not depend on any human-assigned MEANING being correct - a field can be named wrongly and still
//! round-trip, but it cannot be positioned wrongly and round-trip. The committed unit tests
//! (tests/encode.rs) cover the same property on the checked-in fixtures; this example runs it over
//! however many millions of live packets are lying around in run/.
//!
//! Only lines the exact decoder produced are considered: lines marked "heuristic" and anything older than the
//! marker commit are skipped, for the reasons in scripts/registry-facts.py.

const HEURISTIC_MARKER_EPOCH: f64 = 1793069981.0;

fn main() {
    let r = neon_protocol::Registry::builtin().unwrap();
    let (mut ok, mut fail, mut nodec) = (0u64, 0u64, 0u64);
    let mut by_op: std::collections::BTreeMap<u16, (u64, u64)> = Default::default();
    let mut examples: Vec<String> = Vec::new();

    for arg in std::env::args().skip(1) {
        let f = std::fs::read_to_string(&arg).unwrap_or_default();
        let mut seen = 0;
        for line in f.lines() {
            let Some((head, hex)) = line.split_once(": ") else {
                continue;
            };
            if !head.contains(" op ") || head.ends_with("heuristic") {
                continue;
            }
            seen += 1;
            if seen <= 13 {
                continue; // the first frames of a capture predate IV sync
            }
            let Some(ts) = head.split(' ').next().and_then(|s| s.parse::<f64>().ok()) else {
                continue;
            };
            if ts < HEURISTIC_MARKER_EPOCH {
                continue;
            }
            let hex: String = hex.split_whitespace().collect();
            if hex.len() < 4 || hex.len() % 2 != 0 {
                continue;
            }
            let Ok(bytes) = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16))
                .collect::<Result<Vec<u8>, _>>()
            else {
                continue;
            };
            let id = u16::from_le_bytes([bytes[0], bytes[1]]);
            match r.decode(neon_protocol::Dir::In, &bytes) {
                None => nodec += 1,
                Some(d) => match r.encode_onto(neon_protocol::Dir::In, &bytes, &d.fields) {
                    Ok(out) if out == bytes => {
                        ok += 1;
                        by_op.entry(id).or_default().0 += 1;
                    }
                    other => {
                        fail += 1;
                        by_op.entry(id).or_default().1 += 1;
                        if examples.len() < 5 {
                            examples.push(match other {
                                Err(e) => format!("  {id:#06x}: {e}"),
                                _ => format!(
                                    "  {id:#06x}: re-encoded bytes differ from the original"
                                ),
                            });
                        }
                    }
                },
            }
        }
    }

    println!("round-tripped OK: {ok}   MISMATCH/ERROR: {fail}   (no registry entry fits, out of scope: {nodec})");
    for e in &examples {
        println!("{e}");
    }
    println!("per-opcode (ok/fail):");
    for (op, (a, b)) in by_op {
        println!("  {op:#06x}: {a} ok, {b} fail");
    }
    if fail > 0 {
        std::process::exit(1);
    }
}
