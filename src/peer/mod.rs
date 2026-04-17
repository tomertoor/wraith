//! Wraith symmetric peer connection module.
//!
//! Provides peer-to-peer connections via `wraith listen` and `wraith connect`.
//! Both sides use Yamux streams with channels:
//! - 0 (TUNNEL_DATA): raw IP packets for VPN tunnel
//! - 1 (TUNNEL_CONTROL): tunnel open/close/keepalive messages
//! - 3 (FORWARD_DATA): raw bytes for port-forward traffic
//! - 5 (FORWARD_CONTROL): forward open/close messages

pub mod handshake;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::Result;
use futures::io::AsyncReadExt as FutAsyncReadExt; // For yamux Stream
use futures::AsyncWriteExt as FutAsyncWriteExt;
use prost::Message;
use tokio::io::{AsyncReadExt as TokioAsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use yamux::{Config, Connection, ConnectionError, Mode};

use crate::proto::*;
use crate::relay::SocatRelay;
use crate::tunnel::channel::{FORWARD_CONTROL, FORWARD_DATA, TUNNEL_CONTROL, TUNNEL_DATA};
use crate::tunnel::multiplex::encode_frame;

use handshake::create_peer_info;

/// Protocol version prefix for peer connections.
const PEER_PROTOCOL_V2: &[u8] = b"WRAITH2\n";

/// Represents the state of an active tunnel.
#[derive(Debug, Clone)]
pub struct TunnelState {
    pub tunnel_id: String,
    pub tun_ip: String,
    pub netmask: u8,
    pub bytes_out: u64,
}

/// Represents the state of an active port forward.
#[derive(Debug, Clone)]
pub struct ForwardState {
    pub relay_id: String,
    pub mode: String,
    pub local_addr: String,
    pub remote_addr: String,
}

/// A symmetric peer connection.
///
/// After connection establishment, both sides run the same code
/// to handle inbound Yamux streams and dispatch to tunnel/forward handlers.
#[derive(Debug)]
pub struct PeerConnection {
    pub node_id: String,
    pub addr: String,
    pub is_initiator: bool,
    pub peer_info: Option<handshake::PeerInfo>,
    pub tunnels: HashMap<String, TunnelState>,
    pub forwards: HashMap<String, ForwardState>,
    /// Local TUN IP for tunnels created on this node.
    tun_ip: Option<String>,
    /// Local netmask for TUN interfaces.
    netmask: Option<u8>,
    yamux: Option<Connection<Compat<TcpStream>>>,
    /// Port forwarding manager for relay-style TCP/UDP bridging.
    port_forward: SocatRelay,
}

/// Helper future for polling Yamux inbound streams.
struct PollInbound<'a> {
    conn: &'a mut Connection<Compat<TcpStream>>,
}

impl Future for PollInbound<'_> {
    type Output = Option<Result<yamux::Stream, ConnectionError>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: self is Pin<&mut Self> and Connection is !Unpin.
        // We need to re-pin the inner Connection.
        let this = unsafe { self.get_unchecked_mut() };
        let conn_ref = this.conn as *mut Connection<Compat<TcpStream>>;
        let mut pinned_conn = unsafe { Pin::new_unchecked(&mut *conn_ref) };
        pinned_conn.poll_next_inbound(cx)
    }
}

/// Helper future for opening Yamux outbound streams.
struct PollOutbound<'a> {
    conn: &'a mut Connection<Compat<TcpStream>>,
}

impl Future for PollOutbound<'_> {
    type Output = Result<yamux::Stream, ConnectionError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: self is Pin<&mut Self> and Connection is !Unpin.
        let this = unsafe { self.get_unchecked_mut() };
        let conn_ref = this.conn as *mut Connection<Compat<TcpStream>>;
        let mut pinned_conn = unsafe { Pin::new_unchecked(&mut *conn_ref) };
        pinned_conn.poll_new_outbound(cx)
    }
}

impl PeerConnection {
    /// Connect to a remote peer (outbound connection).
    ///
    /// Sends the WRAITH2 version prefix, creates a Yamux client connection,
    /// then exchanges PeerInfo via outbound/inbound Yamux streams.
    pub async fn connect(addr: &str, tun_ip: &str, netmask: u8) -> Result<Self> {
        let node_id = uuid::Uuid::new_v4().to_string();
        let mut stream = TcpStream::connect(addr).await?;

        // Send WRAITH2 protocol version prefix
        stream.write_all(PEER_PROTOCOL_V2).await?;
        stream.flush().await?;

        // Create Yamux connection as client
        let compat = stream.compat();
        let config = Config::default();
        let yamux = Connection::new(compat, config, Mode::Client);

        let mut conn = Self {
            node_id: node_id.clone(),
            addr: addr.to_string(),
            is_initiator: true,
            peer_info: None,
            tunnels: HashMap::new(),
            forwards: HashMap::new(),
            tun_ip: Some(tun_ip.to_string()),
            netmask: Some(netmask),
            yamux: Some(yamux),
            port_forward: SocatRelay::new(),
        };

        // Exchange PeerInfo: open outbound stream, send our info, read peer info
        conn.exchange_peer_info().await?;

        Ok(conn)
    }

    /// Accept an inbound peer connection.
    ///
    /// Reads the WRAITH2 version prefix, creates a Yamux server connection,
    /// then exchanges PeerInfo with the connecting peer.
    pub async fn accept(stream: TcpStream, is_initiator: bool, tun_ip: Option<&str>, netmask: Option<u8>) -> Result<Self> {
        let node_id = uuid::Uuid::new_v4().to_string();
        let mut stream = stream;

        // Read protocol version prefix
        let mut prefix = [0u8; 8];
        stream.read_exact(&mut prefix).await?;

        if &prefix != PEER_PROTOCOL_V2 {
            anyhow::bail!("Unknown protocol prefix: {:?}", &prefix);
        }

        // Create Yamux connection as server
        let compat = stream.compat();
        let config = Config::default();
        let yamux = Connection::new(compat, config, Mode::Server);

        let mut conn = Self {
            node_id: node_id.clone(),
            addr: "unknown".to_string(),
            is_initiator,
            peer_info: None,
            tunnels: HashMap::new(),
            forwards: HashMap::new(),
            tun_ip: tun_ip.map(String::from),
            netmask,
            yamux: Some(yamux),
            port_forward: SocatRelay::new(),
        };

        // Exchange PeerInfo with the initiator
        conn.exchange_peer_info().await?;

        Ok(conn)
    }

    /// Exchange PeerInfo with the connected peer.
    ///
    /// Opens an outbound stream and sends our PeerInfo, then reads the peer's
    /// PeerInfo from an inbound stream. On the initiating side, we also read
    /// the peer's outbound stream (which is our inbound).
    async fn exchange_peer_info(&mut self) -> Result<()> {
        let yamux_conn = self.yamux.as_mut().ok_or_else(|| anyhow::anyhow!("connection not initialized"))?;

        // Open outbound stream and send our PeerInfo
        let mut outbound = PollOutbound { conn: yamux_conn }.await?;

        let peer_info = create_peer_info(&self.node_id, self.tun_ip.as_deref().unwrap_or(""));
        let mut buf = Vec::new();
        peer_info.encode(&mut buf)?;
        outbound.write_all(&buf).await?;
        outbound.flush().await?;

        // Read peer's PeerInfo from inbound stream
        if let Some(Ok(mut inbound)) = (PollInbound { conn: yamux_conn }).await {
            let mut info_data = Vec::new();
            inbound.read_to_end(&mut info_data).await?;
            if let Ok(info) = PeerInfo::decode(&info_data[..]) {
                self.peer_info = Some(handshake::PeerInfo {
                    node_id: info.node_id,
                    version: info.version,
                    tun_capable: info.tun_capable,
                    tun_ip: info.tun_ip,
                    supports_relay: info.supports_relay,
                    supports_nexus: info.supports_nexus,
                    options: info.options,
                });
                log::info!("Peer exchange complete: {:?}", self.peer_info);
            }
        }

        Ok(())
    }

    /// Main run loop — handle inbound Yamux streams from the peer.
    ///
    /// Each inbound stream is a new logical channel. We read an 8-byte header
    /// (channel_id + length) followed by the payload. The payload format
    /// depends on the channel:
    /// - Channel 0 (TUNNEL_DATA): TunnelData protobuf — raw IP packet to forward
    /// - Channel 1 (TUNNEL_CONTROL): TunnelControl wrapper with open/close/keepalive
    /// - Channel 3 (FORWARD_DATA): raw bytes for port-forward relay
    /// - Channel 5 (FORWARD_CONTROL): RelayControl wrapper with open/close
    pub async fn run(&mut self) -> Result<()> {
        let mut yamux = self.yamux.take().ok_or_else(|| anyhow::anyhow!("already running"))?;

        loop {
            match (PollInbound { conn: &mut yamux }).await {
                Some(Ok(mut stream)) => {
                    log::debug!("Received inbound Yamux stream from peer");

                    // Read 8-byte header: [4-byte channel_id][4-byte length]
                    let mut header = [0u8; 8];
                    futures::io::AsyncReadExt::read_exact(&mut stream, &mut header).await?;
                    let channel_id = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
                    let len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;

                    // Read payload
                    let mut data = vec![0u8; len];
                    futures::io::AsyncReadExt::read_exact(&mut stream, &mut data).await?;

                    // RelayControl messages need to start the actual listener before
                    // we spawn the stream handler, so handle them inline here.
                    if channel_id == FORWARD_CONTROL {
                        let relay_msg = match RelayControl::decode(&data[..]) {
                            Ok(m) => m,
                            Err(e) => {
                                log::warn!("Failed to decode RelayControl: {}", e);
                                continue;
                            }
                        };

                        use crate::proto::relay_control::Msg;
                        match relay_msg.msg {
                            Some(Msg::Open(open)) => {
                                log::info!("RelayOpen from peer: {} mode={} local={} remote={}",
                                    open.relay_id, open.mode, open.local_addr, open.remote_addr);

                                // Start the actual TCP/UDP listener via SocatRelay
                                if let Err(e) = self.port_forward.start_relay(
                                    &open.relay_id,
                                    &open.mode,
                                    &open.local_addr,
                                    &open.remote_addr,
                                ).await {
                                    log::error!("Failed to start relay {}: {}", open.relay_id, e);
                                    // Send failure ack
                                    let ack = RelayOpenAck {
                                        relay_id: open.relay_id.clone(),
                                        success: false,
                                        error_msg: e.to_string(),
                                        bound_addr: String::new(),
                                    };
                                    let mut buf = Vec::new();
                                    ack.encode(&mut buf).ok();
                                    let framed = encode_frame(FORWARD_CONTROL, &buf);
                                    futures::io::AsyncWriteExt::write_all(&mut stream, &framed).await.ok();
                                    futures::io::AsyncWriteExt::flush(&mut stream).await.ok();
                                    continue;
                                }

                                self.forwards.insert(open.relay_id.clone(), ForwardState {
                                    relay_id: open.relay_id.clone(),
                                    mode: open.mode.clone(),
                                    local_addr: open.local_addr.clone(),
                                    remote_addr: open.remote_addr.clone(),
                                });

                                // Send success ack
                                let ack = RelayOpenAck {
                                    relay_id: open.relay_id,
                                    success: true,
                                    error_msg: String::new(),
                                    bound_addr: open.local_addr.clone(),
                                };
                                let mut buf = Vec::new();
                                if ack.encode(&mut buf).is_ok() {
                                    let framed = encode_frame(FORWARD_CONTROL, &buf);
                                    futures::io::AsyncWriteExt::write_all(&mut stream, &framed).await.ok();
                                    futures::io::AsyncWriteExt::flush(&mut stream).await.ok();
                                }
                                continue;
                            }
                            Some(Msg::Close(close)) => {
                                log::info!("RelayClose from peer: {} ({})", close.relay_id, close.reason);
                                self.port_forward.stop_relay(&close.relay_id).await.ok();
                                self.forwards.remove(&close.relay_id);
                                // No ack needed for close
                                continue;
                            }
                            _ => {
                                // Stats, list, etc — ignore for now
                                continue;
                            }
                        }
                    }

                    // For other channels, spawn handler task (don't block loop)
                    let tunnels = self.tunnels.clone();
                    let forwards = self.forwards.clone();
                    let tun_ip = self.tun_ip.clone();
                    let netmask = self.netmask;
                    let is_initiator = self.is_initiator;

                    tokio::spawn(async move {
                        // Errors are logged but don't kill the connection
                        if let Err(e) = Self::handle_stream(
                            channel_id,
                            &mut stream,
                            data,
                            tunnels,
                            forwards,
                            tun_ip,
                            netmask,
                            is_initiator,
                        ).await {
                            log::warn!("Stream handler error on channel {}: {}", channel_id, e);
                        }
                    });
                }
                Some(Err(e)) => {
                    // Yamux protocol error — log and keep running
                    log::warn!("Yamux stream error: {}", e);
                }
                None => {
                    log::info!("Yamux connection closed by peer");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle an inbound stream from the peer, dispatched by channel ID.
    async fn handle_stream(
        channel_id: u32,
        stream: &mut yamux::Stream,
        data: Vec<u8>,
        mut tunnels: HashMap<String, TunnelState>,
        mut forwards: HashMap<String, ForwardState>,
        tun_ip: Option<String>,
        netmask: Option<u8>,
        _is_initiator: bool,
    ) -> Result<()> {
        match channel_id {
            TUNNEL_DATA => {
                // Channel 0: tunnel data — raw IP packets from the peer
                if let Ok(tunnel_data) = TunnelData::decode(&data[..]) {
                    log::trace!(
                        "TUNNEL_DATA for {}: {} bytes",
                        tunnel_data.tunnel_id,
                        tunnel_data.payload.len()
                    );
                    // TODO: Write to local TUN device here.
                    // For now, just log. The TUN write would go to the PyWraith-owned TUN
                    // via a channel or the direct mode JSONL interface.
                    if let Some(tunnel) = tunnels.get_mut(&tunnel_data.tunnel_id) {
                        tunnel.bytes_out += tunnel_data.payload.len() as u64;
                    }
                }
            }
            TUNNEL_CONTROL => {
                // Channel 1: tunnel control messages (wrapped in TunnelControl)
                let ctrl = match TunnelControl::decode(&data[..]) {
                    Ok(m) => m,
                    Err(e) => {
                        log::warn!("Failed to decode TunnelControl: {}", e);
                        return Ok(());
                    }
                };

                use crate::proto::tunnel_control::Msg;
                match ctrl.msg {
                    Some(Msg::Open(open)) => {
                        log::info!("TunnelOpen from peer: {} (tun_ip={}, netmask={})",
                            open.tunnel_id, open.relay_tun_ip, open.tunnel_netmask);

                        // Record the tunnel
                        let tun_ip_str = tun_ip.clone().unwrap_or_else(|| open.agent_tun_ip.clone());
                        let nm = netmask.unwrap_or(24);
                        tunnels.insert(open.tunnel_id.clone(), TunnelState {
                            tunnel_id: open.tunnel_id.clone(),
                            tun_ip: open.agent_tun_ip.clone(),
                            netmask: nm,
                            bytes_out: 0,
                        });

                        // Send ack back on this stream
                        let ack = TunnelOpenAck {
                            tunnel_id: open.tunnel_id,
                            success: true,
                            error_msg: String::new(),
                            assigned_ip: tun_ip_str,
                        };
                        let mut buf = Vec::new();
                        ack.encode(&mut buf)?;
                        let framed = encode_frame(TUNNEL_CONTROL, &buf);
                        futures::io::AsyncWriteExt::write_all(stream, &framed).await?;
                        futures::io::AsyncWriteExt::flush(stream).await?;

                    }
                    Some(Msg::Close(close)) => {
                        log::info!("TunnelClose from peer: {} ({})", close.tunnel_id, close.reason);
                        tunnels.remove(&close.tunnel_id);
                    }
                    Some(Msg::Keepalive(ka)) => {
                        log::trace!("Keepalive from peer for tunnel {}", ka.tunnel_id);
                        let ack = KeepaliveAck {
                            tunnel_id: ka.tunnel_id,
                            timestamp: ka.timestamp,
                        };
                        let mut buf = Vec::new();
                        ack.encode(&mut buf)?;
                        let framed = encode_frame(TUNNEL_CONTROL, &buf);
                        futures::io::AsyncWriteExt::write_all(stream, &framed).await?;
                        futures::io::AsyncWriteExt::flush(stream).await?;
                    }
                    _ => {
                        // Stats, StatsRequest, KeepaliveAck — ignore
                    }
                }
            }
            FORWARD_DATA => {
                // Channel 3: relay data — raw bytes for port-forward
                log::trace!("FORWARD_DATA from peer: {} bytes", data.len());
                // TODO: Forward to the target TCP/UDP socket
            }
            FORWARD_CONTROL => {
                // Channel 5: relay control messages (always handled inline in run() now)
                // This case should not be reached, but handle gracefully anyway
                let ctrl = match RelayControl::decode(&data[..]) {
                    Ok(m) => m,
                    Err(e) => {
                        log::warn!("Failed to decode RelayControl in handle_stream: {}", e);
                        return Ok(());
                    }
                };

                use crate::proto::relay_control::Msg;
                match ctrl.msg {
                    Some(Msg::Open(open)) => {
                        forwards.insert(open.relay_id.clone(), ForwardState {
                            relay_id: open.relay_id.clone(),
                            mode: open.mode.clone(),
                            local_addr: open.local_addr.clone(),
                            remote_addr: open.remote_addr.clone(),
                        });
                        let ack = RelayOpenAck {
                            relay_id: open.relay_id,
                            success: true,
                            error_msg: String::new(),
                            bound_addr: String::new(),
                        };
                        let mut buf = Vec::new();
                        ack.encode(&mut buf)?;
                        let framed = encode_frame(FORWARD_CONTROL, &buf);
                        futures::io::AsyncWriteExt::write_all(stream, &framed).await?;
                        futures::io::AsyncWriteExt::flush(stream).await?;
                    }
                    Some(Msg::Close(close)) => {
                        forwards.remove(&close.relay_id);
                    }
                    _ => {
                        // Stats, List, etc — ignore
                    }
                }
            }
            _ => {
                log::warn!("Unknown channel ID from peer: {}", channel_id);
            }
        }

        Ok(())
    }

    /// Open a tunnel with this peer.
    ///
    /// Sends a TunnelOpen message on channel 1 (TUNNEL_CONTROL) and waits
    /// for the ack on the same stream.
    pub async fn open_tunnel(&mut self, tunnel_id: &str, tun_ip: &str, netmask: u8) -> anyhow::Result<()> {
        let yamux_conn = self.yamux.as_mut().ok_or_else(|| anyhow::anyhow!("connection not initialized"))?;
        let mut stream = PollOutbound { conn: yamux_conn }.await?;

        // Wrap TunnelOpen in TunnelControl (matching what PyWraith sends)
        let tunnel_msg = crate::proto::TunnelControl {
            msg: Some(crate::proto::tunnel_control::Msg::Open(TunnelOpen {
                tunnel_id: tunnel_id.to_string(),
                relay_tun_ip: tun_ip.to_string(),
                agent_tun_ip: tun_ip.to_string(),
                tunnel_netmask: netmask.to_string(),
                routes: String::new(),
                encryption: "chacha20".to_string(),
                nonce: vec![0u8; 12],
            })),
        };

        let mut buf = Vec::new();
        prost::Message::encode(&tunnel_msg, &mut buf)?;
        let frame = encode_frame(TUNNEL_CONTROL, &buf);
        stream.write_all(&frame).await?;
        stream.flush().await?;

        // Read the ack from the stream (also wrapped in TunnelControl)
        let mut header = [0u8; 8];
        futures::io::AsyncReadExt::read_exact(&mut stream, &mut header).await?;
        let ack_len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let mut ack_data = vec![0u8; ack_len];
        futures::io::AsyncReadExt::read_exact(&mut stream, &mut ack_data).await?;

        let ack_msg = crate::proto::TunnelControl::decode(&ack_data[..])
            .map_err(|e| anyhow::anyhow!("failed to decode TunnelControl ack: {}", e))?;

        let ack = match ack_msg.msg {
            Some(crate::proto::tunnel_control::Msg::OpenAck(a)) => a,
            _ => anyhow::bail!("unexpected ack type"),
        };

        if !ack.success {
            anyhow::bail!("peer rejected tunnel: {}", ack.error_msg);
        }

        self.tunnels.insert(tunnel_id.to_string(), TunnelState {
            tunnel_id: tunnel_id.to_string(),
            tun_ip: tun_ip.to_string(),
            netmask,
            bytes_out: 0,
        });

        log::info!("Tunnel {} opened (assigned_ip={})", tunnel_id, ack.assigned_ip);
        Ok(())
    }

    /// Close a tunnel with this peer.
    pub async fn close_tunnel(&mut self, tunnel_id: &str) -> anyhow::Result<()> {
        let yamux_conn = self.yamux.as_mut().ok_or_else(|| anyhow::anyhow!("connection not initialized"))?;
        let mut stream = PollOutbound { conn: yamux_conn }.await?;

        let tunnel_msg = crate::proto::TunnelControl {
            msg: Some(crate::proto::tunnel_control::Msg::Close(TunnelClose {
                tunnel_id: tunnel_id.to_string(),
                reason: "requested".to_string(),
            })),
        };

        let mut buf = Vec::new();
        prost::Message::encode(&tunnel_msg, &mut buf)?;
        let frame = encode_frame(TUNNEL_CONTROL, &buf);
        stream.write_all(&frame).await?;
        stream.flush().await?;

        self.tunnels.remove(tunnel_id);
        log::info!("Tunnel {} closed", tunnel_id);
        Ok(())
    }

    /// Open a forward with this peer.
    pub async fn open_forward(&mut self, relay_id: &str, mode: &str, local_addr: &str, remote_addr: &str) -> anyhow::Result<()> {
        let yamux_conn = self.yamux.as_mut().ok_or_else(|| anyhow::anyhow!("connection not initialized"))?;
        let mut stream = PollOutbound { conn: yamux_conn }.await?;

        // Wrap RelayOpen in RelayControl (matching what PyWraith sends)
        let relay_msg = crate::proto::RelayControl {
            msg: Some(crate::proto::relay_control::Msg::Open(RelayOpen {
                relay_id: relay_id.to_string(),
                mode: mode.to_string(),
                local_addr: local_addr.to_string(),
                remote_addr: remote_addr.to_string(),
                file_path: String::new(),
                command: String::new(),
                timeout: 300,
                keepalive: true,
            })),
        };

        let mut buf = Vec::new();
        prost::Message::encode(&relay_msg, &mut buf)?;
        let frame = encode_frame(FORWARD_CONTROL, &buf);
        stream.write_all(&frame).await?;
        stream.flush().await?;

        // Read ack (also wrapped in RelayControl)
        let mut header = [0u8; 8];
        futures::io::AsyncReadExt::read_exact(&mut stream, &mut header).await?;
        let ack_len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let mut ack_data = vec![0u8; ack_len];
        futures::io::AsyncReadExt::read_exact(&mut stream, &mut ack_data).await?;

        let ack_msg = crate::proto::RelayControl::decode(&ack_data[..])
            .map_err(|e| anyhow::anyhow!("failed to decode RelayControl ack: {}", e))?;

        let ack = match ack_msg.msg {
            Some(crate::proto::relay_control::Msg::OpenAck(a)) => a,
            _ => anyhow::bail!("unexpected ack type"),
        };

        if !ack.success {
            anyhow::bail!("peer rejected relay: {}", ack.error_msg);
        }

        self.forwards.insert(relay_id.to_string(), ForwardState {
            relay_id: relay_id.to_string(),
            mode: mode.to_string(),
            local_addr: local_addr.to_string(),
            remote_addr: remote_addr.to_string(),
        });

        log::info!("Relay {} opened (bound={})", relay_id, ack.bound_addr);
        Ok(())
    }

    /// Close a forward with this peer.
    pub async fn close_forward(&mut self, relay_id: &str) -> anyhow::Result<()> {
        let yamux_conn = self.yamux.as_mut().ok_or_else(|| anyhow::anyhow!("connection not initialized"))?;
        let mut stream = PollOutbound { conn: yamux_conn }.await?;

        let relay_msg = crate::proto::RelayControl {
            msg: Some(crate::proto::relay_control::Msg::Close(RelayClose {
                relay_id: relay_id.to_string(),
                reason: "requested".to_string(),
            })),
        };

        let mut buf = Vec::new();
        prost::Message::encode(&relay_msg, &mut buf)?;
        let frame = encode_frame(FORWARD_CONTROL, &buf);
        stream.write_all(&frame).await?;
        stream.flush().await?;

        self.forwards.remove(relay_id);
        log::info!("Relay {} closed", relay_id);
        Ok(())
    }

    pub fn list_tunnels(&self) -> Vec<&TunnelState> {
        self.tunnels.values().collect()
    }

    pub fn list_forwards(&self) -> Vec<&ForwardState> {
        self.forwards.values().collect()
    }
}

/// Listen for incoming peer connections.
pub async fn run_listen(addr: &str, _tun_name: Option<&str>, tunnel_ip: &str) -> Result<()> {
    use tokio::net::TcpListener;

    // Parse tunnel_ip "10.8.0.1/24"
    let parts: Vec<&str> = tunnel_ip.split('/').collect();
    let ip = parts[0];
    let netmask = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(24);

    let listener = TcpListener::bind(addr).await?;
    log::info!("Listening for peers on {}", addr);

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        log::info!("Accepted peer connection from {}", peer_addr);

        let tun_ip = Some(ip.to_string());
        let nm = Some(netmask);

        let mut conn = PeerConnection::accept(stream, false, tun_ip.as_deref(), nm).await?;
        log::info!("Peer handshake complete with {}", conn.peer_info.as_ref().map(|p| p.node_id.as_str()).unwrap_or("?"));

        tokio::spawn(async move {
            if let Err(e) = conn.run().await {
                log::error!("Peer connection error: {}", e);
            }
        });
    }
}

/// Connect to a remote peer.
pub async fn run_connect(addr: &str, _tun_name: Option<&str>, tunnel_ip: &str) -> Result<()> {
    // Parse tunnel_ip "10.8.0.1/24"
    let parts: Vec<&str> = tunnel_ip.split('/').collect();
    let ip = parts[0];
    let netmask = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(24);

    loop {
        log::info!("Connecting to peer at {}...", addr);
        match PeerConnection::connect(addr, ip, netmask).await {
            Ok(mut conn) => {
                log::info!("Connected to peer {} (peer={:?})",
                    addr, conn.peer_info.as_ref().map(|p| p.node_id.as_str()));
                // run() blocks until the connection closes
                // On disconnect, it returns Ok(()) — loop and reconnect
                if let Err(e) = conn.run().await {
                    log::error!("Peer connection error: {}", e);
                }
                log::info!("Peer disconnected. Reconnecting in 5s...");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
            Err(e) => {
                log::error!("Failed to connect to {}: {}. Retrying in 5s...", addr, e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    }
}
