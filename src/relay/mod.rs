use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;
use log::{info, error};

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
}

pub struct RelayManager {
    relays: HashMap<String, Relay>,
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
            active: r.active,
        }).collect()
    }

    pub fn get_relay(&self, id: &str) -> Option<&Relay> {
        self.relays.get(id)
    }
}

pub struct RelayInfo {
    pub relay_id: String,
    pub listen_host: String,
    pub listen_port: u16,
    pub forward_host: String,
    pub forward_port: u16,
    pub active: bool,
}

pub async fn start_relay(listen_addr: String, forward_addr: String) -> Result<()> {
    let listener = TcpListener::bind(&listen_addr).await?;
    info!("Relay listening on {}", listen_addr);

    loop {
        match listener.accept().await {
            Ok((mut inbound, _)) => {
                let forward = forward_addr.clone();
                tokio::spawn(async move {
                    match TcpStream::connect(&forward).await {
                        Ok(mut outbound) => {
                            let (mut ri, mut wi) = inbound.split();
                            let (mut ro, mut wo) = outbound.split();
                            match tokio::io::copy(&mut ri, &mut wo).await {
                                Ok(_) => {}
                                Err(e) => error!("Relay error: {}", e),
                            }
                            match tokio::io::copy(&mut ro, &mut wi).await {
                                Ok(_) => {}
                                Err(e) => error!("Relay error: {}", e),
                            }
                        }
                        Err(e) => error!("Failed to connect to forward: {}", e),
                    }
                });
            }
            Err(e) => error!("Failed to accept connection: {}", e),
        }
    }
}