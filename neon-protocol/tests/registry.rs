use neon_protocol::{Dir, Registry, Status, Value};

fn plain(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn fixtures() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/plain_packets.json")).unwrap()
}

#[test]
fn builtin_registry_parses_and_validates() {
    let r = Registry::builtin().unwrap();
    assert!(r.len() > 100);
    assert!(r.count(Dir::In, Status::Verified) >= 8);
}

#[test]
fn real_packets_decode_to_the_independently_recorded_values() {
    let r = Registry::builtin().unwrap();
    let f = fixtures();
    let mut checked = 0;
    for (key, v) in f.as_object().unwrap() {
        let Some(expect) = v.get("expect").and_then(|e| e.as_object()) else { continue };
        if key.starts_with("0x7e") {
            continue; // value position depends on the mask: covered by stat_update_mask_is_read_from_both_real_packets
        }
        let p = plain(v["plain"].as_str().unwrap());
        let d = r.decode(Dir::In, &p).unwrap_or_else(|| panic!("{key} did not decode"));
        for (name, want) in expect {
            let got = d.get(name).unwrap_or_else(|| panic!("{key}: no field {name}"));
            match (got, want) {
                (Value::Str(s), serde_json::Value::String(w)) => assert_eq!(s, w, "{key}.{name}"),
                (g, w) => assert_eq!(g.as_i64(), w.as_i64(), "{key}.{name}"),
            }
            checked += 1;
        }
    }
    assert!(checked >= 15, "only {checked} field values checked");
}

#[test]
fn stat_update_mask_is_read_from_both_real_packets() {
    let r = Registry::builtin().unwrap();
    let f = fixtures();
    for (key, mask) in [("0x7e_hp", 1024), ("0x7e_exp", 65536)] {
        let p = plain(f[key]["plain"].as_str().unwrap());
        let d = r.decode(Dir::In, &p).unwrap();
        assert_eq!(d.name.as_deref(), Some("StatUpdate"));
        assert_eq!(d.get("mask").unwrap().as_i64(), Some(mask), "{key}");
        // With a single stat bit set, its value (low 4 bytes) starts at @9.
        let value = u32::from_le_bytes(p[9..13].try_into().unwrap()) as i64;
        assert_eq!(Some(value), f[key]["expect"]["value"].as_i64(), "{key} value");
    }
}

#[test]
fn size_bounds_and_truncation_are_rejected() {
    let r = Registry::builtin().unwrap();
    let f = fixtures();
    let p = plain(f["0x3d4"]["plain"].as_str().unwrap());
    assert!(r.decode(Dir::In, &p).is_some());
    assert!(r.decode(Dir::In, &p[..5]).is_none(), "shorter than len_min");
    // 0x0028 is registered as exactly 2 bytes (structure only: no fields, so it decodes to an empty field list).
    assert!(r.decode(Dir::In, &[0x28, 0x00]).is_some());
    assert!(r.decode(Dir::In, &[0x28, 0x00, 0x00]).is_none(), "longer than len_max");
    assert!(r.decode(Dir::In, &[0xff]).is_none());
    assert!(r.decode(Dir::In, &[0xee, 0xee, 0, 0]).is_none(), "unknown opcode");
}

#[test]
fn direction_matters() {
    let r = Registry::builtin().unwrap();
    // 0x029d is player move *incoming*; it is not an outgoing opcode we know.
    assert!(r.get(Dir::In, 0x029d).is_some());
    assert!(r.get(Dir::Out, 0x029d).is_none());
}

#[test]
fn validation_catches_bad_entries() {
    let bad = r#"
[[opcode]]
id = 1
dir = "in"
status = "verified"

[[opcode]]
id = 2
dir = "in"
name = "X"
status = "observed"

[[opcode]]
id = 3
dir = "in"
status = "observed"
len_min = 5
len_max = 2

[[opcode]]
id = 3
dir = "in"
status = "observed"

[[opcode]]
id = 4
dir = "in"
name = "Y"
status = "structure"
len_min = 6
field = [{ name = "a", ty = "u32", off = 4 }]
"#;
    let e = Registry::parse(bad).unwrap_err().to_string();
    for needle in ["need a name", "at least one field", "must not carry a name", "len_min 5 > len_max 2", "duplicate", "beyond len_min"] {
        assert!(e.contains(needle), "missing {needle:?} in:\n{e}");
    }
}

#[test]
fn evidence_rule_every_verified_opcode_with_a_fixture_decodes() {
    // Every verified incoming opcode that has a real packet in the fixtures must decode from it.
    let r = Registry::builtin().unwrap();
    let f = fixtures();
    let mut have = std::collections::HashSet::new();
    for v in f.as_object().unwrap().values() {
        let p = plain(v["plain"].as_str().unwrap());
        have.insert(u16::from_le_bytes([p[0], p[1]]));
    }
    let untested: Vec<_> = r
        .iter()
        .filter(|o| o.dir == Dir::In && o.status == Status::Verified && !have.contains(&o.id))
        .map(|o| format!("{:#06x}", o.id))
        .collect();
    assert!(untested.is_empty(), "verified without a real packet fixture: {untested:?}");
}

#[test]
fn noise_from_a_misestimated_constant_is_rejected_like_the_live_decoder_does() {
    let r = Registry::builtin().unwrap();
    let fp: serde_json::Value = serde_json::from_str(include_str!("fixtures/false_positives.json")).unwrap();
    let bad = plain(fp["022d_bad_name_len"]["plain"].as_str().unwrap());
    assert!(r.decode(Dir::In, &bad).is_none(), "garbage name must not decode");
    // And the same real packet shape with a valid name does decode.
    let f = fixtures();
    assert!(r.decode(Dir::In, &plain(f["0x22d"]["plain"].as_str().unwrap())).is_some());
    // Coordinates out of range are rejected (player move with x = 30000).
    let mut mv = plain(f["0x29d"]["plain"].as_str().unwrap());
    mv[10..12].copy_from_slice(&30000i16.to_le_bytes());
    assert!(r.decode(Dir::In, &mv).is_none());
}

#[test]
fn constraint_validation() {
    let bad = r#"
[[opcode]]
id = 9
dir = "in"
status = "structure"
field = [{ name = "a", ty = "u32", off = 2, min = 5, max = 1 }, { name = "b", ty = "u32", off = 6, charset = "ident" }]
"#;
    let e = Registry::parse(bad).unwrap_err().to_string();
    assert!(e.contains("min 5 > max 1") && e.contains("charset but is not str16"), "{e}");
}
