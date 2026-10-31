//! Minimal AES-256 (encrypt direction only), enough for the OFB keystream of the client->server cipher.
//! Written out here so the build stays offline; checked against the FIPS-197 appendix C.3 vector.

fn xtime(x: u8) -> u8 {
    (x << 1) ^ if x & 0x80 != 0 { 0x1b } else { 0 }
}

fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0;
    while b != 0 {
        if b & 1 != 0 {
            p ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    p
}

/// The AES S-box, computed (multiplicative inverse in GF(2^8), then the affine map).
fn sbox() -> [u8; 256] {
    let mut s = [0u8; 256];
    for i in 0..256usize {
        let inv = if i == 0 {
            0
        } else {
            (1..=255u8).find(|&c| gmul(i as u8, c) == 1).unwrap()
        };
        let b = inv;
        s[i] = b ^ b.rotate_left(1) ^ b.rotate_left(2) ^ b.rotate_left(3) ^ b.rotate_left(4) ^ 0x63;
    }
    s
}

pub struct Aes256 {
    sbox: [u8; 256],
    round_keys: [[u8; 16]; 15],
}

impl Aes256 {
    pub fn new(key: &[u8; 32]) -> Aes256 {
        let sbox = sbox();
        // key expansion into 60 words
        let mut w = [[0u8; 4]; 60];
        for i in 0..8 {
            w[i].copy_from_slice(&key[4 * i..4 * i + 4]);
        }
        let mut rcon = 1u8;
        for i in 8..60 {
            let mut t = w[i - 1];
            if i % 8 == 0 {
                t = [
                    sbox[t[1] as usize] ^ rcon,
                    sbox[t[2] as usize],
                    sbox[t[3] as usize],
                    sbox[t[0] as usize],
                ];
                rcon = xtime(rcon);
            } else if i % 8 == 4 {
                t = [
                    sbox[t[0] as usize],
                    sbox[t[1] as usize],
                    sbox[t[2] as usize],
                    sbox[t[3] as usize],
                ];
            }
            for j in 0..4 {
                w[i][j] = w[i - 8][j] ^ t[j];
            }
        }
        let mut round_keys = [[0u8; 16]; 15];
        for (r, rk) in round_keys.iter_mut().enumerate() {
            for c in 0..4 {
                rk[4 * c..4 * c + 4].copy_from_slice(&w[4 * r + c]);
            }
        }
        Aes256 { sbox, round_keys }
    }

    pub fn encrypt_block(&self, block: &[u8; 16]) -> [u8; 16] {
        let mut s = *block;
        let add = |s: &mut [u8; 16], r: usize| {
            for i in 0..16 {
                s[i] ^= self.round_keys[r][i];
            }
        };
        add(&mut s, 0);
        for r in 1..=14 {
            for b in s.iter_mut() {
                *b = self.sbox[*b as usize];
            }
            // shift rows (state is column-major: s[4*c + row])
            let t = s;
            for c in 0..4 {
                for row in 0..4 {
                    s[4 * c + row] = t[4 * ((c + row) % 4) + row];
                }
            }
            if r != 14 {
                for c in 0..4 {
                    let col = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                    let x = |v: u8| xtime(v);
                    s[4 * c] = x(col[0]) ^ (x(col[1]) ^ col[1]) ^ col[2] ^ col[3];
                    s[4 * c + 1] = col[0] ^ x(col[1]) ^ (x(col[2]) ^ col[2]) ^ col[3];
                    s[4 * c + 2] = col[0] ^ col[1] ^ x(col[2]) ^ (x(col[3]) ^ col[3]);
                    s[4 * c + 3] = (x(col[0]) ^ col[0]) ^ col[1] ^ col[2] ^ x(col[3]);
                }
            }
            add(&mut s, r);
        }
        s
    }

    /// AES-OFB: `out = data ^ keystream`, keystream = E(iv), E(E(iv)), ... (the same call encrypts and decrypts).
    pub fn ofb(&self, iv: [u8; 16], data: &[u8]) -> Vec<u8> {
        let mut state = iv;
        let mut out = Vec::with_capacity(data.len());
        for chunk in data.chunks(16) {
            state = self.encrypt_block(&state);
            out.extend(chunk.iter().zip(state.iter()).map(|(d, k)| d ^ k));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn fips_197_appendix_c3_vector() {
        let key: [u8; 32] = (0u8..32).collect::<Vec<_>>().try_into().unwrap();
        let aes = Aes256::new(&key);
        let pt: [u8; 16] = hex("00112233445566778899aabbccddeeff").try_into().unwrap();
        assert_eq!(
            aes.encrypt_block(&pt).to_vec(),
            hex("8ea2b7ca516745bfeafc49904b496089")
        );
    }

    #[test]
    fn sbox_matches_the_known_corners() {
        let s = sbox();
        assert_eq!(
            (s[0x00], s[0x01], s[0x53], s[0xff]),
            (0x63, 0x7c, 0xed, 0x16)
        );
    }

    #[test]
    fn ofb_is_its_own_inverse_and_handles_partial_blocks() {
        let aes = Aes256::new(&[7u8; 32]);
        let msg: Vec<u8> = (0..37).collect();
        let ct = aes.ofb([9u8; 16], &msg);
        assert_ne!(ct, msg);
        assert_eq!(aes.ofb([9u8; 16], &ct), msg);
    }
}
