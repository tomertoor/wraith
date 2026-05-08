use anyhow::Result;
use log::info;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::oneshot;

use super::{RelayConfig, RelayInfo, Transport};

pub struct RelayManager {
    relays: HashMap<String, (Arc<ProtocolRelay>, oneshot::Sender<()>)>,
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
        let relay = Arc::new(ProtocolRelay::new(config.clone()));
        let id = relay.id.clone();
        info!(
            "Creating relay: {}:{} ({} -> {}) -> {}:{}",
            config.listen.host,
            config.listen.port,
            config.listen.protocol,
            config.forward.host,
            config.forward.port,
            config.forward.protocol
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

pub struct ProtocolRelay {
    pub(crate) id: String,
    pub(crate) config: RelayConfig,
    pub(crate) active: Arc<AtomicBool>,
}

impl ProtocolRelay {
    pub fn new(config: RelayConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            config,
            active: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn to_relay_info(&self) -> RelayInfo {
        RelayInfo {
            relay_id: self.id.clone(),
            listen_host: self.config.listen.host.clone(),
            listen_port: self.config.listen.port,
            forward_host: self.config.forward.host.clone(),
            forward_port: self.config.forward.port,
            active: self.active.load(Ordering::SeqCst),
            protocol: format!("{}->{}", self.config.listen.protocol, self.config.forward.protocol),
        }
    }

    async fn start_relay(
        self: Arc<Self>,
        shutdown: oneshot::Receiver<()>,
    ) -> Result<()> {
        let listen_addr = format!("{}:{}", self.config.listen.host, self.config.listen.port);
        info!("[RELAY] Starting relay on {} ({}) -> {}:{} ({})",
            listen_addr, self.config.listen.protocol,
            self.config.forward.host, self.config.forward.port, self.config.forward.protocol);

        match self.config.listen.protocol {
            Transport::Tcp => {
                self.run_tcp_listen(listen_addr, shutdown).await?;
            }
            Transport::Udp => {
                self.run_udp_listen(listen_addr, shutdown).await?;
            }
        }

        info!("[RELAY] Relay {} stopped", self.id);
        Ok(())
    }
}
