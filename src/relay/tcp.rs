use anyhow::Result;
use async_trait::async_trait;
use log::{debug, error, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::relay::{RelayConfig, RelayInfo, RelayTrait};

pub struct TcpRelay {
    id: String,
    config: RelayConfig,
    active: bool,
}

impl TcpRelay {
    pub fn new(config: RelayConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            config,
            active: true,
        }
    }
}

#[async_trait]
impl RelayTrait for TcpRelay {
    fn id(&self) -> &str {
        &self.id
    }

    fn config(&self) -> &RelayConfig {
        &self.config
    }

    fn is_active(&self) -> bool {
        self.active
    }

    async fn start_relay(
        self: Arc<Self>,
        mut shutdown: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<()> {
        let listen_addr = format!("{}:{}", self.config.listen_host, self.config.listen_port);
        let forward_addr = format!("{}:{}", self.config.forward_host, self.config.forward_port);
        info!("[TCP] Starting relay listener on {}", listen_addr);
        let listener = TcpListener::bind(&listen_addr).await?;
        debug!("[TCP] Relay bound to {}, forwarding to {}", listen_addr, forward_addr);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    info!("[TCP] Relay on {} shutting down", listen_addr);
                    break;
                }
                result = listener.accept() => {
                    match result {
                        Ok((mut inbound, addr)) => {
                            debug!("[TCP] Relay accepted connection from {}", addr);
                            let forward = forward_addr.clone();
                            tokio::spawn(async move {
                                match TcpStream::connect(&forward).await {
                                    Ok(mut outbound) => {
                                        debug!("[TCP] Relay connected to forward {} for session from {}", forward, addr);
                                        let (mut ri, mut wi) = inbound.split();
                                        let (mut ro, mut wo) = outbound.split();
                                        let active = Arc::new(AtomicBool::new(true));
                                        let active_clone = Arc::clone(&active);

                                        let (_result1, _result2) = tokio::join! {
                                            async move {
                                                let mut buf = [0u8; 8192];
                                                while active_clone.load(Ordering::SeqCst) {
                                                    match ri.read(&mut buf).await {
                                                        Ok(0) => break,
                                                        Ok(n) => {
                                                            if wo.write_all(&buf[..n]).await.is_err() {
                                                                active_clone.store(false, Ordering::SeqCst);
                                                                break;
                                                            }
                                                        }
                                                        Err(_) => {
                                                            active_clone.store(false, Ordering::SeqCst);
                                                            break;
                                                        }
                                                    }
                                                }
                                            },
                                            async move {
                                                let mut buf = [0u8; 8192];
                                                while active.load(Ordering::SeqCst) {
                                                    match ro.read(&mut buf).await {
                                                        Ok(0) => break,
                                                        Ok(n) => {
                                                            if wi.write_all(&buf[..n]).await.is_err() {
                                                                active.store(false, Ordering::SeqCst);
                                                                break;
                                                            }
                                                        }
                                                        Err(_) => {
                                                            active.store(false, Ordering::SeqCst);
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                        };
                                    }
                                    Err(e) => error!("[TCP] Failed to connect to forward: {}", e),
                                }
                            });
                        }
                        Err(e) => error!("[TCP] Failed to accept connection: {}", e),
                    }
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
            protocol: "tcp".to_string(),
        }
    }
}
