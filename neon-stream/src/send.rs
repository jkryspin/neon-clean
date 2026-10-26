//! Exact client->server decryption (found 2026-10-26 from the client's own code, docs/network-protocol.md).
//!
//! * Framing: 4-byte header of two big-endian u16 `a`, `c`; the body length is `a ^ c` with its two bytes swapped.
//! * `a = ((iv[2] << 8) | iv[3]) ^ 0xdf00`, so every header checks the send IV chain; the IV advances by the same shuffle
//!   as the receive side after every packet.
//! * The body is AES-256-OFB: the 4-byte IV repeated four times is the first block; every 1456/1460-byte chunk starts again from
//!   that same IV. The key is the 8 words at stride 16 of the client's static table (each word is one byte, 0..255).
//!   The result is what the server receives; it is not the client's internal packet object (opcodes differ).

use crate::aes::Aes256;
use crate::outgoing::shuffle_iv;

/// The client's static AES-256 key (words read at stride 16 from the table at `0x143bb4e80`, Classic World v1.0.1).
pub const KEY: [u8; 32] = [
    0x0f, 0, 0, 0, 0x1b, 0, 0, 0, 0xc5, 0, 0, 0, 0x46, 0, 0, 0, 0xf3, 0, 0, 0, 0xbe, 0, 0, 0, 0xff, 0, 0, 0, 0x75, 0, 0, 0,
];

pub const SYNC_FRAMES: usize = 12;
const MAX_BODY: usize = 4000;

fn len_of(h: &[u8]) -> (u16, usize) {
    let a = u16::from_be_bytes([h[0], h[1]]);
    let c = u16::from_be_bytes([h[2], h[3]]);
    (a, (a ^ c).swap_bytes() as usize)
}

pub fn header_matches(a: u16, iv: &[u8; 4]) -> bool {
    a ^ 0xdf00 == u16::from(iv[2]) << 8 | u16::from(iv[3])
}

pub fn find_iv(headers: &[u16]) -> Option<[u8; 4]> {
    if headers.len() < SYNC_FRAMES {
        return None;
    }
    let hi = (headers[0] ^ 0xdf00).to_be_bytes();
    let mut found = None;
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
                return None;
            }
            found = Some(start);
        }
    }
    found
}

pub fn decrypt(aes: &Aes256, body: &[u8], iv: &[u8; 4]) -> Vec<u8> {
    let block = [iv[0], iv[1], iv[2], iv[3], iv[0], iv[1], iv[2], iv[3], iv[0], iv[1], iv[2], iv[3], iv[0], iv[1], iv[2], iv[3]];
    let mut out = Vec::with_capacity(body.len());
    let mut pos = 0;
    let mut first = true;
    while pos < body.len() {
        let n = (body.len() - pos).min(if first { 1456 } else { 1460 });
        out.extend(aes.ofb(block, &body[pos..pos + n]));
        pos += n;
        first = false;
    }
    out
}

/// Splits the client->server byte stream into (header `a`, body) frames, finding its own alignment mid-stream.
#[derive(Default)]
pub struct SendFramer {
    buf: Vec<u8>,
    synced: bool,
}

impl SendFramer {
    pub fn reset(&mut self) {
        self.buf.clear();
        self.synced = false;
    }

    fn chain(&self, start: usize, limit: usize) -> usize {
        let (mut i, mut n) = (start, 0);
        while n < limit && i + 4 <= self.buf.len() {
            let (_, l) = len_of(&self.buf[i..i + 4]);
            if (2..=MAX_BODY).contains(&l) && i + 4 + l <= self.buf.len() {
                i += 4 + l;
                n += 1;
            } else {
                break;
            }
        }
        n
    }

    pub fn push(&mut self, data: &[u8]) -> Vec<(u16, Vec<u8>)> {
        self.buf.extend_from_slice(data);
        let mut out = vec![];
        loop {
            if !self.synced {
                if self.buf.len() > 256 * 1024 {
                    let cut = self.buf.len() - 64 * 1024;
                    self.buf.drain(..cut);
                }
                let window = self.buf.len().min(3000);
                let mut best = (0usize, 0usize);
                for s in 0..window {
                    let n = self.chain(s, 40);
                    if n > best.0 {
                        best = (n, s);
                        if n >= 40 {
                            break;
                        }
                    }
                }
                if best.0 < 6 {
                    return out;
                }
                self.buf.drain(..best.1);
                self.synced = true;
            }
            if self.buf.len() < 4 {
                return out;
            }
            let (a, l) = len_of(&self.buf[..4]);
            if !(2..=MAX_BODY).contains(&l) {
                self.synced = false;
                self.buf.drain(..1);
                continue;
            }
            if self.buf.len() < 4 + l {
                return out;
            }
            out.push((a, self.buf[4..4 + l].to_vec()));
            self.buf.drain(..4 + l);
        }
    }
}

/// Per-connection send state; mirrors `recv::RecvCipher`, except that frames seen before the IV is known are decrypted afterwards.
pub struct SendCipher {
    aes: Aes256,
    iv: Option<[u8; 4]>,
    pending: Vec<(u16, Vec<u8>)>,
    since_try: usize,
    pub exact: u64,
    pub breaks: u64,
}

impl Default for SendCipher {
    fn default() -> Self {
        SendCipher { aes: Aes256::new(&KEY), iv: None, pending: vec![], since_try: 0, exact: 0, breaks: 0 }
    }
}

impl SendCipher {
    pub fn is_synced(&self) -> bool {
        self.iv.is_some()
    }

    /// Feed one frame; returns the plaintexts that became known (none while unsynced, the whole backlog when the IV is found).
    pub fn frame(&mut self, a: u16, body: &[u8]) -> Vec<Vec<u8>> {
        if let Some(mut iv) = self.iv {
            if header_matches(a, &iv) {
                let plain = decrypt(&self.aes, body, &iv);
                shuffle_iv(&mut iv);
                self.iv = Some(iv);
                self.exact += 1;
                return vec![plain];
            }
            self.iv = None;
            self.pending.clear();
            self.breaks += 1;
        }
        self.pending.push((a, body.to_vec()));
        if self.pending.len() > SYNC_FRAMES {
            self.pending.remove(0);
        }
        self.since_try += 1;
        if self.pending.len() == SYNC_FRAMES && self.since_try >= 4 {
            self.since_try = 0;
            let headers: Vec<u16> = self.pending.iter().map(|(a, _)| *a).collect();
            if let Some(mut iv) = find_iv(&headers) {
                let mut out = Vec::with_capacity(SYNC_FRAMES);
                for (_, b) in std::mem::take(&mut self.pending) {
                    out.push(decrypt(&self.aes, &b, &iv));
                    shuffle_iv(&mut iv);
                    self.exact += 1;
                }
                self.iv = Some(iv);
                return out;
            }
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/fixtures/send_headers.json")).unwrap()
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    fn headers() -> Vec<u16> {
        fixture()["headers"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u16).collect()
    }

    #[test]
    fn the_send_iv_is_found_from_twelve_real_headers_and_follows_the_chain() {
        let h = headers();
        let want: [u8; 4] = hex(fixture()["iv0"].as_str().unwrap()).try_into().unwrap();
        let mut iv = find_iv(&h[..SYNC_FRAMES]).expect("unique");
        assert_eq!(iv, want);
        for (i, &a) in h.iter().enumerate() {
            assert!(header_matches(a, &iv), "frame {i}");
            shuffle_iv(&mut iv);
        }
        assert_eq!(find_iv(&h[..SYNC_FRAMES - 1]), None);
    }

    #[test]
    fn real_small_frames_decrypt_to_the_known_plaintext() {
        // Verified independently: the twelve 2-byte keepalive-sized frames of the connection (twelve different IVs) all decrypt to the same plaintext 86 87
        let aes = Aes256::new(&KEY);
        for s in fixture()["samples"].as_array().unwrap() {
            let iv: [u8; 4] = hex(s["iv"].as_str().unwrap()).try_into().unwrap();
            assert_eq!(decrypt(&aes, &hex(s["body"].as_str().unwrap()), &iv), hex(s["plain"].as_str().unwrap()));
        }
    }

    #[test]
    fn a_stream_is_framed_and_decrypted_with_backlog_released_on_sync() {
        let aes = Aes256::new(&KEY);
        let h = headers();
        let mut iv: [u8; 4] = hex(fixture()["iv0"].as_str().unwrap()).try_into().unwrap();
        // build a synthetic client stream of 20 frames with known plaintext using the real header halves and IV chain
        let mut stream = vec![];
        let mut plains = vec![];
        for (i, &a) in h.iter().enumerate().take(20) {
            let plain: Vec<u8> = (0..(2 + i % 7)).map(|k| (i * 7 + k) as u8).collect();
            let ct = decrypt(&aes, &plain, &iv); // OFB is its own inverse
            let c = (((plain.len() as u16).swap_bytes()) ^ a) as u16;
            stream.extend(a.to_be_bytes());
            stream.extend(c.to_be_bytes());
            stream.extend(&ct);
            plains.push(plain);
            shuffle_iv(&mut iv);
        }
        let mut fr = SendFramer::default();
        let mut sc = SendCipher::default();
        let mut got = vec![];
        for chunk in stream.chunks(37) {
            for (a, body) in fr.push(chunk) {
                got.extend(sc.frame(a, &body));
            }
        }
        assert!(sc.is_synced());
        assert_eq!(sc.breaks, 0);
        assert_eq!(got.len(), 20, "everything, including the first twelve, comes out once");
        assert_eq!(got, plains);
    }

    #[test]
    fn garbage_and_truncated_streams_never_panic_the_send_side() {
        // xorshift bytes in odd-sized chunks, then a real-looking stream cut short in every possible place
        let mut x: u64 = 0x9e3779b97f4a7c15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let mut fr = SendFramer::default();
        let mut sc = SendCipher::default();
        for _ in 0..400 {
            let n = (next() % 700) as usize;
            let chunk: Vec<u8> = (0..n).map(|_| next() as u8).collect();
            for (a, body) in fr.push(&chunk) {
                let _ = sc.frame(a, &body);
            }
        }
        let aes = Aes256::new(&KEY);
        let mut iv: [u8; 4] = hex(fixture()["iv0"].as_str().unwrap()).try_into().unwrap();
        let mut stream = vec![];
        for (i, &a) in headers().iter().enumerate().take(30) {
            let plain: Vec<u8> = (0..(2 + i % 9)).map(|k| (i + k) as u8).collect();
            let ct = decrypt(&aes, &plain, &iv);
            let c = ((plain.len() as u16).swap_bytes()) ^ a;
            stream.extend(a.to_be_bytes());
            stream.extend(c.to_be_bytes());
            stream.extend(&ct);
            shuffle_iv(&mut iv);
        }
        for cut in 0..stream.len() {
            let mut fr = SendFramer::default();
            let mut sc = SendCipher::default();
            for (a, body) in fr.push(&stream[..cut]) {
                let _ = sc.frame(a, &body);
            }
        }
    }
}

