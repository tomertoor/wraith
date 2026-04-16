//! Custom lightweight userspace network stack for the agent (no root required).
//!
//! NOTE: This is a stub module. The TCP/UDP/ICMP handlers do not actually
//! proxy traffic — packets are logged and discarded. Full implementation
//! requires a userspace TCP stack or running the agent as root with a real TUN.
//!
//! To use this module, either implement the socket-based proxying here, or
//! require CAP_NET_ADMIN and use the real TUN path instead.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// A virtual "TUN" interface in userspace.
/// Emulates TUN behavior by intercepting IP packets and forwarding them
/// through actual network sockets to the target addresses.
pub struct UserspaceTun {
    /// Our virtual IP address.
    pub local_ip: Ipv4Addr,
    /// Relay's virtual IP address (the "gateway").
    pub remote_ip: Ipv4Addr,
    /// Ring buffer for outbound packets (to be encrypted and sent to relay).
    packet_tx: mpsc::Sender<Packet>,
    /// Inbound packet handler.
    packet_rx: mpsc::Receiver<Packet>,
    /// Active TCP connections proxied through the tunnel.
    connections: Arc<RwLock<HashMap<u64, ConnectionState>>>,
}

#[derive(Debug)]
pub struct Packet {
    pub data: Vec<u8>,
    pub src_ip: Ipv4Addr,
    pub dst_ip: Ipv4Addr,
    pub protocol: IpProtocol,
}

#[derive(Debug, Clone, Copy)]
pub enum IpProtocol {
    ICMP,
    TCP,
    UDP,
    Unknown,
}

impl Packet {
    /// Parse IP header to extract protocol.
    pub fn from_raw(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }
        let version = (data[0] >> 4) & 0xF;
        if version != 4 {
            return None;
        }
        let ihl = (data[0] & 0xF) as usize * 4;
        let proto_num = data[9];
        let src_bytes = &data[12..16];
        let dst_bytes = &data[16..20];

        let src_ip = Ipv4Addr::new(src_bytes[0], src_bytes[1], src_bytes[2], src_bytes[3]);
        let dst_ip = Ipv4Addr::new(dst_bytes[0], dst_bytes[1], dst_bytes[2], dst_bytes[3]);

        let protocol = match proto_num {
            1 => IpProtocol::ICMP,
            6 => IpProtocol::TCP,
            17 => IpProtocol::UDP,
            _ => IpProtocol::Unknown,
        };

        Some(Self {
            data: data[ihl..].to_vec(),
            src_ip,
            dst_ip,
            protocol,
        })
    }
}

enum ConnectionState {
    Pending,
    Connected(tokio::net::TcpStream),
}

/// Statistics for the userspace TUN.
#[derive(Debug, Default)]
pub struct TunStats {
    pub packets_in: u64,
    pub packets_out: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

impl UserspaceTun {
    pub fn new(local_ip: Ipv4Addr, remote_ip: Ipv4Addr) -> Self {
        let (packet_tx, packet_rx) = mpsc::channel(1024);
        Self {
            local_ip,
            remote_ip,
            packet_tx,
            packet_rx,
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Enqueue a packet for transmission through the tunnel.
    /// NOTE: The receiving end (next_outbound) must be polled by the agent
    /// loop, otherwise this channel will eventually become full and this
    /// method will return None.
    pub async fn send_packet(&self, data: Vec<u8>, protocol: IpProtocol) -> Option<()> {
        let src_ip = self.local_ip;
        let dst_ip = self.remote_ip;
        self.packet_tx
            .send(Packet {
                data,
                src_ip,
                dst_ip,
                protocol,
            })
            .await
            .ok()
    }

    /// Handle an incoming packet from the relay tunnel.
    /// Returns true if the packet was handled, false if it should be
    /// delivered to the virtual interface (loopback-style).
    pub async fn handle_incoming(&self, packet: &[u8]) -> Option<()> {
        if let Some(pkt) = Packet::from_raw(packet) {
            match pkt.protocol {
                IpProtocol::TCP => {
                    self.handle_tcp(pkt).await;
                }
                IpProtocol::UDP => {
                    self.handle_udp(pkt).await;
                }
                IpProtocol::ICMP => {
                    self.handle_icmp(pkt).await;
                }
                IpProtocol::Unknown => {}
            }
        }
        Some(())
    }

    async fn handle_tcp(&self, _pkt: Packet) {
        // TODO: Implement userspace TCP proxying.
        // Requires parsing the TCP header, tracking connections, and
        // establishing outbound sockets for each flow.
        unimplemented!("UserspaceTUN TCP handling not implemented — run agent as root for real TUN");
    }

    async fn handle_udp(&self, _pkt: Packet) {
        // TODO: Implement userspace UDP proxying.
        // Requires creating UDP sockets and NAT-like translation.
        unimplemented!("UserspaceTUN UDP handling not implemented — run agent as root for real TUN");
    }

    async fn handle_icmp(&self, _pkt: Packet) {
        // TODO: Implement userspace ICMP echo handling.
        unimplemented!("UserspaceTUN ICMP handling not implemented — run agent as root for real TUN");
    }

    /// Consume outbound packets from the queue.
    pub async fn next_outbound(&mut self) -> Option<Packet> {
        self.packet_rx.recv().await
    }

    /// Check if the agent has root privileges (real TUN capable).
    pub fn is_root() -> bool {
        std::fs::metadata("/dev/net/tun").is_ok()
    }
}
