//! Command handlers for the agent — tunnel and relay commands.

use crate::proto::*;
use crate::socat::relay::SocatRelay;
use crate::wraith::Config;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages command execution on the agent side.
pub struct CommandHandler {
    config: Config,
    client: Arc<RwLock<Option<crate::agent::client::AgentClient>>>,
    socat: Arc<RwLock<SocatRelay>>,
}

impl CommandHandler {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            client: Arc::new(RwLock::new(None)),
            socat: Arc::new(RwLock::new(SocatRelay::new())),
        }
    }

    /// Start the tunnel client and connect to relay.
    pub async fn start_tunnel(&self, tunnel_id: &str, relay_ip: &str, agent_ip: &str, netmask: &str) -> anyhow::Result<()> {
        let mut client = crate::agent::client::AgentClient::new(self.config.clone());
        client.connect_with_reconnect().await?;
        client.open_tunnel(tunnel_id, relay_ip, agent_ip, netmask).await?;
        *self.client.write().await = Some(client);
        Ok(())
    }

    /// Stop a tunnel.
    pub async fn stop_tunnel(&self, tunnel_id: &str) -> anyhow::Result<()> {
        if let Some(client) = self.client.write().await.as_mut() {
            client.close_tunnel(tunnel_id).await?;
        }
        Ok(())
    }

    /// Get tunnel statistics.
    pub async fn tunnel_stats(&self, tunnel_id: &str) -> anyhow::Result<TunnelStats> {
        if let Some(client) = self.client.read().await.as_ref() {
            let tunnels = client.tunnels();
            let tunnels = tunnels.read().await;
            if let Some(tunnel) = tunnels.get(tunnel_id) {
                return Ok(TunnelStats {
                    tunnel_id: tunnel.tunnel_id.clone(),
                    bytes_in: tunnel.bytes_in,
                    bytes_out: tunnel.bytes_out,
                    packets_in: tunnel.packets_in,
                    packets_out: tunnel.packets_out,
                });
            }
        }
        anyhow::bail!("tunnel not found")
    }

    /// Start a relay (socat-style) on the agent.
    pub async fn start_relay(&self, relay_id: &str, mode: &str, local_addr: &str, remote_addr: &str) -> anyhow::Result<()> {
        let mut socat = self.socat.write().await;
        socat.start_relay(relay_id, mode, local_addr, remote_addr).await?;
        Ok(())
    }

    /// Stop a relay.
    pub async fn stop_relay(&self, relay_id: &str) -> anyhow::Result<()> {
        let mut socat = self.socat.write().await;
        socat.stop_relay(relay_id).await?;
        Ok(())
    }

    /// List active relays.
    pub async fn list_relays(&self) -> anyhow::Result<RelayListResponse> {
        let socat = self.socat.read().await;
        Ok(socat.list_relays())
    }
}
