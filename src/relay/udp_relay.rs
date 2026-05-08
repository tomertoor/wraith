use anyhow::Result;
use dashmap::DashMap;
use log::{debug, error, warn};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::Duration;

use crate::relay::RELAY_BUFFER_SIZE;

use super::ProtocolRelay;

// ============================================================================
// Session State for UDP Relays
// ============================================================================

pub(super) struct SessionState {
    // Channel to send data to the session task
    forward_tx: mpsc::Sender<Vec<u8>>,
    // Channel to receive responses from session task
    #[allow(dead_code)]
    response_rx: mpsc::Receiver<Vec<u8>>,
    // Flag to signal session closure
    close_rx: Arc<AtomicBool>,
}

impl SessionState {
    pub(super) async fn new(src: SocketAddr, forward_addr: String, active: Arc<AtomicBool>) -> Result<Self> {
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

    pub(super) fn send_to_forward(&self, data: Vec<u8>) {
        // Use try_send to avoid blocking
        let _ = self.forward_tx.try_send(data);
    }

    #[allow(dead_code)]
    pub(super) fn close(&self) {
        self.close_rx.store(true, Ordering::SeqCst);
    }
}

pub(super) async fn session_task(
    src: SocketAddr,
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
    // Drop response_tx to signal the receiver that the session is done
    drop(response_tx);
}

// ============================================================================
// ProtocolRelay UDP listener
// ============================================================================

impl ProtocolRelay {
    pub(crate) async fn run_udp_listen(&self, listen_addr: String, mut shutdown: tokio::sync::oneshot::Receiver<()>) -> Result<()> {
        let inbound = Arc::new(UdpSocket::bind(&listen_addr).await?);
        let mut buf = [0u8; RELAY_BUFFER_SIZE];

        // Track sessions keyed by source SocketAddr
        let sessions: Arc<DashMap<SocketAddr, SessionState>> = Arc::new(DashMap::new());
        let forward_addr = format!("{}:{}", self.config.forward.host, self.config.forward.port);
        let active = Arc::clone(&self.active);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    log::info!("[RELAY-UDP] Shutdown signal received");
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
