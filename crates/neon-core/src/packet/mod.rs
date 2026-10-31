//! Packet module: intercept, craft, block, watch

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Packet {
    pub id: u16,
    pub direction: PacketDirection,
    pub data: Vec<u8>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PacketDirection {
    ClientToServer,
    ServerToClient,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketFilter {
    pub id: u16,
    pub direction: Option<PacketDirection>,
    pub action: PacketAction,
    pub condition: Option<String>, // Lua expression
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PacketAction {
    Allow,
    Block,
    Modify(Vec<u8>),
    Log,
}

pub struct PacketHandler {
    pub filters: Vec<PacketFilter>,
    pub logged_packets: Vec<Packet>,
}

impl Default for PacketHandler {
    fn default() -> Self {
        Self {
            filters: vec![],
            logged_packets: vec![],
        }
    }
}

impl PacketHandler {
    pub fn process(&mut self, packet: &mut Packet) -> bool {
        // Apply filters
        for filter in &self.filters {
            if filter.id == packet.id {
                if let Some(ref direction) = filter.direction {
                    if &packet.direction != direction {
                        continue;
                    }
                }

                match &filter.action {
                    PacketAction::Block => return false,
                    PacketAction::Modify(data) => {
                        packet.data = data.clone();
                    }
                    PacketAction::Log => {
                        self.logged_packets.push(packet.clone());
                    }
                    PacketAction::Allow => {}
                }
            }
        }
        true
    }
}
