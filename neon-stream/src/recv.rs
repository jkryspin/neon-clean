//! Exact server->client packet decryption (found 2026-10-26 by reading the client's receive path, docs/network-protocol.md).
//!
//! * The client keeps a 32-bit receive IV. Every packet's header half `a` carries two of its bytes:
//!   `(iv[3] << 8 | iv[2]) == a ^ 0xfffe`, so every packet checks that the chain is still in step.
//! * The body is decrypted as `plain = cipher - iv[0]` for every byte of every 1456/1460-byte chunk (one constant per packet).
//! * After the packet the IV advances by the classic shuffle (`outgoing::shuffle_iv`).
//!
//! Mid-connection the IV is unknown, but `iv[2]`, `iv[3]` of the first frame are in its header, and `iv[0]`, `iv[1]` (65,536 candidates)
//! are fixed by requiring the next headers to follow the shuffle chain: 12 frames give a unique answer. Until then (and after any
//! lost bytes) the caller falls back to the per-block heuristic in `net.rs`.

use crate::outgoing::shuffle_iv;

/// Frames needed to pin the IV (each header checks 16 bits, so 12 leave no false candidates in practice).
pub const SYNC_FRAMES: usize = 12;

fn iv_hi(a: u16) -> u16 {
    a ^ 0xfffe
}

/// Does the header `a` belong to a packet encrypted with `iv`?
pub fn header_matches(a: u16, iv: &[u8; 4]) -> bool {
    iv_hi(a) == u16::from(iv[3]) << 8 | u16::from(iv[2])
}

/// Find the IV in force at the first of `headers` (the `a` halves of consecutive frames). `None` when no or several candidates fit.
pub fn find_iv(headers: &[u16]) -> Option<[u8; 4]> {
    if headers.len() < SYNC_FRAMES {
        return None;
    }
    let hi = iv_hi(headers[0]).to_le_bytes();
    let mut found: Option<[u8; 4]> = None;
    for lo in 0..=u16::MAX {
        let l = lo.to_le_bytes();
        let start = [l[0], l[1], hi[0], hi[1]];
        let mut iv = start;
        let mut ok = true;
        for &a in headers {
            if !header_matches(a, &iv) {
                ok = false;
                break;
            }
            shuffle_iv(&mut iv);
        }
        if ok {
            if found.is_some() {
                return None; // ambiguous: need more frames
            }
            found = Some(start);
        }
    }
    found
}

/// Where a connection's first ordinary frame starts, given the server hello at the start of its stream.
///
/// The hello is `13 00` + 19 bytes (receive IV at bytes 15..19 of the stream). After it comes one special frame (its 4 header bytes are
/// zero, its body runs to the next header) which uses up one shuffle; the second frame's header therefore satisfies
/// `a == ((iv1[3] << 8) | iv1[2]) ^ 0xfffe` with `iv1 = shuffle(receive IV)`. Returns that offset and `iv1`, once the stream holds enough bytes
/// to see it followed by a second consistent header. `None`: not a hello stream, or not enough bytes yet.
pub fn second_frame(stream: &[u8]) -> Option<(usize, [u8; 4])> {
    if stream.len() < 40 || stream[0] != 0x13 || stream[1] != 0 {
        return None;
    }
    let mut iv: [u8; 4] = stream[15..19].try_into().ok()?;
    shuffle_iv(&mut iv);
    let want = (u16::from(iv[3]) << 8 | u16::from(iv[2])) ^ 0xfffe;
    let mut iv2 = iv;
    shuffle_iv(&mut iv2);
    let want2 = (u16::from(iv2[3]) << 8 | u16::from(iv2[2])) ^ 0xfffe;
    for p in 27..stream.len().saturating_sub(8).min(9000) {
        let a = u16::from_le_bytes([stream[p], stream[p + 1]]);
        if a != want {
            continue;
        }
        let c = u16::from_le_bytes([stream[p + 2], stream[p + 3]]);
        let len = (a ^ c) as usize;
        if !(2..=8000).contains(&len) || p + 4 + len + 2 > stream.len() {
            continue;
        }
        let q = p + 4 + len;
        if u16::from_le_bytes([stream[q], stream[q + 1]]) == want2 {
            return Some((p, iv));
        }
    }
    None
}

/// Undo the per-packet constant on a whole body (chunking does not matter: the constant is the same in every chunk).
pub fn decrypt(body: &[u8], iv: &[u8; 4]) -> Vec<u8> {
    body.iter().map(|b| b.wrapping_sub(iv[0])).collect()
}

/// Per-connection receive state.
#[derive(Debug, Default)]
pub struct RecvCipher {
    iv: Option<[u8; 4]>,
    pending: Vec<u16>,
    since_try: usize,
    /// Packets decrypted exactly / with the heuristic (while unsynced), and times the chain broke.
    pub exact: u64,
    pub heuristic: u64,
    pub breaks: u64,
}

impl RecvCipher {
    /// A cipher whose IV is already known (the connection start was seen: see `second_frame`).
    pub fn at(iv: [u8; 4]) -> RecvCipher {
        RecvCipher { iv: Some(iv), ..Default::default() }
    }

    #[cfg(test)]
    pub fn is_synced(&self) -> bool {
        self.iv.is_some()
    }

    /// Feed one frame (header half `a`, ciphertext body). Returns the exact plaintext, or `None` when the IV is not known yet
    /// (the caller then uses the heuristic for this frame).
    pub fn frame(&mut self, a: u16, body: &[u8]) -> Option<Vec<u8>> {
        if let Some(mut iv) = self.iv {
            if header_matches(a, &iv) {
                let plain = decrypt(body, &iv);
                shuffle_iv(&mut iv);
                self.iv = Some(iv);
                self.exact += 1;
                return Some(plain);
            }
            // out of step (bytes were lost, or the connection changed): forget and learn again
            self.iv = None;
            self.pending.clear();
            self.breaks += 1;
        }
        self.heuristic += 1;
        self.pending.push(a);
        if self.pending.len() > SYNC_FRAMES {
            self.pending.remove(0);
        }
        self.since_try += 1;
        if self.pending.len() == SYNC_FRAMES && self.since_try >= 4 {
            self.since_try = 0;
            if let Some(mut iv) = find_iv(&self.pending) {
                for _ in 0..SYNC_FRAMES {
                    shuffle_iv(&mut iv);
                }
                self.iv = Some(iv); // the IV for the frame after the last one seen
                self.pending.clear();
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/fixtures/recv_headers.json")).unwrap()
    }

    fn headers() -> Vec<u16> {
        fixture()["headers"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u16).collect()
    }

    #[test]
    fn the_iv_is_found_from_twelve_real_headers_and_predicts_the_rest() {
        let h = headers();
        let ks: Vec<u8> = fixture()["k"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
        let mut iv = find_iv(&h[..SYNC_FRAMES]).expect("unique iv");
        assert_eq!(iv[0], ks[0], "the first constant is the iv's first byte");
        for (i, &a) in h.iter().enumerate() {
            assert!(header_matches(a, &iv), "header {i}");
            assert_eq!(iv[0], ks[i], "constant of frame {i}");
            shuffle_iv(&mut iv);
        }
    }

    #[test]
    fn too_few_or_wrong_headers_do_not_sync() {
        let h = headers();
        assert_eq!(find_iv(&h[..SYNC_FRAMES - 1]), None);
        let mut bad = h[..SYNC_FRAMES].to_vec();
        bad[5] ^= 0x0100;
        assert_eq!(find_iv(&bad), None, "a header off the chain leaves no candidate");
    }

    #[test]
    fn recv_cipher_syncs_midstream_then_decrypts_real_small_frames_exactly() {
        let f = fixture();
        let h = headers();
        let samples = f["samples"].as_array().unwrap();
        let mut rc = RecvCipher::default();
        // feed 12 frames the heuristic would have to handle, plus the next ones, as a client that joined mid-connection would see them
        for (i, &a) in h.iter().enumerate().take(SYNC_FRAMES) {
            let s = samples.iter().find(|s| s["index"].as_u64() == Some(i as u64));
            let body = s.map(|s| hex(s["body"].as_str().unwrap())).unwrap_or_else(|| vec![0, 0, 0]);
            assert_eq!(rc.frame(a, &body), None, "frame {i} arrives before the iv is known");
        }
        assert!(rc.is_synced());
        for (i, &a) in h.iter().enumerate().skip(SYNC_FRAMES) {
            if let Some(s) = samples.iter().find(|s| s["index"].as_u64() == Some(i as u64)) {
                assert_eq!(rc.frame(a, &hex(s["body"].as_str().unwrap())).unwrap(), hex(s["plain"].as_str().unwrap()), "frame {i}");
            } else {
                assert!(rc.frame(a, &[1, 2, 3]).is_some(), "frame {i} stays in step");
            }
        }
        assert_eq!(rc.breaks, 0);
    }

    #[test]
    fn a_missing_frame_breaks_the_chain_and_it_is_relearned() {
        let h = headers();
        let mut rc = RecvCipher::default();
        for &a in &h[..SYNC_FRAMES] {
            rc.frame(a, &[0, 0]);
        }
        assert!(rc.is_synced());
        assert!(rc.frame(h[SYNC_FRAMES + 3], &[0, 0]).is_none(), "skipping three frames: header off the chain");
        assert_eq!(rc.breaks, 1);
        assert!(!rc.is_synced());
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn garbage_headers_never_panic_the_receive_side() {
        let mut x: u64 = 0x2545f4914f6cdd1d;
        let mut rc = RecvCipher::default();
        for _ in 0..3000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let body: Vec<u8> = (0..(x % 40) as usize).map(|k| (x >> (k % 8)) as u8).collect();
            let _ = rc.frame((x >> 20) as u16, &body);
        }
        assert!(rc.exact + rc.heuristic > 0);
    }

    #[test]
    fn the_second_frame_of_a_real_stream_start_is_found_from_the_hello() {
        let f: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/stream_start.json")).unwrap();
        let hex = |s: &str| -> Vec<u8> { (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect() };
        let s = hex(f["hex"].as_str().unwrap());
        let (p, iv) = second_frame(&s).expect("found");
        assert_eq!(p as u64, f["second_frame_at"].as_u64().unwrap());
        assert!(header_matches(u16::from_le_bytes([s[p], s[p + 1]]), &iv), "its header matches the shuffled hello IV");
        assert_eq!(second_frame(&s[..100]), None, "too little data: no answer");
        assert_eq!(second_frame(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42]), None, "not a hello");
    }
}

