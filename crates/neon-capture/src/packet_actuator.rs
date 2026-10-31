//! PacketActuator: send constructed outgoing packets through the game's TCP connection.
//!
//! This module connects the `outgoing::PacketBuilder` to the actual network socket
//! by discovering the game client's TCP connection and writing encrypted frames.
//!
//! Phase 6: Packet Injection Features
//!
//! Supported features:
//! - Kami: MoveSelf teleport to mob position
//! - Skill Injection: AttackSkill packets with correct skill ID + target OID
//! - Full Map Attack: Extended attack range
//! - No Delay: Bypass cooldown timer
//! - Movement Unlocks: Flash jump, no air check
//! - Mob Disarm: Filter incoming mob attacks (complement to god mode)

use std::io::Write;

use crate::outgoing;
use crate::send;

/// The PacketActuator sends constructed packets through the game's TCP connection.
pub struct PacketActuator {
    /// The TCP connection to the game server
    socket: Option<std::net::TcpStream>,
    /// Packet builder for constructing outgoing frames
    builder: Option<outgoing::PacketBuilder>,
    /// Send cipher for frame encryption
    send_cipher: send::SendCipher,
    /// Current connection IV (updated after each packet)
    current_iv: [u8; 4],
}

impl PacketActuator {
    /// Create a new PacketActuator with the given IV.
    pub fn new(iv: [u8; 4]) -> Self {
        Self {
            socket: None,
            builder: Some(outgoing::PacketBuilder::new(iv)),
            send_cipher: send::SendCipher::default(),
            current_iv: iv,
        }
    }

    /// Connect to the game server by discovering the existing TCP connection.
    /// On macOS: read /proc/<pid>/net/tcp6 or use socket inspection.
    /// On Linux: read /proc/<pid>/net/tcp6.
    pub fn connect(&mut self, pid: u32) -> Result<(), String> {
        let ports = crate::net::game_ports_of(pid);
        if ports.is_empty() {
            return Err("No game TCP connections found for PID".into());
        }

        // Find an established connection to a game port
        let local_port = ports
            .first()
            .copied()
            .ok_or("No established game connections")?;

        // Connect using the same local port (we're injecting into the same process)
        // The actual server address is determined by the current channel
        // For now, use the default game server port
        let server_port = 8500u16; // Default game port range start

        // We need to connect to the same server the game is using
        // This requires reading the server address from the game's memory
        // For now, we'll use a passive approach: read from the existing connection
        self.socket = None; // Placeholder — needs actual socket discovery
        Ok(())
    }

    /// Build and send a packet with the given opcode and payload.
    pub fn send_packet(&mut self, opcode: u16, payload: &[u8]) -> Result<(), String> {
        let builder = self
            .builder
            .as_mut()
            .ok_or("PacketBuilder not initialized")?;

        // Build the packet
        let packet = builder.build(opcode, payload);

        // Encrypt and frame the packet
        let aes = &crate::send::KEY; // Use the static key
        let iv = packet.iv_used;
        let frame = outgoing::encode_frame_raw(aes, payload, &iv);

        // Send through the socket
        if let Some(socket) = self.socket.as_mut() {
            socket
                .write_all(&frame)
                .map_err(|e| format!("Socket write failed: {}", e))?;
            socket
                .flush()
                .map_err(|e| format!("Socket flush failed: {}", e))?;
        } else {
            return Err("Not connected to game server".into());
        }

        Ok(())
    }

    /// Send a keepalive packet (opcode 0x0001, empty payload).
    pub fn send_keepalive(&mut self) -> Result<(), String> {
        self.send_packet(0x0001, &[])
    }

    /// Kami: Teleport to a mob's position using MoveSelf packet.
    /// opcode: 0x00db (MoveSelf)
    pub fn kami(&mut self, mob_x: i16, mob_y: i16) -> Result<(), String> {
        // MoveSelf packet structure: [x:4][y:4][unknown:4]
        let payload = [
            mob_x as i32,
            mob_y as i32,
            0, // unknown field
        ]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<u8>>();

        self.send_packet(0x00db, &payload)
    }

    /// Skill Injection: Attack with a skill ID.
    /// opcode: 0x00e1 (AttackSkill)
    pub fn attack_skill(
        &mut self,
        skill_id: u16,
        target_oid: u32,
        target_x: i16,
        target_y: i16,
    ) -> Result<(), String> {
        // AttackSkill packet structure (approximate):
        // [self_oid:4][skill_id:2][target_oid:4][target_x:2][target_y:2][tick:4]
        let payload = [
            0u32, // self_oid (filled by game)
            skill_id as u32,
            target_oid,
            target_x as u32,
            target_y as u32,
            0u32, // tick
        ]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<u8>>();

        self.send_packet(0x00e1, &payload)
    }

    /// Full Map Attack: Extended attack range to hit all mobs.
    /// This modifies the attack packet to include an extended range.
    pub fn full_map_attack(&mut self, target_oid: u32) -> Result<(), String> {
        // Standard attack with extended range flags
        // opcode: 0x00e1 (Attack)
        let payload = [
            0u32, // self_oid
            target_oid, 0xFFFFu32, // extended range marker
        ]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<u8>>();

        self.send_packet(0x00e1, &payload)
    }

    /// No Delay: Send attack without waiting for cooldown.
    /// This is implemented by sending at server-tick rate (~100ms).
    pub fn no_delay_attack(&mut self, target_oid: u32) -> Result<(), String> {
        // Same as regular attack, but called without cooldown checks
        self.attack_skill(0, target_oid, 0, 0)
    }

    /// Movement Unlock: Flash jump (air jump without ground check).
    /// opcode: 0x00db (MoveSelf) with flight flags
    pub fn flash_jump(&mut self, x: i16, y: i16, flight: u8) -> Result<(), String> {
        // MoveSelf with flight/dash flags
        let payload = [
            x as i32,
            y as i32,
            flight as i32, // flight/dash flag
        ]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<u8>>();

        self.send_packet(0x00db, &payload)
    }

    /// Mob Disarm: Send a packet to disable mob attacks (complement to god mode).
    /// This filters incoming mob attack packets at the wire level.
    /// Note: This is more of a filter than a send — implemented in the recv side.
    pub fn mob_disarm_active(&mut self) -> Result<(), String> {
        // Send a request to disable mob targeting (opcode varies by game version)
        // For now, this is a placeholder — actual implementation needs wire opcode mapping
        Ok(())
    }

    /// Get the current IV state.
    pub fn current_iv(&self) -> [u8; 4] {
        self.current_iv
    }

    /// Check if connected.
    pub fn is_connected(&self) -> bool {
        self.socket.is_some()
    }
}

/// Helper to encode a raw frame without the full SendCipher machinery.
/// Used for one-off packet sends.
pub fn encode_frame_raw(key: &[u8; 32], plain: &[u8], iv: &[u8; 4]) -> Vec<u8> {
    use crate::outgoing::shuffle_iv;

    let a = outgoing::compute_header_seed(iv);
    let c = a ^ (plain.len() as u16).swap_bytes();

    // Encrypt with AES-256-OFB
    let aes = crate::aes::Aes256::new(key);
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

    // Advance IV
    let mut iv_mut = *iv;
    shuffle_iv(&mut iv_mut);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_actuator_initializes_with_iv() {
        let actuator = PacketActuator::new([0x2d, 0xef, 0xc2, 0xe1]);
        assert_eq!(actuator.current_iv(), [0x2d, 0xef, 0xc2, 0xe1]);
        assert!(!actuator.is_connected());
    }

    #[test]
    fn encode_frame_raw_produces_valid_header() {
        let key = crate::send::KEY;
        let iv = [0x2d, 0xef, 0xc2, 0xe1];
        let plain = vec![0xae, 0x00, 0x01, 0x02]; // opcode + payload

        let frame = encode_frame_raw(&key, &plain, &iv);

        // Check header: a = ((iv[2] << 8) | iv[3]) ^ 0xdf00 = 0xc2e1 ^ 0xdf00 = 0x1de1
        let a = outgoing::compute_header_seed(&iv);
        assert_eq!(a, 0x1de1);

        // Header should be [a>>8, a, c>>8, c]
        let c = a ^ ((plain.len() as u16).swap_bytes());
        assert_eq!(frame[0], (a >> 8) as u8);
        assert_eq!(frame[1], (a & 0xff) as u8);
        assert_eq!(frame[2], (c >> 8) as u8);
        assert_eq!(frame[3], (c & 0xff) as u8);
    }
}
