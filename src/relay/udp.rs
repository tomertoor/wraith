use anyhow::Result;
use async_trait::async_trait;
use log::{debug, error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

use crate::relay::{RelayConfig, RelayInfo, RelayTrait};

pub struct UdpRelay {
    id: String,
    config: RelayConfig,
}

impl UdpRelay {
    pub fn new(config: RelayConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            config,
        }
    }
}

#[async_trait]
impl RelayTrait for UdpRelay {
    fn id(&self) -> &str {
        &self.id
    }

    fn config(&self) -> &RelayConfig {
        &self.config
    }

    fn is_active(&self) -> bool {
        true
    }

    async fn start_relay(
        self: Arc<Self>,
        mut shutdown: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<()> {
        let listen_addr = format!("{}:{}", self.config.listen_host, self.config.listen_port);
        let forward_addr = format!("{}:{}", self.config.forward_host, self.config.forward_port);
        info!("[UDP] Starting relay listener on {}", listen_addr);

        let inbound = Arc::new(UdpSocket::bind(&listen_addr).await?);
        debug!("[UDP] Relay bound to {}, forwarding to {}", listen_addr, forward_addr);

        let mut buf = [0u8; 8192];

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    info!("[UDP] Relay on {} shutting down", listen_addr);
                    break;
                }
                result = inbound.recv_from(&mut buf) => {
                    let (n, src) = match result {
                        Ok(x) => x,
                        Err(e) => {
                            error!("[UDP] Failed to receive: {}", e);
                            continue;
                        }
                    };
                    debug!("[UDP] Received {} bytes from {}", n, src);

                    let outbound = match UdpSocket::bind("0.0.0.0:0").await {
                        Ok(s) => s,
                        Err(e) => {
                            error!("[UDP] Failed to bind outbound socket: {}", e);
                            continue;
                        }
                    };

                    if let Err(e) = outbound.send_to(&buf[..n], &forward_addr).await {
                        error!("[UDP] Failed to send to forward {}: {}", forward_addr, e);
                        continue;
                    }

                    let inbound_clone = Arc::clone(&inbound);
                    let src_addr = src;

                    tokio::spawn(async move {
                        let mut response_buf = [0u8; 8192];
                        let result = timeout(
                            Duration::from_millis(500),
                            outbound.recv_from(&mut response_buf)
                        ).await;
                        if let Ok(Ok((m, _))) = result {
                            let _ = inbound_clone.send_to(&response_buf[..m], src_addr).await;
                        }
                    });
                }
            }
        }
        Ok(())
    }

    fn to_relay_info(&self) -> RelayInfo {
        RelayInfo {
            relay_id: self.id.clone(),
            listen_host: self.config.listen_host.clone(),
            listen_port: self.config.listen_port,
            forward_host: self.config.forward_host.clone(),
            forward_port: self.config.forward_port,
            active: self.is_active(),
            protocol: "udp".to_string(),
        }
    }
}
