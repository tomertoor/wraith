//! Relay handler — manages relay connections and bridges data between streams.
//!
//! For a relay chain A → B → C:
//! - When A receives a connection on a listener, it:
//!   1. Opens a Yamux stream to B
//!   2. Sends RelayOpen on channel 5 to B
//!   3. Bridges data on channel 3 between the TCP connection and the Yamux stream
//! - B does the same to C, etc.

use crate::tunnel::channel::RELAY_DATA;
use futures::{AsyncReadExt, AsyncWriteExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncReadExt as TokioAsyncReadExt;
use tokio::io::AsyncWriteExt as TokioAsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use yamux::Stream;

/// State for an active relay connection
#[derive(Debug)]
pub struct RelayConnection {
    pub relay_id: String,
    pub connect_addr: String,
}

/// Manages relay connections for an agent or relay
pub struct RelayHandler {
    /// Active relay connections keyed by relay_id
    connections: Arc<RwLock<HashMap<String, RelayConnection>>>,
}

impl RelayHandler {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Handle RelayOpen from an upstream relay — connect to target and return the connection
    pub async fn handle_relay_open(
        &self,
        relay_id: String,
        connect_addr: String,
    ) -> anyhow::Result<TcpStream> {
        log::info!("Relay {}: opening connection to {}", relay_id, connect_addr);

        // Connect to the target
        let target = TcpStream::connect(&connect_addr).await?;
        log::info!("Relay {}: connected to target {}", relay_id, connect_addr);

        Ok(target)
    }

    /// Bridge data between the upstream Yamux stream and the target TCP connection.
    /// Sequential copy: target -> stream, then stream -> target.
    pub async fn bridge_relay_data(
        &self,
        relay_id: String,
        mut stream: Stream,
        target: TcpStream,
    ) -> anyhow::Result<()> {
        let (mut target_read, mut target_write) = tokio::io::split(target);

        // Target -> Stream (forward data as-is, not framed - the stream handles framing)
        let mut buf = vec![0u8; 8192];
        loop {
            match TokioAsyncReadExt::read(&mut target_read, &mut buf).await {
                Ok(0) => {
                    log::debug!("Relay {}: target EOF", relay_id);
                    break;
                }
                Ok(n) => {
                    if let Err(e) = AsyncWriteExt::write_all(&mut stream, &buf[..n]).await {
                        log::warn!("Relay {}: stream write error: {}", relay_id, e);
                        break;
                    }
                    if let Err(e) = AsyncWriteExt::flush(&mut stream).await {
                        log::warn!("Relay {}: stream flush error: {}", relay_id, e);
                        break;
                    }
                }
                Err(e) => {
                    log::warn!("Relay {}: target read error: {}", relay_id, e);
                    break;
                }
            }
        }

        // Stream -> Target (parse frame header and extract data)
        let mut buf = vec![0u8; 8192 + 8];
        loop {
            match AsyncReadExt::read(&mut stream, &mut buf).await {
                Ok(0) => {
                    log::debug!("Relay {}: stream EOF", relay_id);
                    break;
                }
                Ok(n) => {
                    if n > 8 {
                        let channel_id = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
                        let len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;

                        if channel_id == RELAY_DATA && n >= 8 + len {
                            if let Err(e) = TokioAsyncWriteExt::write_all(&mut target_write, &buf[8..8+len]).await {
                                log::warn!("Relay {}: target write error: {}", relay_id, e);
                                break;
                            }
                            if let Err(e) = TokioAsyncWriteExt::flush(&mut target_write).await {
                                log::warn!("Relay {}: target flush error: {}", relay_id, e);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Relay {}: stream read error: {}", relay_id, e);
                    break;
                }
            }
        }

        // Clean up
        self.connections.write().await.remove(&relay_id);
        log::info!("Relay {}: connection closed", relay_id);

        Ok(())
    }

    /// Handle incoming connection from a listener - bridge to upstream
    /// This is used when the relay itself acts as a proxy listener
    pub async fn handle_incoming(
        &self,
        relay_id: String,
        connect_addr: String,
        tcp_stream: TcpStream,
        upstream_stream: Stream,
    ) -> anyhow::Result<()> {
        log::info!("Relay {}: handling incoming connection to {}", relay_id, connect_addr);

        // Connect to target
        let target = TcpStream::connect(&connect_addr).await?;

        // Bridge between upstream stream and target
        self.bridge_relay_data(relay_id, upstream_stream, target).await?;

        Ok(())
    }
}

impl Default for RelayHandler {
    fn default() -> Self {
        Self::new()
    }
}
