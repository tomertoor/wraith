use anyhow::Result;
use log::{error, info};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

#[derive(Clone)]
pub struct RelayConfig {
    pub listen_host: String,
    pub listen_port: u16,
    pub forward_host: String,
    pub forward_port: u16,
}

pub struct Relay {
    pub id: String,
    pub config: RelayConfig,
    active: bool,
}

impl Relay {
    pub fn new(config: RelayConfig) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            config,
            active: true,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }
}

pub struct RelayManager {
    relays: HashMap<String, Relay>,
}

impl Default for RelayManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayManager {
    pub fn new() -> Self {
        Self { relays: HashMap::new() }
    }

    pub fn create_relay(&mut self, config: RelayConfig) -> String {
        let relay = Relay::new(config);
        let id = relay.id.clone();
        self.relays.insert(id.clone(), relay);
        id
    }

    pub fn delete_relay(&mut self, id: &str) -> bool {
        self.relays.remove(id).is_some()
    }

    pub fn list_relays(&self) -> Vec<RelayInfo> {
        self.relays.values().map(|r| RelayInfo {
            relay_id: r.id.clone(),
            listen_host: r.config.listen_host.clone(),
            listen_port: r.config.listen_port,
            forward_host: r.config.forward_host.clone(),
            forward_port: r.config.forward_port,
            active: r.is_active(),
        }).collect()
    }

    pub fn get_relay(&self, id: &str) -> Option<&Relay> {
        self.relays.get(id)
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
}

pub async fn start_relay(
    listen_addr: String,
    forward_addr: String,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    let listener = TcpListener::bind(&listen_addr).await?;
    info!("Relay listening on {}", listen_addr);

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                info!("Relay shutdown signal received");
                break;
            }
            result = listener.accept() => {
                match result {
                    Ok((mut inbound, _)) => {
                        let forward = forward_addr.clone();
                        tokio::spawn(async move {
                            match TcpStream::connect(&forward).await {
                                Ok(mut outbound) => {
                                    let (mut ri, mut wi) = inbound.split();
                                    let (mut ro, mut wo) = outbound.split();
                                    let active = Arc::new(AtomicBool::new(true));
                                    let active_clone = Arc::clone(&active);

                                    let (result1, result2) = tokio::join! {
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
                                Err(e) => error!("Failed to connect to forward: {}", e),
                            }
                        });
                    }
                    Err(e) => error!("Failed to accept connection: {}", e),
                }
            }
        }
    }
    Ok(())
}