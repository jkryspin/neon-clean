//! Tier B (semantic encode) round-trip tests - see docs/decode-plan.md for why this tier matters.
//!
//! The property that makes an encoder trustworthy: patching a real captured packet with exactly the values
//! decoded out of it must reproduce that packet byte for byte. Any wrong offset, size, endianness or type in
//! the registry shows up here immediately, and it does so WITHOUT depending on whether the human-assigned
//! MEANING of a field is correct - which is exactly the kind of check this project's evidence rules want.

use neon_protocol::{Dir, Registry, Value};

fn plain(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn fixtures() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/plain_packets.json")).unwrap()
}

#[test]
fn encode_is_the_exact_inverse_of_decode_on_every_real_packet() {
    let r = Registry::builtin().unwrap();
    let mut checked = 0;
    for (key, v) in fixtures().as_object().unwrap() {
        let Some(hex) = v.get("plain").and_then(|p| p.as_str()) else {
            continue;
        };
        let bytes = plain(hex);
        let Some(d) = r.decode(Dir::In, &bytes) else {
            continue;
        };
        let out = r
            .encode_onto(Dir::In, &bytes, &d.fields)
            .unwrap_or_else(|e| panic!("{key}: encode_onto failed: {e}"));
        assert_eq!(
            out, bytes,
            "{key}: re-encoding the decoded fields changed the packet"
        );
        checked += 1;
    }
    assert!(
        checked >= 6,
        "expected to round-trip several real packets, only did {checked}"
    );
}

#[test]
fn a_changed_field_changes_exactly_those_bytes_and_decodes_back() {
    // The actual point of Tier B: take a real packet, change one field, get a valid packet back.
    let r = Registry::builtin().unwrap();
    let bytes = plain(fixtures()["0x29d"]["plain"].as_str().unwrap());
    let d = r.decode(Dir::In, &bytes).expect("0x029d must decode");
    let (fname, _) = d
        .fields
        .iter()
        .find(|(n, _)| n == "char_id")
        .expect("PlayerMove has char_id");

    let out = r
        .encode_onto(Dir::In, &bytes, &[(fname.clone(), Value::U32(1234567))])
        .unwrap();
    assert_eq!(
        out.len(),
        bytes.len(),
        "patching a fixed-width field must not resize the packet"
    );
    let differing = out.iter().zip(&bytes).filter(|(a, b)| a != b).count();
    assert!(differing <= 4, "changing one u32 touched {differing} bytes");

    let again = r
        .decode(Dir::In, &out)
        .expect("the patched packet must still decode");
    let got = again
        .fields
        .iter()
        .find(|(n, _)| n == "char_id")
        .map(|(_, v)| v.clone());
    assert_eq!(
        got,
        Some(Value::U32(1234567)),
        "the new value must read back"
    );
}

#[test]
fn encode_refuses_an_unknown_field_instead_of_silently_ignoring_it() {
    let r = Registry::builtin().unwrap();
    let bytes = plain(fixtures()["0x29d"]["plain"].as_str().unwrap());
    let err = r
        .encode_onto(Dir::In, &bytes, &[("not_a_field".into(), Value::U32(1))])
        .expect_err("an unknown field name must be an error, not a no-op");
    assert!(
        err.contains("not_a_field"),
        "the error should name the offending field, got: {err}"
    );
}

#[test]
fn encode_refuses_a_value_outside_the_registered_bounds() {
    // PlayerSpawn's level is registered min 0 max 250; writing 9999 must fail rather than produce a packet
    // the real client would never send.
    let r = Registry::builtin().unwrap();
    let bytes = plain(fixtures()["0x22d"]["plain"].as_str().unwrap());
    assert!(r.decode(Dir::In, &bytes).is_some());
    let err = r
        .encode_onto(Dir::In, &bytes, &[("level".into(), Value::U32(9999))])
        .expect_err("out-of-bounds value must be refused");
    assert!(
        err.contains("level"),
        "the error should name the field, got: {err}"
    );
}

#[test]
fn a_string_field_cannot_be_patched_to_a_different_length() {
    // Changing a str16's length would shift every later byte, including ones the registry does not understand,
    // so this must be refused rather than silently corrupting the packet.
    let r = Registry::builtin().unwrap();
    let bytes = plain(fixtures()["0x22d"]["plain"].as_str().unwrap());
    let err = r
        .encode_onto(
            Dir::In,
            &bytes,
            &[("name".into(), Value::Str("a_much_longer_name".into()))],
        )
        .expect_err("a different-length string must be refused");
    assert!(err.contains("same length"), "got: {err}");

    // ...but a same-length replacement is fine.
    let same = r
        .encode_onto(
            Dir::In,
            &bytes,
            &[("name".into(), Value::Str("bob55".into()))],
        )
        .unwrap();
    let back = r.decode(Dir::In, &same).unwrap();
    let got = back
        .fields
        .iter()
        .find(|(n, _)| n == "name")
        .map(|(_, v)| v.clone());
    assert_eq!(got, Some(Value::Str("bob55".into())));
}
