pub mod forward;
pub mod manager;
pub mod tcp_relay;
pub mod udp_relay;

// Re-exports: public API stays the same for all external consumers
pub use manager::RelayManager;
pub use manager::ProtocolRelay;

use serde::Serialize;

pub(crate) const RELAY_BUFFER_SIZE: usize = 8192;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Udp,
}

impl Default for Transport {
    fn default() -> Self {
        Transport::Tcp
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transport::Tcp => write!(f, "tcp"),
            Transport::Udp => write!(f, "udp"),
        }
    }
}

impl Transport {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "udp" => Transport::Udp,
            _ => Transport::Tcp,
        }
    }
}

#[derive(Clone)]
pub struct RelayEndpoint {
    pub host: String,
    pub port: u16,
    pub protocol: Transport,
}

impl RelayEndpoint {
    pub fn new(host: String, port: u16, protocol: Transport) -> Self {
        Self { host, port, protocol }
    }

    pub fn from_str(host: &str, port: u16, protocol: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            protocol: Transport::from_str(protocol),
        }
    }
}

#[derive(Clone)]
pub struct RelayConfig {
    pub listen: RelayEndpoint,
    pub forward: RelayEndpoint,
}

impl RelayConfig {
    pub fn new(listen: RelayEndpoint, forward: RelayEndpoint) -> Self {
        Self { listen, forward }
    }
}

#[derive(Serialize)]
pub struct RelayInfo {
    pub relay_id: String,
    pub listen_host: String,
    pub listen_port: u16,
    pub forward_host: String,
    pub forward_port: u16,
    pub active: bool,
    pub protocol: String,
}
