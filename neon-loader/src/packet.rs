//! Packet hooking module using WinDivert

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Debug, Clone, Copy)]
pub struct PacketInfo {
    pub src_ip: IpAddr,
    pub dst_ip: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: u8,
}

pub struct PacketHook {
    pub pid: u32,
    pub filter: String,
    // TODO: Add WinDivertHandle when available
}

impl PacketHook {
    pub fn new(pid: u32) -> Result<Self, String> {
        // Filter for localhost traffic (MapleStory typically connects to local proxy)
        let filter = "ip.src == 127.0.0.1 or ip.dst == 127.0.0.1";

        Ok(Self { pid, filter: filter.to_string() })
    }

    pub fn start(&mut self) -> Result<(), String> {
        // TODO: Initialize WinDivert handle
        // let handle = windivert::WinDivertOpen(
        //     &self.filter,
        //     windivert::Layer::IPv4,
        //     windivert::Priority::Highest,
        //     0,
        // )?;

        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        // TODO: Close WinDivert handle
        Ok(())
    }

    /// Parse packet data to extract MapleStory packet info
    pub fn parse_maplestory_packet(&self, data: &[u8]) -> Option<MaplePacket> {
        if data.len() < 4 {
            return None;
        }

        // MapleStory packet format: [length:u16][packet_id:u16][data...]
        let length = u16::from_le_bytes([data[0], data[1]]) as usize;
        let packet_id = u16::from_le_bytes([data[2], data[3]]);

        if length > data.len() {
            return None;
        }

        Some(MaplePacket {
            length,
            packet_id,
            data: data[4..length].to_vec(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct MaplePacket {
    pub length: usize,
    pub packet_id: u16,
    pub data: Vec<u8>,
}

impl MaplePacket {
    /// Known packet IDs
    pub fn is_login_response(&self) -> bool {
        self.packet_id == 0x0008 // LOGIN_SUCCESS
    }

    pub fn is_damage_packet(&self) -> bool {
        self.packet_id == 0x0250 // DAMAGE
    }

    pub fn is_mob_spawn(&self) -> bool {
        self.packet_id == 0x0128 // MOB_SPAWN
    }

    pub fn is_skill_packet(&self) -> bool {
        self.packet_id == 0x0190 || self.packet_id == 0x0191 // SKILL_START
    }
}

impl Drop for PacketHook {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
