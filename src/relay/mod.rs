use anyhow::Result;
use async_trait::async_trait;
use log::info;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::oneshot;

pub mod tcp;
pub mod udp;

pub use tcp::TcpRelay;
pub use udp::UdpRelay;

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
pub struct RelayConfig {
    pub listen_host: String,
    pub listen_port: u16,
    pub forward_host: String,
    pub forward_port: u16,
    pub protocol: Transport,
}

impl RelayConfig {
    pub fn new(
        listen_host: String,
        listen_port: u16,
        forward_host: String,
        forward_port: u16,
        protocol: Transport,
    ) -> Self {
        Self {
            listen_host,
            listen_port,
            forward_host,
            forward_port,
            protocol,
        }
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

#[async_trait]
pub trait RelayTrait: Send + Sync {
    fn id(&self) -> &str;
    fn config(&self) -> &RelayConfig;
    fn is_active(&self) -> bool;
    async fn start_relay(self: Arc<Self>, shutdown: oneshot::Receiver<()>) -> Result<()>;
    fn to_relay_info(&self) -> RelayInfo;
}

pub struct RelayManager {
    relays: HashMap<String, (Arc<dyn RelayTrait>, oneshot::Sender<()>)>,
}

impl Default for RelayManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayManager {
    pub fn new() -> Self {
        Self {
            relays: HashMap::new(),
        }
    }

    pub fn create_relay(&mut self, config: RelayConfig) -> String {
        let relay: Arc<dyn RelayTrait> = match config.protocol {
            Transport::Tcp => Arc::new(TcpRelay::new(config.clone())),
            Transport::Udp => Arc::new(UdpRelay::new(config.clone())),
        };
        let id = relay.id().to_string();
        info!(
            "Creating {} relay {}: {}:{} -> {}:{}",
            config.protocol,
            id,
            config.listen_host,
            config.listen_port,
            config.forward_host,
            config.forward_port
        );

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let relay_clone = Arc::clone(&relay);

        self.relays.insert(id.clone(), (relay, shutdown_tx));

        tokio::spawn(async move {
            let _ = relay_clone.start_relay(shutdown_rx).await;
        });
        id
    }

    pub fn delete_relay(&mut self, id: &str) -> bool {
        if let Some((_relay, shutdown_tx)) = self.relays.remove(id) {
            let _ = shutdown_tx.send(());
            info!("Deleted relay {}", id);
            true
        } else {
            log::warn!("Attempted to delete non-existent relay: {}", id);
            false
        }
    }

    pub fn list_relays(&self) -> Vec<RelayInfo> {
        self.relays
            .values()
            .map(|(r, _)| r.to_relay_info())
            .collect()
    }
}
