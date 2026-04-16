//! Peer handshake protocol.
//!
//! Handles version negotiation and peer info exchange during connection establishment.

use crate::proto::PeerMessage;

pub use crate::proto::PeerInfo;

/// Create local peer info for outbound connections.
pub fn create_peer_info(node_id: &str, tun_ip: &str) -> PeerInfo {
    PeerInfo {
        node_id: node_id.to_string(),
        version: "0.2.0".to_string(),
        tun_capable: true,
        tun_ip: tun_ip.to_string(),
        supports_relay: true,
        supports_nexus: true,
        options: std::collections::HashMap::new(),
    }
}

pub fn encode_peer_message(msg: &PeerMessage) -> Vec<u8> {
    let mut buf = Vec::new();
    prost::Message::encode(msg, &mut buf).unwrap();
    buf
}
