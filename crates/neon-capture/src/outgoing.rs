//! Outgoing packet construction for MapleStory Classic World.
//!
//! Based on analysis in docs/network-protocol.md:
//! - Header: `[a>>8, a, c>>8, c]` big-endian, where `c = a ^ byteswap(len)`
//! - IV: 4 bytes, starts at bytes 9..12 of plaintext server handshake
//! - IV advances via classic MapleStory shuffle after each packet
//! - Header seed: `a = ((iv[2] << 8) | iv[3]) ^ 0xdf00`
//! - Payload: NOT yet solved (uniformly random per packet)

// std::io::{Read, Seek} no longer needed after read_iv_from_memory refactor

/// The 256-byte shuffle table used by MapleStory for IV advancement.
/// This is a permutation table that maps each byte value 0-255 to another value.
/// Located at MapleStory.exe file offset 0x3bb3d00.
const SHUFFLE_TABLE: [u8; 256] = [
    0xec, 0x3f, 0x77, 0xa4, 0x45, 0xd0, 0x71, 0xbf, 0xb7, 0x98, 0x20, 0xfc, 0x4b, 0xe9, 0xb3, 0xe1,
    0x5c, 0x22, 0xf7, 0x0c, 0x44, 0x1b, 0x81, 0xbd, 0x63, 0x8d, 0xd4, 0xc3, 0xf2, 0x10, 0x19, 0xe0,
    0xfb, 0xa1, 0x6e, 0x66, 0xea, 0xae, 0xd6, 0xce, 0x06, 0x18, 0x4e, 0xeb, 0x78, 0x95, 0xdb, 0xba,
    0xb6, 0x42, 0x7a, 0x2a, 0x83, 0x0b, 0x54, 0x67, 0x6d, 0xe8, 0x65, 0xe7, 0x2f, 0x07, 0xf3, 0xaa,
    0x27, 0x7b, 0x85, 0xb0, 0x26, 0xfd, 0x8b, 0xa9, 0xfa, 0xbe, 0xa8, 0xd7, 0xcb, 0xcc, 0x92, 0xda,
    0xf9, 0x93, 0x60, 0x2d, 0xdd, 0xd2, 0xa2, 0x9b, 0x39, 0x5f, 0x82, 0x21, 0x4c, 0x69, 0xf8, 0x31,
    0x87, 0xee, 0x8e, 0xad, 0x8c, 0x6a, 0xbc, 0xb5, 0x6b, 0x59, 0x13, 0xf1, 0x04, 0x00, 0xf6, 0x5a,
    0x35, 0x79, 0x48, 0x8f, 0x15, 0xcd, 0x97, 0x57, 0x12, 0x3e, 0x37, 0xff, 0x9d, 0x4f, 0x51, 0xf5,
    0xa3, 0x70, 0xbb, 0x14, 0x75, 0xc2, 0xb8, 0x72, 0xc0, 0xed, 0x7d, 0x68, 0xc9, 0x2e, 0x0d, 0x62,
    0x46, 0x17, 0x11, 0x4d, 0x6c, 0xc4, 0x7e, 0x53, 0xc1, 0x25, 0xc7, 0x9a, 0x1c, 0x88, 0x58, 0x2c,
    0x89, 0xdc, 0x02, 0x64, 0x40, 0x01, 0x5d, 0x38, 0xa5, 0xe2, 0xaf, 0x55, 0xd5, 0xef, 0x1a, 0x7c,
    0xa7, 0x5b, 0xa6, 0x6f, 0x86, 0x9f, 0x73, 0xe6, 0x0a, 0xde, 0x2b, 0x99, 0x4a, 0x47, 0x9c, 0xdf,
    0x09, 0x76, 0x9e, 0x30, 0x0e, 0xe4, 0xb2, 0x94, 0xa0, 0x3b, 0x34, 0x1d, 0x28, 0x0f, 0x36, 0xe3,
    0x23, 0xb4, 0x03, 0xd8, 0x90, 0xc8, 0x3c, 0xfe, 0x5e, 0x32, 0x24, 0x50, 0x1f, 0x3a, 0x43, 0x8a,
    0x96, 0x41, 0x74, 0xac, 0x52, 0x33, 0xf0, 0xd9, 0x29, 0x80, 0xb1, 0x16, 0xd3, 0xab, 0x91, 0xb9,
    0x84, 0x7f, 0x61, 0x1e, 0xcf, 0xc5, 0xd1, 0x56, 0x3d, 0xca, 0xf4, 0x05, 0xc6, 0xe5, 0x08, 0x49,
];

/// The classic MapleStory IV shuffle, ported from `neon-calib/tools/mcrypt_check.py`, which reproduces the header
/// seed of all 93 client frames of a real connection start (`a ^ ((iv[2]<<8)|iv[3])` is the constant 0xdf00 for each).
pub fn shuffle_iv(iv: &mut [u8; 4]) {
    let t = &SHUFFLE_TABLE;
    let mut v: [u8; 4] = [0xf2, 0x53, 0x50, 0xc6];
    for &inp in iv.iter() {
        let a = inp as usize;
        let m = t[v[1] as usize].wrapping_sub(inp);
        v[0] = v[0].wrapping_add(m);
        let m = v[2] ^ t[a];
        v[1] = v[1].wrapping_sub(m);
        let e = v[3];
        let m = t[e as usize].wrapping_add(inp) ^ v[2];
        v[2] = m;
        v[3] = e.wrapping_sub(v[0]).wrapping_add(t[a]);
        let x = u32::from_le_bytes(v).rotate_left(3);
        v = x.to_le_bytes();
    }
    *iv = v;
}

/// Compute the header seed from the current IV.
/// a = ((iv[2] << 8) | iv[3]) ^ 0xdf00
pub fn compute_header_seed(iv: &[u8; 4]) -> u16 {
    let base = ((iv[2] as u16) << 8) | (iv[3] as u16);
    base ^ 0xdf00
}

/// Encode a raw client->server frame for direct socket send.
/// This is a simplified version of `send::encode_frame` that takes the AES key directly.
/// Used by `packet_actuator.rs` for one-off packet sends.
pub fn encode_frame_raw(key: &[u8; 32], plain: &[u8], iv: &[u8; 4]) -> Vec<u8> {
    use crate::aes::Aes256;

    let a = compute_header_seed(iv);
    let c = a ^ (plain.len() as u16).swap_bytes();

    // Encrypt with AES-256-OFB
    let aes = Aes256::new(key);
    let block = [
        iv[0], iv[1], iv[2], iv[3], iv[0], iv[1], iv[2], iv[3], iv[0], iv[1], iv[2], iv[3], iv[0],
        iv[1], iv[2], iv[3],
    ];
    let mut encrypted = Vec::with_capacity(plain.len());
    let mut pos = 0;
    let mut first = true;
    while pos < plain.len() {
        let n = (plain.len() - pos).min(if first { 1456 } else { 1460 });
        encrypted.extend(aes.ofb(block, &plain[pos..pos + n]));
        pos += n;
        first = false;
    }

    let mut out = Vec::with_capacity(4 + plain.len());
    out.extend(a.to_be_bytes());
    out.extend(c.to_be_bytes());
    out.extend(encrypted);

    out
}

/// Build a 4-byte header for an outgoing packet.
/// Format: `[a>>8, a, c>>8, c]` big-endian, where `c = a ^ byteswap(len)`
pub fn build_header(a: u16, len: usize) -> [u8; 4] {
    let c = a ^ ((len as u16) as u16).swap_bytes();
    [
        (a >> 8) as u8,
        (a & 0xff) as u8,
        (c >> 8) as u8,
        (c & 0xff) as u8,
    ]
}

/// A constructed outgoing packet ready to send.
#[derive(Debug, Clone)]
pub struct OutgoingPacket {
    /// The full packet including header
    pub data: Vec<u8>,
    /// The packet opcode (first 2 bytes of payload)
    pub opcode: u16,
    /// The IV used for this packet (before shuffle)
    pub iv_used: [u8; 4],
}

/// Builder for outgoing packets.
#[derive(Debug, Clone)]
pub struct PacketBuilder {
    /// Current IV (advances after each packet)
    iv: [u8; 4],
    /// Whether we've started (for initial IV setup)
    initialized: bool,
}

impl PacketBuilder {
    /// Create a new builder with the given initial IV.
    /// The IV is bytes 9..12 of the plaintext server handshake.
    pub fn new(initial_iv: [u8; 4]) -> Self {
        Self {
            iv: initial_iv,
            initialized: true,
        }
    }

    /// Create a builder with the known default IV from the handshake.
    /// Based on: `01 00 01 00 00 00 01 00 30 | 2d ef c2 e1 | 09 27 cf 40 01 04`
    /// IV = `2d ef c2 e1`
    pub fn with_default_iv() -> Self {
        Self::new([0x2d, 0xef, 0xc2, 0xe1])
    }

    /// Build a packet with the given opcode and payload.
    /// The payload is NOT encrypted (encryption key not yet solved).
    pub fn build(&mut self, opcode: u16, payload: &[u8]) -> OutgoingPacket {
        let iv_used = self.iv;
        let a = compute_header_seed(&self.iv);
        let len = 2 + payload.len(); // opcode + payload
        let header = build_header(a, len);

        // Build the packet: header + opcode + payload
        let mut data = Vec::with_capacity(4 + len);
        data.extend_from_slice(&header);
        data.extend_from_slice(&opcode.to_le_bytes());
        data.extend_from_slice(payload);

        // Advance IV for next packet
        shuffle_iv(&mut self.iv);

        OutgoingPacket {
            data,
            opcode,
            iv_used,
        }
    }

    /// Build a ping packet (opcode 0x0001, empty payload).
    pub fn ping(&mut self) -> OutgoingPacket {
        self.build(0x0001, &[])
    }

    /// Get the current IV.
    pub fn current_iv(&self) -> [u8; 4] {
        self.iv
    }

    /// Set the IV directly (for recovery after reading from memory).
    pub fn set_iv(&mut self, iv: [u8; 4]) {
        self.iv = iv;
        self.initialized = true;
    }
}

impl Default for PacketBuilder {
    fn default() -> Self {
        Self::with_default_iv()
    }
}

/// Read the current send IV from the game process memory.
/// Location: 0x2c44c90 (build-specific, may need to be re-found).
/// NOTE: this address is below the PE image base so it is NOT rebased by --image-base;
/// it may be a Wine/heap address that needs separate calibration on macOS.
pub fn read_iv_from_memory(pid: u32) -> Option<[u8; 4]> {
    let scanner = crate::scanner::MemoryScanner::new(pid).ok()?;
    let addr: u64 = 0x2c44c90;
    let data = crate::chains::Mem::read(&scanner, addr, 4)?;
    data.try_into().ok()
}

// There is deliberately no table of outgoing opcode constants here. The constants that used to be here (PING 0x0001,
// MOVE 0x029d, ATTACK 0x0128, SKILL 0x0190) were guesses; 0x029d is an *incoming* opcode. Observed outgoing opcodes
// live in docs/opcodes.md / docs/outgoing-opcodes.json and are not verified well enough to send.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iv_chain_matches_the_verified_python_reference() {
        // Values from neon-calib/tools/mcrypt_check.py run on the real MapleStory.exe table (also checked there
        // against 93 real client headers).
        let want = [
            [0x2d, 0xef, 0xc2, 0xe1],
            [0xc4, 0xa8, 0x64, 0x1f],
            [0x51, 0xb1, 0x87, 0x51],
            [0xe0, 0xf7, 0xb2, 0xc2],
            [0x6b, 0x9d, 0x0d, 0xa5],
        ];
        let mut iv = want[0];
        for w in &want[1..] {
            shuffle_iv(&mut iv);
            assert_eq!(&iv, w);
        }
    }

    #[test]
    fn shuffle_table_is_a_permutation() {
        let mut seen = [false; 256];
        for &b in SHUFFLE_TABLE.iter() {
            assert!(!seen[b as usize], "duplicate {b:#x}");
            seen[b as usize] = true;
        }
        assert_eq!(
            &SHUFFLE_TABLE[..8],
            &[0xec, 0x3f, 0x77, 0xa4, 0x45, 0xd0, 0x71, 0xbf]
        );
    }

    #[test]
    fn shuffle_iv_advances_correctly() {
        let mut iv = [0x2d, 0xef, 0xc2, 0xe1];
        let original = iv;
        shuffle_iv(&mut iv);
        assert_ne!(iv, original, "IV should change after shuffle");

        // Verify the shuffle is deterministic
        let mut iv2 = original;
        shuffle_iv(&mut iv2);
        assert_eq!(iv, iv2, "Shuffle should be deterministic");
    }

    #[test]
    fn header_seed_computation() {
        let iv = [0x2d, 0xef, 0xc2, 0xe1];
        let seed = compute_header_seed(&iv);
        // a = ((0xc2 << 8) | 0xe1) ^ 0xdf00 = 0xc2e1 ^ 0xdf00 = 0x1de1
        assert_eq!(seed, 0x1de1);
    }

    #[test]
    fn header_building() {
        let header = build_header(0x1234, 10);
        // c = 0x1234 ^ byteswap(10) = 0x1234 ^ 0x0a00 = 0x1834
        assert_eq!(header, [0x12, 0x34, 0x18, 0x34]);
    }

    #[test]
    fn packet_construction() {
        let mut builder = PacketBuilder::with_default_iv();
        let packet = builder.build(0x00ae, b"Hello");

        assert_eq!(packet.opcode, 0x00ae);
        assert_eq!(packet.data.len(), 4 + 2 + 5); // header + opcode + payload
        assert!(packet.data[4..6] == [0xae, 0x00]); // opcode little-endian
        assert!(packet.data[6..] == *b"Hello");
    }

    #[test]
    fn iv_advances_between_packets() {
        let mut builder = PacketBuilder::with_default_iv();
        let iv1 = builder.current_iv();
        let _ = builder.ping();
        let iv2 = builder.current_iv();
        assert_ne!(iv1, iv2, "IV should advance after each packet");
    }
}
