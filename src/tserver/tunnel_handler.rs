//! Tunnel handler — manages forward connections and bridges data between streams.
//!
//! For a forward chain A → B → C:
//! - When A receives a connection on a listener, it:
//!   1. Opens a Yamux stream to B
//!   2. Sends RelayOpen on channel 5 to B
//!   3. Bridges data on channel 3 (FORWARD_DATA) between the TCP connection and the Yamux stream
//! - B does the same to C, etc.

use crate::tunnel::channel::FORWARD_DATA;
use futures::{AsyncReadExt, AsyncWriteExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncReadExt as TokioAsyncReadExt;
use tokio::io::AsyncWriteExt as TokioAsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use yamux::Stream;

/// State for an active forward connection
#[derive(Debug)]
pub struct TunnelConnection {
    pub tunnel_id: String,
    pub connect_addr: String,
}

/// Manages tunnel connections for an agent or tunnel server
pub struct TunnelHandler {
    /// Active forward connections keyed by tunnel_id
    connections: Arc<RwLock<HashMap<String, TunnelConnection>>>,
}

impl TunnelHandler {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Handle RelayOpen from an upstream — connect to target and return the connection
    pub async fn handle_forward_open(
        &self,
        tunnel_id: String,
        connect_addr: String,
    ) -> anyhow::Result<TcpStream> {
        log::info!("Forward {}: opening connection to {}", tunnel_id, connect_addr);

        // Connect to the target
        let target = TcpStream::connect(&connect_addr).await?;
        log::info!("Forward {}: connected to target {}", tunnel_id, connect_addr);

        Ok(target)
    }

    /// Bridge data between the upstream Yamux stream and the target TCP connection.
    /// Sequential copy: target -> stream, then stream -> target.
    pub async fn bridge_forward_data(
        &self,
        tunnel_id: String,
        mut stream: Stream,
        target: TcpStream,
    ) -> anyhow::Result<()> {
        let (mut target_read, mut target_write) = tokio::io::split(target);

        // Target -> Stream (forward data as-is, not framed - the stream handles framing)
        let mut buf = vec![0u8; 8192];
        loop {
            match TokioAsyncReadExt::read(&mut target_read, &mut buf).await {
                Ok(0) => {
                    log::debug!("Forward {}: target EOF", tunnel_id);
                    break;
                }
                Ok(n) => {
                    if let Err(e) = AsyncWriteExt::write_all(&mut stream, &buf[..n]).await {
                        log::warn!("Forward {}: stream write error: {}", tunnel_id, e);
                        break;
                    }
                    if let Err(e) = AsyncWriteExt::flush(&mut stream).await {
                        log::warn!("Forward {}: stream flush error: {}", tunnel_id, e);
                        break;
                    }
                }
                Err(e) => {
                    log::warn!("Forward {}: target read error: {}", tunnel_id, e);
                    break;
                }
            }
        }

        // Stream -> Target (parse frame header and extract data)
        let mut buf = vec![0u8; 8192 + 8];
        loop {
            match AsyncReadExt::read(&mut stream, &mut buf).await {
                Ok(0) => {
                    log::debug!("Forward {}: stream EOF", tunnel_id);
                    break;
                }
                Ok(n) => {
                    if n > 8 {
                        let channel_id = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
                        let len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;

                        if channel_id == FORWARD_DATA && n >= 8 + len {
                            if let Err(e) = TokioAsyncWriteExt::write_all(&mut target_write, &buf[8..8+len]).await {
                                log::warn!("Forward {}: target write error: {}", tunnel_id, e);
                                break;
                            }
                            if let Err(e) = TokioAsyncWriteExt::flush(&mut target_write).await {
                                log::warn!("Forward {}: target flush error: {}", tunnel_id, e);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Forward {}: stream read error: {}", tunnel_id, e);
                    break;
                }
            }
        }

        // Clean up
        self.connections.write().await.remove(&tunnel_id);
        log::info!("Forward {}: connection closed", tunnel_id);

        Ok(())
    }

    /// Handle incoming connection from a listener - bridge to upstream
    /// This is used when the tunnel server itself acts as a proxy listener
    pub async fn handle_incoming(
        &self,
        tunnel_id: String,
        connect_addr: String,
        tcp_stream: TcpStream,
        upstream_stream: Stream,
    ) -> anyhow::Result<()> {
        log::info!("Forward {}: handling incoming connection to {}", tunnel_id, connect_addr);

        // Connect to target
        let target = TcpStream::connect(&connect_addr).await?;

        // Bridge between upstream stream and target
        self.bridge_forward_data(tunnel_id, upstream_stream, target).await?;

        Ok(())
    }
}

impl Default for TunnelHandler {
    fn default() -> Self {
        Self::new()
    }
}
