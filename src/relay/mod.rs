use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::Duration;
use std::collections::HashMap;
use serde::Serialize;

const RELAY_BUFFER_SIZE: usize = 8192;

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

    async fn run_udp_listen(&self, listen_addr: String, mut shutdown: oneshot::Receiver<()>) -> Result<()> {
        let inbound = Arc::new(UdpSocket::bind(&listen_addr).await?);
        let mut buf = [0u8; RELAY_BUFFER_SIZE];

        // Track sessions keyed by source SocketAddr
        let sessions: Arc<DashMap<std::net::SocketAddr, SessionState>> = Arc::new(DashMap::new());
        let forward_addr = format!("{}:{}", self.config.forward.host, self.config.forward.port);
        let active = Arc::clone(&self.active);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    info!("[RELAY-UDP] Shutdown signal received");
                    active.store(false, Ordering::SeqCst);
                    // Drop sessions to close them
                    drop(sessions);
                    break;
                }
                // Receive data from clients on inbound socket
                result = inbound.recv_from(&mut buf) => {
                    let (n, src) = match result {
                        Ok(x) => x,
                        Err(e) => {
                            warn!("[RELAY-UDP] recv error: {}", e);
                            continue;
                        }
                    };
                    debug!("[RELAY-UDP] Received {} bytes from {}", n, src);

                    // Check if session exists
                    if !sessions.contains_key(&src) {
                        // Create new session
                        let session = match SessionState::new(src, forward_addr.clone(), Arc::clone(&active)).await {
                            Ok(s) => s,
                            Err(e) => {
                                error!("[RELAY-UDP] Failed to create session for {}: {}", src, e);
                                continue;
                            }
                        };
                        sessions.insert(src, session);
                    }

                    // Send data to session's forward channel
                    if let Some(session) = sessions.get(&src) {
                        let data = buf[..n].to_vec();
                        session.send_to_forward(data);
                    } else {
                        sessions.remove(&src);
                    }
                }
            }
        }
        Ok(())
    }
}

// ============================================================================
// Session State for UDP Relays
// ============================================================================

struct SessionState {
    // Channel to send data to the session task
    forward_tx: mpsc::Sender<Vec<u8>>,
    // Channel to receive responses from session task
    response_rx: mpsc::Receiver<Vec<u8>>,
    // Flag to signal session closure
    close_rx: Arc<AtomicBool>,
}

impl SessionState {
    async fn new(src: std::net::SocketAddr, forward_addr: String, active: Arc<AtomicBool>) -> Result<Self> {
        let (forward_tx, forward_rx) = mpsc::channel(100);
        let (response_tx, response_rx) = mpsc::channel(100);
        let close_rx = Arc::new(AtomicBool::new(false));

        let close_flag = Arc::clone(&close_rx);
        // Spawn the session task
        tokio::spawn(session_task(
            src,
            forward_addr,
            forward_rx,
            response_tx,
            close_flag,
            active,
        ));

        Ok(Self {
            forward_tx,
            response_rx,
            close_rx,
        })
    }

    fn send_to_forward(&self, data: Vec<u8>) {
        // Use try_send to avoid blocking
        let _ = self.forward_tx.try_send(data);
    }

    fn close(&self) {
        self.close_rx.store(true, Ordering::SeqCst);
    }
}

async fn session_task(
    src: std::net::SocketAddr,
    forward_addr: String,
    mut forward_rx: mpsc::Receiver<Vec<u8>>,
    response_tx: mpsc::Sender<Vec<u8>>,
    close_rx: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
) {
    // Socket to send to destination and receive responses
    let to_dest = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            error!("[RELAY-UDP] Failed to bind to_dest socket: {}", e);
            return;
        }
    };

    // Socket to send back to original source
    let to_src = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            error!("[RELAY-UDP] Failed to bind to_src socket: {}", e);
            return;
        }
    };

    let mut buf_to_dest = [0u8; RELAY_BUFFER_SIZE];
    let mut buf_to_src = [0u8; RELAY_BUFFER_SIZE];

    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if close_rx.load(Ordering::SeqCst) {
                    break;
                }
                continue;
            }
            // Receive data to forward from run_udp_listen
            data = forward_rx.recv() => {
                if let Some(d) = data {
                    if to_dest.send_to(&d, &forward_addr).await.is_err() {
                        break;
                    }
                }
            }
            // Receive response from destination
            result = to_dest.recv_from(&mut buf_to_dest) => {
                let (n, _) = match result {
                    Ok(x) => x,
                    Err(e) => {
                        warn!("[RELAY-UDP] to_dest recv error: {}", e);
                        continue;
                    }
                };
                // Send response back to original source
                if to_src.send_to(&buf_to_dest[..n], src).await.is_err() {
                    break;
                }
            }
            // Also receive any data from original source on to_src socket
            result = to_src.recv_from(&mut buf_to_src) => {
                let (n, _) = match result {
                    Ok(x) => x,
                    Err(e) => {
                        // This socket is primarily for sending, so recv errors are less critical
                        warn!("[RELAY-UDP] to_src recv error: {}", e);
                        continue;
                    }
                };
                // Forward to destination
                if to_dest.send_to(&buf_to_src[..n], &forward_addr).await.is_err() {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(300)) => {
                if !active.load(Ordering::SeqCst) {
                    break;
                }
            }
        }
    }
}

// ============================================================================
// Generic Bidirectional Relay Functions
// ============================================================================

async fn relay_bidirectional<R1, W1, R2, W2>(
    mut a_read: R1,
    mut a_write: W1,
    mut b_read: R2,
    mut b_write: W2,
    active: Arc<AtomicBool>,
) -> Result<()>
where
    R1: AsyncRead + Unpin,
    W1: AsyncWrite + Unpin,
    R2: AsyncRead + Unpin,
    W2: AsyncWrite + Unpin,
{
    let active_a = Arc::clone(&active);
    let active_b = Arc::clone(&active);

    let _ = tokio::join! {
        async move {
            let mut buf = [0u8; RELAY_BUFFER_SIZE];
            while active_a.load(Ordering::SeqCst) {
                match a_read.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if b_write.write_all(&buf[..n]).await.is_err() { break; }
                    }
                }
            }
        },
        async move {
            let mut buf = [0u8; RELAY_BUFFER_SIZE];
            while active_b.load(Ordering::SeqCst) {
                match b_read.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if a_write.write_all(&buf[..n]).await.is_err() { break; }
                    }
                }
            }
        },
    };
    Ok(())
}

async fn relay_stream_to_datagram(
    forward_addr: &str,
    inbound: TcpStream,
    active: Arc<AtomicBool>,
) -> Result<()> {
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
            let mut buf = [0u8; RELAY_BUFFER_SIZE];
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
            let mut resp = [0u8; RELAY_BUFFER_SIZE];
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

async fn relay_connection(config: &RelayConfig, inbound: TcpStream, active: Arc<AtomicBool>) -> Result<()> {
    let forward_addr = format!("{}:{}", config.forward.host, config.forward.port);

    match config.forward.protocol {
        Transport::Tcp => {
            let outbound = TcpStream::connect(&forward_addr).await?;
            let (a_read, a_write) = tokio::io::split(inbound);
            let (b_read, b_write) = tokio::io::split(outbound);
            relay_bidirectional(b_read, b_write, a_read, a_write, active).await?;
        }
        Transport::Udp => {
            relay_stream_to_datagram(&forward_addr, inbound, active).await?;
        }
    }
    Ok(())
}
