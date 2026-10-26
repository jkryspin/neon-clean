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

/// Every real packet we keep as a fixture (decoded plaintext), from all fixture files.
fn all_fixture_packets() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for src in [include_str!("fixtures/plain_packets.json"), include_str!("fixtures/combat_packets.json"), include_str!("fixtures/party_packets.json")] {
        let v: serde_json::Value = serde_json::from_str(src).unwrap();
        for e in v.as_object().unwrap().values() {
            let hex = e.get("plain").and_then(|p| p.as_str()).or_else(|| e.as_str()).unwrap();
            out.push(plain(hex));
        }
    }
    out
}

#[test]
fn evidence_rule_every_verified_variant_decodes_a_real_packet() {
    // A verified entry must decode at least one real captured packet *as that entry* (name included): sharing an opcode
    // with a fixture is not enough when an opcode has several variants.
    let r = Registry::builtin().unwrap();
    let packets = all_fixture_packets();
    let mut missing = Vec::new();
    for o in r.iter().filter(|o| o.dir == Dir::In && o.status == Status::Verified) {
        let ok = packets.iter().any(|p| r.decode(Dir::In, p).map_or(false, |d| d.id == o.id && d.name == o.name));
        if !ok {
            missing.push(format!("{:#06x} {}", o.id, o.name.as_deref().unwrap_or("?")));
        }
    }
    assert!(missing.is_empty(), "verified without a real packet fixture: {missing:?}");
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

#[test]
fn real_combat_packets_decode_with_the_registered_layouts() {
    let r = Registry::builtin().unwrap();
    let c: serde_json::Value = serde_json::from_str(include_str!("fixtures/combat_packets.json")).unwrap();
    let get = |k: &str| plain(c[k]["plain"].as_str().unwrap());
    let spawn = r.decode(Dir::In, &get("0x3d5")).expect("0x03d5 decodes");
    assert_eq!(spawn.name.as_deref(), Some("MobRespawn"));
    assert!(spawn.get("template").unwrap().as_i64().unwrap() >= 2);
    // MobSpawn and MobRespawn share one layout: the same bytes under 0x03c9 give the same fields.
    let mut as_spawn = get("0x3d5");
    as_spawn[0..2].copy_from_slice(&0x03c9u16.to_le_bytes());
    let old = r.decode(Dir::In, &as_spawn).unwrap();
    for name in ["oid", "template", "x", "y"] {
        assert_eq!(old.get(name), spawn.get(name), "{name}");
    }
    // the respawn also carries our own character id (334426, known independently from drop ownership)
    assert_eq!(spawn.get("controller").unwrap().as_i64(), Some(334426));
    assert!(old.get("controller").is_none());
    let dead = r.decode(Dir::In, &get("0x3f3_dead")).unwrap();
    let live = r.decode(Dir::In, &get("0x3f3_spawn")).unwrap();
    assert_eq!(dead.get("state").unwrap().as_i64(), Some(0));
    assert_eq!(live.get("state").unwrap().as_i64(), Some(15));
    assert!(r.decode(Dir::In, &get("0xd2")).is_some());
}

#[test]
fn a_real_player_attack_broadcast_decodes() {
    let r = Registry::builtin().unwrap();
    let c: serde_json::Value = serde_json::from_str(include_str!("fixtures/combat_packets.json")).unwrap();
    let p = plain(c["0x2a8"]["plain"].as_str().unwrap());
    let d = r.decode(Dir::In, &p).expect("0x02a8 decodes");
    assert_eq!(d.name.as_deref(), Some("PlayerAttack"));
    let pl = d.get("char_id").unwrap().as_i64().unwrap();
    assert!((100_000..10_000_000).contains(&pl), "player id {pl}");
    // Length is 63 + 24n; the first target record starts at @63 with the mob oid and (probably the damage) at +8.
    assert_eq!((p.len() - 63) % 24, 0, "length {} is not 63 + 24n", p.len());
    assert!(p.len() >= 87);
    let mob = u32::from_le_bytes(p[63..67].try_into().unwrap());
    let dmg = u32::from_le_bytes(p[71..75].try_into().unwrap());
    assert!((10_000_000..100_000_000).contains(&mob), "mob oid {mob}");
    assert!((1..10_000).contains(&dmg), "value {dmg}");
    // the attacker's position is stored twice and both pairs are plausible map coordinates
    let (ax, ay, bx, by) = (d.get("x").unwrap().as_i64().unwrap(), d.get("y").unwrap().as_i64().unwrap(), d.get("x2").unwrap().as_i64().unwrap(), d.get("y2").unwrap().as_i64().unwrap());
    assert!((ax - bx).abs() < 400 && (ay - by).abs() < 400, "the two position pairs agree: ({ax},{ay}) vs ({bx},{by})");
    // a swing at nothing is 63 bytes and still decodes
    assert!(r.decode(Dir::In, &p[..63]).is_some());
    assert!(r.decode(Dir::In, &p[..62]).is_none());
}

#[test]
fn variants_are_selected_by_mode_byte_and_length() {
    let r = Registry::builtin().unwrap();
    let c: serde_json::Value = serde_json::from_str(include_str!("fixtures/combat_packets.json")).unwrap();
    let exp = r.decode(Dir::In, &plain(c["0x8b_exp"]["plain"].as_str().unwrap())).unwrap();
    assert_eq!(exp.name.as_deref(), Some("ExpMessage"));
    assert_eq!(exp.get("amount").unwrap().as_i64(), c["0x8b_exp"]["expect"]["amount"].as_i64());
    let rec = r.decode(Dir::In, &plain(c["0x8b_count"]["plain"].as_str().unwrap())).unwrap();
    assert_eq!(rec.name.as_deref(), Some("QuestRecordMessage"));
    assert!(matches!(rec.get("text"), Some(Value::Str(t)) if t.starts_with("date=")));
    // the same opcode with another mode byte is a different packet; an unknown mode does not decode
    let mut other = plain(c["0x8b_exp"]["plain"].as_str().unwrap());
    other[2] = 0x7f;
    assert!(r.decode(Dir::In, &other).is_none());
    // party: three real packets, three variants
    let party: serde_json::Value = serde_json::from_str(include_str!("fixtures/party_packets.json")).unwrap();
    let name = |k: &str| r.decode(Dir::In, &plain(party[k].as_str().unwrap())).and_then(|d| d.name);
    assert_eq!(name("create586").as_deref(), Some("PartyCreated"));
    assert_eq!(name("join1267").as_deref(), Some("PartyJoined"));
    assert_eq!(name("leave8").as_deref(), Some("PartyLeft"));
    let left = r.decode(Dir::In, &plain(party["leave8"].as_str().unwrap())).unwrap();
    assert_eq!(left.get("char_id").unwrap().as_i64(), Some(334426), "the id that left is our own, known independently from drop ownership");
    // 0x01a9: two packets, told apart by length
    let mut set_field = vec![0u8; 60];
    set_field[0] = 0xa9;
    set_field[1] = 0x01;
    set_field[36..40].copy_from_slice(&1010u32.to_le_bytes());
    assert_eq!(r.decode(Dir::In, &set_field).unwrap().name.as_deref(), Some("MapEnter"));
    let mut inv = set_field.clone();
    inv.resize(1500, 0);
    assert_eq!(r.decode(Dir::In, &inv).unwrap().name.as_deref(), Some("LoginInventory"));
}

#[test]
fn records_expand_and_layout_reports_every_explained_byte() {
    let r = Registry::builtin().unwrap();
    let c: serde_json::Value = serde_json::from_str(include_str!("fixtures/combat_packets.json")).unwrap();
    let p87 = plain(c["0x2a8"]["plain"].as_str().unwrap());
    assert_eq!(p87.len(), 87);
    let d = r.decode(Dir::In, &p87).unwrap();
    assert!(d.get("target[0].mob_oid").is_some() && d.get("target[0].value").is_some() && d.get("target[1].mob_oid").is_none());
    // the same packet with a second target record appended: 111 bytes, two records
    let mut p111 = p87.clone();
    p111.extend_from_slice(&p87[63..87]);
    let d2 = r.decode(Dir::In, &p111).unwrap();
    assert_eq!(d2.get("target[0].mob_oid"), d2.get("target[1].mob_oid"));
    // a tail that is not a whole number of records does not match any variant
    let mut p97 = p87.clone();
    p97.extend_from_slice(&[0; 10]);
    assert!(r.decode(Dir::In, &p97).is_none());
    // layout: opcode (2) + char_id (4) + attacker_trait (1) + two position pairs (4 x 4) + record fields (4 + 4 each)
    let spans = r.layout(Dir::In, &p111);
    let bytes: usize = spans.iter().map(|s| s.size).sum();
    assert_eq!(bytes, 2 + 4 + 1 + 16 + 2 * (4 + 4));
    assert!(spans.iter().any(|s| s.name == "target[1].value" && s.off == 63 + 24 + 8));
    // a mode byte counts as explained too
    let exp = plain(c["0x8b_exp"]["plain"].as_str().unwrap());
    assert!(r.layout(Dir::In, &exp).iter().any(|s| s.name == "mode" && s.off == 2));
}

#[test]
fn validation_covers_variants_and_records() {
    let bad = r#"
[[opcode]]
id = 5
dir = "in"
when = { off = 2, eq = 1 }
status = "structure"
len_min = 4

[[opcode]]
id = 5
dir = "in"
when = { off = 2, eq = 1 }
status = "structure"
len_min = 6

[[opcode]]
id = 6
dir = "in"
when = { off = 9, eq = 1 }
status = "structure"
len_min = 4

[[opcode]]
id = 7
dir = "in"
status = "structure"
[opcode.records]
name = "r"
start = 1
stride = 4
field = [{ name = "a", ty = "u32", off = 2 }]
"#;
    let e = Registry::parse(bad).unwrap_err().to_string();
    for needle in ["duplicate In 0x0005", "when byte 9 is beyond len_min", "overlaps the opcode", "ends beyond the stride"] {
        assert!(e.contains(needle), "missing {needle:?} in:\n{e}");
    }
    // the same opcode with disjoint lengths is a legitimate split
    let ok = r#"
[[opcode]]
id = 5
dir = "in"
status = "structure"
len_min = 2
len_max = 10

[[opcode]]
id = 5
dir = "in"
status = "structure"
len_min = 11
"#;
    assert_eq!(Registry::parse(ok).unwrap().variants(Dir::In, 5).len(), 2);
}

#[test]
fn real_movement_packets_decode_into_fragments_and_a_wrong_count_falls_back() {
    let r = Registry::builtin().unwrap();
    let f = fixtures();
    let pm = plain(f["0x29d"]["plain"].as_str().unwrap()); // a real PlayerMove, 41 bytes = 20 + 21*1
    let d = r.decode(Dir::In, &pm).unwrap();
    assert_eq!(d.get("count").unwrap().as_i64(), Some(1));
    let (sx, sy) = (d.get("x").unwrap().as_i64().unwrap(), d.get("y").unwrap().as_i64().unwrap());
    assert_eq!((sx, sy), (-185, 274), "start position: independently recorded with the fixture");
    let ex = d.get("move[0].x").unwrap().as_i64().unwrap();
    let ey = d.get("move[0].y").unwrap().as_i64().unwrap();
    assert!((-20000..20000).contains(&ex) && (ey - sy).abs() < 400, "end position ({ex},{ey}) is near the start y {sy}");
    assert_eq!(d.get("move[0].stance").map(|v| v.as_i64().unwrap() >= 0), Some(true));
    // a real MobMove, 83 bytes = 41 + 21*2: two fragments
    let mm = plain(f["0x3dc"]["plain"].as_str().unwrap());
    assert_eq!(mm.len(), 83);
    let d = r.decode(Dir::In, &mm).unwrap();
    assert_eq!(d.get("count").unwrap().as_i64(), Some(2));
    assert!(d.get("move[0].x").is_some() && d.get("move[1].x").is_some() && d.get("move[2].x").is_none());
    assert_eq!(d.get("x").unwrap().as_i64(), Some(698), "start position recorded with the fixture");
    // the count byte is a checksum: change it and the fragment layout no longer fits, the start-only variant still decodes
    let mut bad = mm.clone();
    bad[38] = 3;
    let d = r.decode(Dir::In, &bad).unwrap();
    assert!(d.get("move[0].x").is_none() && d.get("x").unwrap().as_i64() == Some(698));
    // layout: every fragment byte that is a field is explained, the unexplained ones remain visible
    let spans = r.layout(Dir::In, &mm);
    assert!(spans.iter().any(|s| s.name == "move[1].duration" && s.off == 39 + 21 + 19));
    let explained: usize = spans.iter().map(|s| s.size).sum();
    assert!(explained < mm.len(), "padding and unknown bytes are not claimed");
}

#[test]
fn a_real_pet_move_decodes_into_fragments() {
    let r = Registry::builtin().unwrap();
    let c: serde_json::Value = serde_json::from_str(include_str!("fixtures/combat_packets.json")).unwrap();
    let p = plain(c["0x282_pet2"]["plain"].as_str().unwrap());
    assert_eq!(p.len(), 24 + 21 * 2);
    let d = r.decode(Dir::In, &p).unwrap();
    assert_eq!(d.name.as_deref(), Some("PetMove"));
    assert_eq!(d.get("count").unwrap().as_i64(), Some(2));
    assert!(d.get("move[1].x").is_some() && d.get("move[2].x").is_none());
    let owner = d.get("char_id").unwrap().as_i64().unwrap();
    assert!((100_000..10_000_000).contains(&owner));
}

#[test]
fn real_outgoing_packets_decode_and_the_movement_fragments_fit() {
    let r = Registry::builtin().unwrap();
    let o: serde_json::Value = serde_json::from_str(include_str!("fixtures/out_packets.json")).unwrap();
    let get = |k: &str| plain(o[k]["plain"].as_str().unwrap());
    for (k, n) in [("0xdb_n1", 1), ("0xdb_n2", 2), ("0xdb_n4", 4)] {
        let p = get(k);
        assert_eq!(p.len(), 26 + 21 * n + 10, "{k}");
        let d = r.decode(Dir::Out, &p).unwrap();
        assert_eq!(d.name.as_deref(), Some("MoveSelf"));
        assert_eq!(d.get("count").unwrap().as_i64(), Some(n as i64));
        // our own position: the walk happened on Amherst's ground (y = 274) or in a jump above it
        let y = d.get("y").unwrap().as_i64().unwrap();
        assert!((150..=274).contains(&y), "{k}: y {y}");
        assert!((0..2000).contains(&d.get("x").unwrap().as_i64().unwrap()));
    }
    // the layout matched the 21-byte fragments (no field of the records is named, but the layout covers the whole packet)
    let spans = r.layout(Dir::Out, &get("0xdb_n2"));
    assert!(spans.iter().any(|s| s.name == "count"));
    // a packet with a jump in it has fragments of another size: the fixed layout does not fit, the header-only variant does
    let m = get("0xdb_mixed");
    assert_eq!(m.len(), 150);
    let d = r.decode(Dir::Out, &m).unwrap();
    assert_eq!(d.get("count").unwrap().as_i64(), Some(6));
    // outgoing attack
    let a = r.decode(Dir::Out, &get("0xe1_attack")).unwrap();
    assert_eq!(a.name.as_deref(), Some("Attack"));
    // direction matters: 0x00db is not an incoming opcode we describe
    assert!(r.decode(Dir::In, &get("0xdb_n2")).is_none());
}

#[test]
fn real_party_requests_decode_by_length_and_the_ui_action_carries_the_clicked_button() {
    let r = Registry::builtin().unwrap();
    let o: serde_json::Value = serde_json::from_str(include_str!("fixtures/out_packets.json")).unwrap();
    let get = |k: &str| plain(o[k]["plain"].as_str().unwrap());
    let name = |k: &str| r.decode(Dir::Out, &get(k)).and_then(|d| d.name);
    assert_eq!(name("0x184_create").as_deref(), Some("PartyCreateRequest"));
    assert_eq!(name("0x184_leave").as_deref(), Some("PartyLeaveRequest"));
    let create = r.decode(Dir::Out, &get("0x228_create")).unwrap();
    let leave = r.decode(Dir::Out, &get("0x228_leave")).unwrap();
    assert_eq!(create.get("action").unwrap().as_i64(), Some(0x0282));
    assert_eq!(leave.get("action").unwrap().as_i64(), Some(0x02ac));
    // both ticks come from the same millisecond clock as MoveSelf (values of the same order)
    let t = create.get("tick").unwrap().as_i64().unwrap();
    let mv = r.decode(Dir::Out, &get("0xdb_n2")).unwrap().get("tick").unwrap().as_i64().unwrap();
    assert!((t - mv).abs() < 3_000_000_000, "{t} vs {mv}");
}
