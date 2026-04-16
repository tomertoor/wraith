//! Unified socat-style relay module.

use crate::proto::{RelayListResponse, RelayStats};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{mpsc, RwLock};

/// Active relay instance.
struct Relay {
    id: String,
    mode: String,
    local_addr: String,
    remote_addr: String,
    /// Sender for relay data frames
    data_tx: Option<mpsc::Sender<Vec<u8>>>,
    bytes_in: u64,
    bytes_out: u64,
    connections: u64,
}

/// Unified socat relay manager.
pub struct SocatRelay {
    relays: HashMap<String, Arc<RwLock<Relay>>>,
}

impl SocatRelay {
    pub fn new() -> Self {
        Self {
            relays: HashMap::new(),
        }
    }

    /// Start a relay in the given mode.
    pub async fn start_relay(
        &mut self,
        relay_id: &str,
        mode: &str,
        local_addr: &str,
        remote_addr: &str,
    ) -> Result<()> {
        let (data_tx, _data_rx) = mpsc::channel(1024);

        let relay = Relay {
            id: relay_id.to_string(),
            mode: mode.to_string(),
            local_addr: local_addr.to_string(),
            remote_addr: remote_addr.to_string(),
            data_tx: Some(data_tx),
            bytes_in: 0,
            bytes_out: 0,
            connections: 0,
        };

        let relay_arc = Arc::new(RwLock::new(relay));

        match mode {
            "tcp" => {
                let r_id = relay_id.to_string();
                let l_addr = local_addr.to_string();
                let c_addr = remote_addr.to_string();
                let relay_c = relay_arc.clone();

                tokio::spawn(async move {
                    if let Err(e) = run_tcp_relay(&r_id, &l_addr, &c_addr, relay_c).await {
                        log::error!("TCP relay {} error: {}", r_id, e);
                    }
                });
            }
            "udp" => {
                let r_id = relay_id.to_string();
                let l_addr = local_addr.to_string();
                let relay_c = relay_arc.clone();

                tokio::spawn(async move {
                    if let Err(e) = run_udp_relay(&r_id, &l_addr, relay_c).await {
                        log::error!("UDP relay {} error: {}", r_id, e);
                    }
                });
            }
            "listener" => {
                let r_id = relay_id.to_string();
                let l_addr = local_addr.to_string();
                let relay_c = relay_arc.clone();

                tokio::spawn(async move {
                    if let Err(e) = run_listener_relay(&r_id, &l_addr, relay_c).await {
                        log::error!("Listener relay {} error: {}", r_id, e);
                    }
                });
            }
            "file" | "exec" | "forward" => {
                log::warn!(
                    "Relay {} mode '{}' is not implemented — use tcp, udp, or listener",
                    relay_id,
                    mode
                );
            }
            _ => {
                log::warn!("Unknown relay mode '{}'", mode);
            }
        }

        self.relays.insert(relay_id.to_string(), relay_arc);
        log::info!("Relay {} started in {} mode", relay_id, mode);
        Ok(())
    }

    /// Stop a relay.
    pub async fn stop_relay(&mut self, relay_id: &str) -> Result<()> {
        if let Some(relay) = self.relays.remove(relay_id) {
            relay.write().await.data_tx.take();
            log::info!("Relay {} stopped", relay_id);
        }
        Ok(())
    }

    /// Get stats for a specific relay.
    pub async fn relay_stats(&self, relay_id: &str) -> Option<RelayStats> {
        self.relays.get(relay_id).map(|r| {
            let r = r.blocking_read();
            RelayStats {
                relay_id: r.id.clone(),
                bytes_in: r.bytes_in,
                bytes_out: r.bytes_out,
                connections: r.connections,
            }
        })
    }

    /// List all active relays.
    pub fn list_relays(&self) -> RelayListResponse {
        use crate::proto::relay_list_response::RelayInfo;

        let relays: Vec<RelayInfo> = self
            .relays
            .iter()
            .map(|(_, r)| {
                let r = r.blocking_read();
                RelayInfo {
                    relay_id: r.id.clone(),
                    mode: r.mode.clone(),
                    local_addr: r.local_addr.clone(),
                    remote_addr: r.remote_addr.clone(),
                    status: "active".to_string(),
                    connections: r.connections,
                }
            })
            .collect();

        RelayListResponse { relays }
    }
}

/// TCP relay: listen locally, connect to remote, bidirectional pipe.
async fn run_tcp_relay(
    relay_id: &str,
    listen_addr: &str,
    connect_addr: &str,
    relay: Arc<RwLock<Relay>>,
) -> Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    log::info!("Relay {} listening on {}", relay_id, listen_addr);

    loop {
        let (inbound, _) = listener.accept().await?;
        let relay = relay.clone();
        let c_addr = connect_addr.to_string();
        let r_id = relay_id.to_string();

        tokio::spawn(async move {
            let relay = relay;
            let outbound = match TcpStream::connect(&c_addr).await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Relay {}: connect to {} failed: {}", r_id, c_addr, e);
                    return;
                }
            };

            relay.write().await.connections += 1;
            let relay_inner = relay.clone();

            // Split each stream for bidirectional copy
            let (ri, wi) = tokio::io::split(inbound);
            let (ro, wo) = tokio::io::split(outbound);

            let relay1 = relay.clone();
            let relay2 = relay.clone();

            let h1 = tokio::spawn(async move {
                let mut ri = ri;
                let mut wo = wo;
                let mut buf = vec![0u8; 8192];
                loop {
                    let n = match tokio::io::AsyncReadExt::read(&mut ri, &mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    if tokio::io::AsyncWriteExt::write_all(&mut wo, &buf[..n]).await.is_err() {
                        break;
                    }
                    relay1.write().await.bytes_out += n as u64;
                }
            });

            let h2 = tokio::spawn(async move {
                let mut ro = ro;
                let mut wi = wi;
                let mut buf = vec![0u8; 8192];
                loop {
                    let n = match tokio::io::AsyncReadExt::read(&mut ro, &mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    if tokio::io::AsyncWriteExt::write_all(&mut wi, &buf[..n]).await.is_err() {
                        break;
                    }
                    relay2.write().await.bytes_in += n as u64;
                }
            });

            let _ = tokio::join!(h1, h2);
            relay_inner.write().await.connections = relay_inner.write().await.connections.saturating_sub(1);
        });
    }
}

/// UDP relay: simple datagram counter.
async fn run_udp_relay(
    relay_id: &str,
    local_addr: &str,
    relay: Arc<RwLock<Relay>>,
) -> Result<()> {
    let socket = UdpSocket::bind(local_addr).await?;
    log::info!("Relay {} UDP socket bound to {}", relay_id, local_addr);

    let mut buf = vec![0u8; 65535];
    loop {
        if let Ok((len, _)) = socket.recv_from(&mut buf).await {
            relay.write().await.bytes_in += len as u64;
        }
    }
}

/// Listener relay: persistent listener that forwards to relay channel.
async fn run_listener_relay(
    relay_id: &str,
    listen_addr: &str,
    relay: Arc<RwLock<Relay>>,
) -> Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    log::info!("Relay {} listener on {}", relay_id, listen_addr);

    loop {
        let (mut stream, _) = listener.accept().await?;
        relay.write().await.connections += 1;
        let relay = relay.clone();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 8192];
            while let Ok(n) = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await {
                if n == 0 {
                    break;
                }
                let data = buf[..n].to_vec();
                if let Some(tx) = relay.read().await.data_tx.as_ref() {
                    let _ = tx.send(data).await;
                }
            }
            relay.write().await.connections -= 1;
        });
    }
}

impl Default for SocatRelay {
    fn default() -> Self {
        Self::new()
    }
}
