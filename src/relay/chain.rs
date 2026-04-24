use crate::relay::{RelayConfig, RelayTrait, Transport};
use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::oneshot;
use tokio::time::Duration;

pub struct RelayChain {
    id: String,
    hops: Vec<RelayConfig>,
    active: Arc<AtomicBool>,
}

impl RelayChain {
    pub fn new(hops: Vec<RelayConfig>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            hops,
            active: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn config(&self) -> &[RelayConfig] {
        &self.hops
    }
}

#[async_trait]
impl RelayTrait for RelayChain {
    fn id(&self) -> &str {
        &self.id
    }

    fn config(&self) -> &RelayConfig {
        // For compatibility — return first hop config
        &self.hops[0]
    }

    fn is_active(&self) -> bool {
        self.is_active()
    }

    async fn start_relay(
        self: Arc<Self>,
        mut shutdown: oneshot::Receiver<()>,
    ) -> Result<()> {
        if self.hops.len() < 2 {
            anyhow::bail!("RelayChain requires at least 2 hops");
        }

        info!(
            "[CHAIN] Starting relay chain {} with {} hops",
            self.id,
            self.hops.len()
        );

        // For each hop, we need to know where to forward:
        // hop[0] forwards to hop[1]'s listen addr
        // hop[1] forwards to hop[2]'s listen addr
        // ...
        // Last hop forwards to final forward_host:forward_port
        let mut forward_addrs: Vec<String> = Vec::new();
        for i in 0..self.hops.len() - 1 {
            let next = &self.hops[i + 1];
            forward_addrs.push(format!("{}:{}", next.listen_host, next.listen_port));
        }
        // Last hop forwards to its configured forward address
        let last_hop = self.hops.last().unwrap();
        forward_addrs.push(format!("{}:{}", last_hop.forward_host, last_hop.forward_port));

        // Spawn relay tasks for each hop
        let active = Arc::clone(&self.active);
        let hop_count = self.hops.len();
        let mut hop_configs: Vec<(String, u16, String, u16, Transport)> = Vec::with_capacity(hop_count);
        for hop in &self.hops {
            hop_configs.push((hop.listen_host.clone(), hop.listen_port, hop.forward_host.clone(), hop.forward_port, hop.protocol.clone()));
        }

        let handles = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (completion_tx, mut completion_rx) = tokio::sync::mpsc::channel::<(usize, Result<()>)>(hop_count);

        for (i, hop_config) in hop_configs.into_iter().enumerate() {
            let forward_addr = forward_addrs[i].clone();
            let (listen_host, listen_port, _, _, protocol) = hop_config;
            let active_clone = Arc::clone(&active);
            let ct = completion_tx.clone();

            let handle = tokio::spawn(async move {
                let result = match protocol {
                    Transport::Tcp => run_tcp_hop(&listen_host, listen_port, &forward_addr, active_clone.clone()).await,
                    Transport::Udp => run_udp_hop(&listen_host, listen_port, &forward_addr, active_clone.clone()).await,
                };
                let _ = ct.send((i, result)).await;
            });
            handles.lock().unwrap().push(handle);
        }

        // Wait for shutdown or all hops to complete
        tokio::select! {
            _ = &mut shutdown => {
                info!("[CHAIN] Shutdown signal received");
                active.store(false, Ordering::SeqCst);
            }
            _ = completion_rx.recv() => {
                // One hop completed — check if all done
                if completion_rx.is_closed() {
                    info!("[CHAIN] All hops completed");
                }
            }
        }

        // Wait for all tasks
        let join_handles: Vec<_> = handles.lock().unwrap().drain(..).collect();
        for handle in join_handles {
            let _ = handle.await;
        }

        info!("[CHAIN] Relay chain {} stopped", self.id);
        Ok(())
    }

    fn to_relay_info(&self) -> crate::relay::RelayInfo {
        // Summarize chain as first-hop info (chain is atomic unit)
        crate::relay::RelayInfo {
            relay_id: self.id.clone(),
            listen_host: self.hops.first().map(|h| h.listen_host.clone()).unwrap_or_default(),
            listen_port: self.hops.first().map(|h| h.listen_port).unwrap_or(0),
            forward_host: self.hops.last().map(|h| h.forward_host.clone()).unwrap_or_default(),
            forward_port: self.hops.last().map(|h| h.forward_port).unwrap_or(0),
            active: self.is_active(),
            protocol: format!(
                "chain[{}]",
                self.hops.iter().map(|h| h.protocol.to_string()).collect::<Vec<_>>().join("->")
            ),
        }
    }
}

async fn run_tcp_hop(listen_host: &str, listen_port: u16, forward_addr: &str, active: Arc<AtomicBool>) -> Result<()> {
    let listen_addr = format!("{}:{}", listen_host, listen_port);
    info!("[CHAIN-TCP] Hop listening on {}", listen_addr);
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;

    loop {
        if !active.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            result = listener.accept() => {
                let (mut inbound, addr) = result?;
                debug!("[CHAIN-TCP] Accepted from {} (forwarding to {})", addr, forward_addr);
                let forward = forward_addr.to_string();
                let active_clone = Arc::clone(&active);
                tokio::spawn(async move {
                    if let Ok(mut outbound) = tokio::net::TcpStream::connect(&forward).await {
                        let (mut ri, mut wi) = inbound.split();
                        let (mut ro, mut wo) = outbound.split();
                        let local_active1 = Arc::clone(&active_clone);
                        let local_active2 = Arc::clone(&active_clone);
                        let _ = tokio::join! {
                            async move {
                                let mut buf = [0u8; 8192];
                                while local_active1.load(Ordering::SeqCst) {
                                    match ri.read(&mut buf).await {
                                        Ok(0) | Err(_) => break,
                                        Ok(n) => {
                                            if wo.write_all(&buf[..n]).await.is_err() { break; }
                                        }
                                    }
                                }
                            },
                            async move {
                                let mut buf = [0u8; 8192];
                                while local_active2.load(Ordering::SeqCst) {
                                    match ro.read(&mut buf).await {
                                        Ok(0) | Err(_) => break,
                                        Ok(n) => {
                                            if wi.write_all(&buf[..n]).await.is_err() { break; }
                                        }
                                    }
                                }
                            },
                        };
                    }
                });
            }
            else => break,
        }
    }
    Ok(())
}

async fn run_udp_hop(listen_host: &str, listen_port: u16, forward_addr: &str, active: Arc<AtomicBool>) -> Result<()> {
    let listen_addr = format!("{}:{}", listen_host, listen_port);
    info!("[CHAIN-UDP] Hop listening on {}", listen_addr);
    let inbound = Arc::new(UdpSocket::bind(&listen_addr).await?);

    let mut buf = [0u8; 8192];
    loop {
        if !active.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            result = inbound.recv_from(&mut buf) => {
                let (n, src) = match result {
                    Ok(x) => x,
                    Err(e) => { warn!("[CHAIN-UDP] recv error: {}", e); continue; }
                };
                let fwd = forward_addr.to_string();
                let ib = Arc::clone(&inbound);
                let active_clone = Arc::clone(&active);

                tokio::spawn(async move {
                    let outbound = match UdpSocket::bind("0.0.0.0:0").await {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    if outbound.send_to(&buf[..n], &fwd).await.is_err() { return; }
                    let mut resp = [0u8; 8192];
                    let result = tokio::time::timeout(Duration::from_millis(500), outbound.recv_from(&mut resp)).await;
                    if let Ok(Ok((m, _))) = result {
                        let _ = ib.send_to(&resp[..m], src).await;
                    }
                });
            }
            else => break,
        }
    }
    Ok(())
}