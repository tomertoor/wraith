use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::time::Duration;
use std::collections::HashMap;
use serde::Serialize;

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
pub struct RelayEndpoint {
    pub host: String,
    pub port: u16,
    pub protocol: Transport,
}

impl RelayEndpoint {
    pub fn new(host: String, port: u16, protocol: Transport) -> Self {
        Self { host, port, protocol }
    }

    pub fn from_str(host: &str, port: u16, protocol: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            protocol: Transport::from_str(protocol),
        }
    }
}

#[derive(Clone)]
pub struct RelayConfig {
    pub listen: RelayEndpoint,
    pub forward: RelayEndpoint,
}

impl RelayConfig {
    pub fn new(listen: RelayEndpoint, forward: RelayEndpoint) -> Self {
        Self { listen, forward }
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
        let relay = Arc::new(ProtocolRelay::new(config.clone()));
        let id = relay.id().to_string();
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
    id: String,
    config: RelayConfig,
    active: Arc<AtomicBool>,
}

impl ProtocolRelay {
    pub fn new(config: RelayConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            config,
            active: Arc::new(AtomicBool::new(true)),
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl RelayTrait for ProtocolRelay {
    fn id(&self) -> &str {
        &self.id
    }

    fn config(&self) -> &RelayConfig {
        &self.config
    }

    fn is_active(&self) -> bool {
        self.is_active()
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

    fn to_relay_info(&self) -> RelayInfo {
        RelayInfo {
            relay_id: self.id.clone(),
            listen_host: self.config.listen.host.clone(),
            listen_port: self.config.listen.port,
            forward_host: self.config.forward.host.clone(),
            forward_port: self.config.forward.port,
            active: self.is_active(),
            protocol: format!("{}->{}", self.config.listen.protocol, self.config.forward.protocol),
        }
    }
}

impl ProtocolRelay {
    async fn run_tcp_listen(&self, listen_addr: String, mut shutdown: oneshot::Receiver<()>) -> Result<()> {
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
                                let _ = handle_tcp_connection_static(&config, inbound, active).await;
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

    async fn handle_tcp_connection(&self, inbound: TcpStream) -> Result<()> {
        let forward_addr = format!("{}:{}", self.config.forward.host, self.config.forward.port);
        let active = Arc::clone(&self.active);

        match self.config.forward.protocol {
            Transport::Tcp => {
                handle_tcp_to_tcp(&forward_addr, inbound, active).await?;
            }
            Transport::Udp => {
                relay_tcp_to_udp_static(&forward_addr, inbound, active).await?;
            }
        }
        Ok(())
    }
}

async fn handle_tcp_to_tcp(forward_addr: &str, inbound: TcpStream, active: Arc<AtomicBool>) -> Result<()> {
    let outbound = TcpStream::connect(forward_addr).await?;

    let (mut ri, mut wi) = tokio::io::split(inbound);
    let (mut ro, mut wo) = tokio::io::split(outbound);

    let active_in = Arc::clone(&active);
    let active_out = Arc::clone(&active);

    let _ = tokio::join! {
        async move {
            let mut buf = [0u8; 8192];
            while active_in.load(Ordering::SeqCst) {
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
            while active_out.load(Ordering::SeqCst) {
                match ro.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if wi.write_all(&buf[..n]).await.is_err() { break; }
                    }
                }
            }
        },
    };
    Ok(())
}

async fn relay_tcp_to_udp_static(forward_addr: &str, inbound: TcpStream, active: Arc<AtomicBool>) -> Result<()> {
    let outbound = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    let (ri, wi) = inbound.into_split();
    tokio::pin!(ri);
    tokio::pin!(wi);

    let active_in = Arc::clone(&active);
    let active_out = Arc::clone(&active);
    let forward_addr_owned = forward_addr.to_string();
    let outbound_in = Arc::clone(&outbound);
    let outbound_out = Arc::clone(&outbound);

    let _ = tokio::join! {
        async move {
            let mut buf = [0u8; 8192];
            loop {
                if !active_in.load(Ordering::SeqCst) { break; }
                match tokio::time::timeout(Duration::from_secs(1), ri.as_mut().read(&mut buf)).await {
                    Ok(Ok(0)) | Ok(Err(_)) => break,
                    Ok(Ok(n)) => {
                        if outbound_in.send_to(&buf[..n], &forward_addr_owned).await.is_err() { break; }
                    }
                    Err(_) => continue,
                }
            }
        },
        async move {
            let mut resp = [0u8; 8192];
            loop {
                if !active_out.load(Ordering::SeqCst) { break; }
                match tokio::time::timeout(Duration::from_secs(2), outbound_out.recv_from(&mut resp)).await {
                    Ok(Ok((m, _))) => {
                        if wi.as_mut().write_all(&resp[..m]).await.is_err() { break; }
                    }
                    Ok(Err(_)) => break,
                    Err(_) => continue,
                }
            }
        },
    };
    Ok(())
}

impl ProtocolRelay {
    async fn run_udp_listen(&self, listen_addr: String, mut shutdown: oneshot::Receiver<()>) -> Result<()> {
        let inbound = Arc::new(UdpSocket::bind(&listen_addr).await?);
        let mut buf = [0u8; 8192];

        // Track TCP connections per source address (for UDP->TCP only)
        let tcp_connections: Arc<DashMap<std::net::SocketAddr, Arc<Mutex<TcpStream>>>> = Arc::new(DashMap::new());
        let forward_addr = format!("{}:{}", self.config.forward.host, self.config.forward.port);
        let active = Arc::clone(&self.active);
        let is_tcp_forward = self.config.forward.protocol == Transport::Tcp;

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    info!("[RELAY-UDP] Shutdown signal received");
                    active.store(false, Ordering::SeqCst);
                    // Close all TCP connections
                    if is_tcp_forward {
                        for entry in tcp_connections.iter() {
                            let conn = entry.value();
                            let mut c = conn.lock().await;
                            let _ = c.shutdown().await;
                        }
                    }
                    break;
                }
                result = inbound.recv_from(&mut buf) => {
                    let (n, src) = match result {
                        Ok(x) => x,
                        Err(e) => {
                            warn!("[RELAY-UDP] recv error: {}", e);
                            continue;
                        }
                    };
                    debug!("[RELAY-UDP] Received {} bytes from {}", n, src);
                    let data = buf[..n].to_vec();
                    let active = Arc::clone(&active);
                    let forward_addr = forward_addr.clone();

                    if is_tcp_forward {
                        // UDP -> TCP relay
                        let tcp_connections = Arc::clone(&tcp_connections);
                        tokio::spawn(async move {
                            handle_udp_to_tcp(&forward_addr, src, data, active, tcp_connections).await;
                        });
                    } else {
                        // UDP -> UDP relay
                        tokio::spawn(async move {
                            handle_udp_to_udp(&forward_addr, src, data, active).await;
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

async fn handle_udp_to_udp(forward_addr: &str, src: std::net::SocketAddr, data: Vec<u8>, active: Arc<AtomicBool>) {
    let outbound = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            error!("[RELAY-UDP->UDP] Failed to bind outbound socket: {}", e);
            return;
        }
    };

    if outbound.send_to(&data, forward_addr).await.is_err() {
        return;
    }

    let mut resp = [0u8; 8192];
    match tokio::time::timeout(Duration::from_secs(2), outbound.recv_from(&mut resp)).await {
        Ok(Ok((m, _))) => {
            if let Ok(listener) = UdpSocket::bind("0.0.0.0:0").await {
                let _ = listener.send_to(&resp[..m], src).await;
            }
        }
        _ => {}
    }
}

async fn handle_udp_to_tcp(
    forward_addr: &str,
    src: std::net::SocketAddr,
    data: Vec<u8>,
    active: Arc<AtomicBool>,
    tcp_connections: Arc<DashMap<std::net::SocketAddr, Arc<Mutex<TcpStream>>>>,
) {
    // Try to get existing TCP connection for this source, or create new one
    let conn = match tcp_connections.get(&src) {
        Some(conn) => {
            // Reuse existing connection (clone the Arc)
            Arc::clone(conn.value())
        }
        None => {
            // Create new TCP connection
            match TcpStream::connect(forward_addr).await {
                Ok(c) => {
                    let arc_c = Arc::new(Mutex::new(c));
                    tcp_connections.insert(src, Arc::clone(&arc_c));
                    arc_c
                }
                Err(e) => {
                    error!("[RELAY-UDP->TCP] Failed to connect to forward: {}", e);
                    return;
                }
            }
        }
    };

    // Lock the connection, send data, read response
    let mut outbound = conn.lock().await;

    // Send data to TCP
    if outbound.write_all(&data).await.is_err() {
        drop(outbound);
        tcp_connections.remove(&src);
        return;
    }

    // Read response and send back to UDP
    let mut resp = [0u8; 8192];
    match tokio::time::timeout(Duration::from_secs(2), outbound.read(&mut resp)).await {
        Ok(Ok(0)) | Ok(Err(_)) => {
            drop(outbound);
            tcp_connections.remove(&src);
        }
        Ok(Ok(n)) => {
            drop(outbound);
            // Send response back to UDP source
            if let Ok(listener) = UdpSocket::bind("0.0.0.0:0").await {
                let _ = listener.send_to(&resp[..n], src).await;
            }
        }
        Err(_) => {
            // Timeout, but connection is still valid
        }
    }
}

async fn handle_tcp_connection_static(config: &RelayConfig, inbound: TcpStream, active: Arc<AtomicBool>) -> Result<()> {
    let forward_addr = format!("{}:{}", config.forward.host, config.forward.port);

    match config.forward.protocol {
        Transport::Tcp => {
            handle_tcp_to_tcp(&forward_addr, inbound, active).await?;
        }
        Transport::Udp => {
            relay_tcp_to_udp_static(&forward_addr, inbound, active).await?;
        }
    }
    Ok(())
}