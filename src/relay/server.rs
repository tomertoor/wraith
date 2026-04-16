//! Relay server — accepts agent connections and manages tunnels.

use crate::proto::*;
use crate::tunnel::channel::{TUNNEL_CONTROL, TUNNEL_DATA, RELAY_DATA, RELAY_CONTROL};
use crate::tunnel::multiplex::encode_frame;
use crate::wraith::Config;
use futures::stream::StreamExt;
use prost::Message;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, RwLock};
use tokio_util::compat::TokioAsyncReadCompatExt;
use yamux::{Config as YamuxConfig, Connection, ConnectionError, Mode};

/// Active tunnel state on the relay side.
#[derive(Debug)]
pub struct Tunnel {
    pub tunnel_id: String,
    pub agent_ip: String,
    pub relay_ip: String,
    pub netmask: String,
    pub channels: TunnelChannels,
}

#[derive(Debug, Clone)]
pub struct TunnelChannels {
    pub control_tx: mpsc::Sender<Vec<u8>>,
    pub data_tx: mpsc::Sender<Vec<u8>>,
}

/// Relay server — listens for agent connections and manages tunnels.
pub struct RelayServer {
    config: Config,
    tunnels: Arc<RwLock<HashMap<String, Tunnel>>>,
    /// Channel to send inbound tunnel data packets for TUN writing.
    /// The relay main loop consumes this and writes to the TUN device.
    tun_write_tx: Option<mpsc::Sender<(String, Vec<u8>)>>,
}

impl RelayServer {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            tunnels: Arc::new(RwLock::new(HashMap::new())),
            tun_write_tx: None,
        }
    }

    /// Set the TUN write channel. The relay main loop should read from this
    /// channel and write packets to the TUN device.
    pub fn set_tun_write_channel(&mut self, tx: mpsc::Sender<(String, Vec<u8>)>) {
        self.tun_write_tx = Some(tx);
    }

    /// Start the relay server listening on the configured address.
    pub async fn run(&mut self, listen_addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(listen_addr).await?;
        log::info!("Relay server listening on {}", listen_addr);

        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    log::info!("Agent connected from {}", peer);
                    let tunnels = self.tunnels.clone();
                    let tun_write_tx = self.tun_write_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_agent(stream, tunnels, tun_write_tx).await {
                            log::error!("Agent handler error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    log::error!("Accept error: {}", e);
                }
            }
        }
    }
}

/// Handle a single agent connection.
async fn handle_agent(
    stream: tokio::net::TcpStream,
    tunnels: Arc<RwLock<HashMap<String, Tunnel>>>,
    tun_write_tx: Option<mpsc::Sender<(String, Vec<u8>)>>,
) -> anyhow::Result<()> {
    let config = YamuxConfig::default();
    let compat = stream.compat();
    let mut conn = Connection::new(compat, config, Mode::Server);

    // Create channel dispatchers
    let (ctrl_tx, _ctrl_rx) = mpsc::channel::<Vec<u8>>(1024);
    let (data_tx, _data_rx) = mpsc::channel::<Vec<u8>>(1024);

    // Create a Stream over the connection
    let mut event_stream = ConnectionEventStream { conn: &mut conn };

    loop {
        match event_stream.next().await {
            Some(Ok(mut stream)) => {
                let tunnels = tunnels.clone();
                let ctrl_tx = ctrl_tx.clone();
                let data_tx = data_tx.clone();
                let tun_write_tx = tun_write_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_stream(&mut stream, &tunnels, &ctrl_tx, &data_tx, tun_write_tx).await {
                        log::debug!("stream handler error: {}", e);
                    }
                });
            }
            Some(Err(ConnectionError::Closed)) => break,
            Some(Err(e)) => {
                log::debug!("yamux event error: {}", e);
            }
            None => break,
        }
    }

    Ok(())
}

/// Stream wrapper around yamux Connection's poll_next_inbound.
struct ConnectionEventStream<'a> {
    conn: &'a mut Connection<tokio_util::compat::Compat<tokio::net::TcpStream>>,
}

impl<'a> futures::stream::Stream for ConnectionEventStream<'a> {
    type Item = Result<yamux::Stream, ConnectionError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        let mut pinned_conn = unsafe {
            let conn_ref = &mut *self.conn as *mut Connection<tokio_util::compat::Compat<tokio::net::TcpStream>>;
            Pin::new_unchecked(&mut *conn_ref)
        };
        pinned_conn.poll_next_inbound(cx)
    }
}

/// Handle an inbound yamux stream — read header and dispatch by channel.
async fn handle_stream(
    stream: &mut yamux::Stream,
    tunnels: &Arc<RwLock<HashMap<String, Tunnel>>>,
    ctrl_tx: &mpsc::Sender<Vec<u8>>,
    data_tx: &mpsc::Sender<Vec<u8>>,
    tun_write_tx: Option<mpsc::Sender<(String, Vec<u8>)>>,
) -> anyhow::Result<()> {
    let mut header = [0u8; 8];
    futures::io::AsyncReadExt::read_exact(stream, &mut header).await?;
    let channel_id = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;

    let mut data = vec![0u8; len];
    futures::io::AsyncReadExt::read_exact(stream, &mut data).await?;

    match channel_id {
        TUNNEL_CONTROL => {
            if let Ok(open) = TunnelOpen::decode(&data[..]) {
                log::info!("TunnelOpen: {}", open.tunnel_id);

                let tunnel = Tunnel {
                    tunnel_id: open.tunnel_id.clone(),
                    agent_ip: open.agent_tun_ip.clone(),
                    relay_ip: open.relay_tun_ip.clone(),
                    netmask: open.tunnel_netmask.clone(),
                    channels: TunnelChannels {
                        control_tx: ctrl_tx.clone(),
                        data_tx: data_tx.clone(),
                    },
                };
                tunnels.write().await.insert(open.tunnel_id.clone(), tunnel);

                // Send ack on this same stream
                let ack = TunnelOpenAck {
                    tunnel_id: open.tunnel_id,
                    success: true,
                    error_msg: String::new(),
                    assigned_ip: String::new(),
                };
                let mut buf = Vec::new();
                ack.encode(&mut buf)?;
                let framed = encode_frame(TUNNEL_CONTROL, &buf);
                futures::io::AsyncWriteExt::write_all(stream, &framed).await?;
            } else if let Ok(close) = TunnelClose::decode(&data[..]) {
                log::info!("TunnelClose: {} ({})", close.tunnel_id, close.reason);
                tunnels.write().await.remove(&close.tunnel_id);
            } else if let Ok(stats) = TunnelStats::decode(&data[..]) {
                log::debug!("TunnelStats: {}", stats.tunnel_id);
            } else if let Ok(ka) = Keepalive::decode(&data[..]) {
                let ack = KeepaliveAck {
                    tunnel_id: ka.tunnel_id,
                    timestamp: ka.timestamp,
                };
                let mut buf = Vec::new();
                ack.encode(&mut buf)?;
                let framed = encode_frame(TUNNEL_CONTROL, &buf);
                futures::io::AsyncWriteExt::write_all(stream, &framed).await?;
            }
        }
        TUNNEL_DATA => {
            // Forward tunnel data to the relay TUN device via the write channel.
            if let Ok(tunnel_data) = TunnelData::decode(&data[..]) {
                log::trace!(
                    "Tunnel data for {}: {} bytes",
                    tunnel_data.tunnel_id,
                    tunnel_data.payload.len()
                );
                if let Some(ref tx) = tun_write_tx {
                    if tx.send((tunnel_data.tunnel_id, tunnel_data.payload)).await.is_err() {
                        log::warn!("TUN write channel closed");
                    }
                }
            }
        }
        RELAY_DATA => {
            // Relay data channel — forward to the target service.
            log::trace!("Relay data channel received {} bytes", data.len());
            // TODO: Implement relay data forwarding — decode RelayData proto and
            // forward to the target TCP/UDP socket.
        }
        RELAY_CONTROL => {
            // Relay control channel — handle RelayOpen, RelayClose, etc.
            if let Ok(open) = RelayOpen::decode(&data[..]) {
                log::info!("RelayOpen: {} mode={} local={} remote={}", open.relay_id, open.mode, open.local_addr, open.remote_addr);
                let ack = RelayOpenAck {
                    relay_id: open.relay_id.clone(),
                    success: true,
                    error_msg: String::new(),
                    bound_addr: open.local_addr.clone(),
                };
                let mut buf = Vec::new();
                ack.encode(&mut buf)?;
                let framed = encode_frame(RELAY_CONTROL, &buf);
                futures::io::AsyncWriteExt::write_all(stream, &framed).await?;
            } else if let Ok(close) = RelayClose::decode(&data[..]) {
                log::info!("RelayClose: {} ({})", close.relay_id, close.reason);
            } else if let Ok(stats) = RelayStats::decode(&data[..]) {
                log::debug!("RelayStats: {}", stats.relay_id);
            } else {
                log::warn!("Unknown relay control message, {} bytes", data.len());
            }
        }
        _ => {
            log::warn!("Unknown channel ID: {}", channel_id);
        }
    }

    Ok(())
}
