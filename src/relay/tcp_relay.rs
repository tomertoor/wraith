use anyhow::Result;
use log::{debug, error, info};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::net::TcpListener;

use super::forward::relay_connection;
use super::ProtocolRelay;

impl ProtocolRelay {
    pub(crate) async fn run_tcp_listen(&self, listen_addr: String, mut shutdown: tokio::sync::oneshot::Receiver<()>) -> Result<()> {
        let listener = TcpListener::bind(&listen_addr).await?;
        let active = Arc::clone(&self.active);
        let config = self.config.clone();

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    info!("[RELAY-TCP] Shutdown signal received");
                    active.store(false, Ordering::SeqCst);
                    break;
                }
                result = listener.accept() => {
                    match result {
                        Ok((inbound, addr)) => {
                            debug!("[RELAY-TCP] Accepted connection from {}", addr);
                            let active = Arc::clone(&active);
                            let config = config.clone();
                            tokio::spawn(async move {
                                let _ = relay_connection(&config, inbound, active).await;
                            });
                        }
                        Err(e) => {
                            error!("[RELAY-TCP] Failed to accept: {}", e);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
