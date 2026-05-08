use anyhow::Result;
use futures::future::poll_fn;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};

use crate::commands::agent::AgentCommands;
use crate::commands::relay::RelayCommands;
use crate::proto::wraith::WraithMessage;
use crate::router::Router;
use crate::wraith::state::WraithState;
use super::peer::PeerSession;
use super::message_loop::run_peer_message_loop;

/// Events emitted by the TunnelManager when peers are added or removed.
#[derive(Debug)]
pub enum PeerEvent {
    Added {
        wraith_id: String,
        hostname: String,
        sender: mpsc::Sender<WraithMessage>,
    },
    Removed {
        wraith_id: String,
    },
}

/// Spawn a background task to drive a yamux Connection.
/// Inbound streams are forwarded through the returned receiver so callers can
/// accept them. Returns (connection handle for opening outbound streams, receiver
/// for inbound streams).
pub fn spawn_yamux_driver_with_inbound(
    conn: yamux::Connection<Compat<TcpStream>>,
) -> (
    Arc<tokio::sync::Mutex<yamux::Connection<Compat<TcpStream>>>>,
    mpsc::Receiver<yamux::Stream>,
) {
    let conn_handle = Arc::new(tokio::sync::Mutex::new(conn));
    let conn_handle_for_spawn = Arc::clone(&conn_handle);
    let (inbound_tx, inbound_rx) = mpsc::channel(16);

    tokio::spawn(async move {
        let mut c = conn_handle_for_spawn.lock().await;
        loop {
            match poll_fn(|cx| Pin::new(&mut c).poll_next_inbound(cx)).await {
                Some(Ok(stream)) => {
                    if inbound_tx.send(stream).await.is_err() {
                        info!("Inbound stream receiver dropped, stopping driver");
                        break;
                    }
                }
                Some(Err(e)) => {
                    warn!("Yamux connection error: {}", e);
                    break;
                }
                None => {
                    info!("Yamux connection closed");
                    break;
                }
            }
        }
        info!("Yamux driver finished");
    });

    (conn_handle, inbound_rx)
}

/// Spawn a yamux driver that accepts inbound streams but drops them.
/// Use this for the client side (which only opens outbound streams).
pub fn spawn_yamux_driver(
    conn: yamux::Connection<Compat<TcpStream>>,
) -> Arc<tokio::sync::Mutex<yamux::Connection<Compat<TcpStream>>>> {
    let conn_handle = Arc::new(tokio::sync::Mutex::new(conn));
    let conn_handle_for_spawn = Arc::clone(&conn_handle);

    tokio::spawn(async move {
        let mut c = conn_handle_for_spawn.lock().await;
        loop {
            match poll_fn(|cx| Pin::new(&mut c).poll_next_inbound(cx)).await {
                Some(Ok(_)) => { /* inbound stream — not needed on client side */ }
                Some(Err(e)) => {
                    warn!("Yamux connection error: {}", e);
                    break;
                }
                None => {
                    info!("Yamux connection closed");
                    break;
                }
            }
        }
        info!("Yamux driver finished");
    });

    conn_handle
}

pub struct TunnelManager {
    sessions: Arc<RwLock<HashMap<String, PeerSession>>>,
    peer_event_tx: mpsc::Sender<PeerEvent>,
    router: Router,
    state: Arc<Mutex<WraithState>>,
}

impl TunnelManager {
    /// Creates a TunnelManager with the given shared state and peer event channel.
    /// Command handlers are set after construction via set_commands() to break the
    /// circular dependency between TunnelManager and command types.
    pub fn new(state: Arc<Mutex<WraithState>>, peer_event_tx: mpsc::Sender<PeerEvent>) -> Self {
        let relay_manager = Arc::clone(&state.lock().expect("state lock poisoned").relay_manager);
        let router = Router::new(
            Arc::clone(&state),
            Arc::new(Mutex::new(RelayCommands::new_without_tunnel(relay_manager))),
            Arc::new(Mutex::new(AgentCommands::new_without_tunnel())),
        );
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            peer_event_tx,
            router,
            state,
        }
    }

    /// Set the command handlers for routing messages.
    /// Required to be called after new() due to the circular dependency:
    /// TunnelManager needs commands, but commands need Arc<TunnelManager>.
    pub fn set_commands(&self, relay_commands: RelayCommands, agent_commands: AgentCommands) {
        self.router.set_commands(relay_commands, agent_commands);
    }

    fn notify_peer_added(&self, wraith_id: &str, hostname: &str, sender: &mpsc::Sender<WraithMessage>) {
        let _ = self.peer_event_tx.try_send(PeerEvent::Added {
            wraith_id: wraith_id.to_string(),
            hostname: hostname.to_string(),
            sender: sender.clone(),
        });
    }

    fn notify_peer_removed(&self, wraith_id: &str) {
        let _ = self.peer_event_tx.try_send(PeerEvent::Removed {
            wraith_id: wraith_id.to_string(),
        });
    }

    pub async fn add_session(&self, wraith_id: String, session: PeerSession) {
        let hostname = session.hostname.clone();
        let command_tx = session.command_tx.clone();
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(wraith_id.clone(), session);
        }
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            state.add_peer(wraith_id.clone(), hostname.clone(), command_tx.clone());
        }
        self.notify_peer_added(&wraith_id, &hostname, &command_tx);
        info!("Added peer session: {}", wraith_id);
    }

    pub async fn remove_session(&self, wraith_id: &str) {
        {
            let mut sessions = self.sessions.write().await;
            sessions.remove(wraith_id);
        }
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            state.remove_peer(wraith_id);
        }
        self.notify_peer_removed(wraith_id);
        info!("Removed peer session: {}", wraith_id);
    }

    pub async fn get_session(&self, wraith_id: &str) -> Option<PeerSession> {
        let sessions = self.sessions.read().await;
        sessions.get(wraith_id).cloned()
    }

    pub async fn list_sessions(&self) -> Vec<(String, String)> {
        let sessions = self.sessions.read().await;
        sessions.iter()
            .map(|(id, s)| (id.clone(), s.hostname.clone()))
            .collect()
    }

    pub async fn get_all_session_ids(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
    }

    /// Route a message: check target_wraith_id, forward to peer or dispatch locally.
    ///
    /// Delegates to the internal [`Router`] which handles command dispatching and
    /// peer forwarding. The `state` parameter is no longer needed because the
    /// `Router` holds its own reference to the shared state.
    pub async fn route_message(&self, msg: WraithMessage) -> Option<WraithMessage> {
        self.router.route_message(msg, &self.sessions).await
    }

    pub async fn start_peer_listener(self: Arc<Self>, addr: &str) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        info!("Listening for peer connections on {}", addr);
        let state = Arc::clone(&self.state);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    info!("Peer connection from: {}", peer_addr);
                    let state = Arc::clone(&state);
                    let this = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_peer_connection(stream, state, this).await {
                            warn!("Peer connection handler error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    warn!("Failed to accept peer connection: {}", e);
                }
            }
        }
    }

    pub async fn handle_peer_connection(
        stream: TcpStream,
        state: Arc<Mutex<WraithState>>,
        tunnel_manager: Arc<TunnelManager>,
    ) -> Result<()> {
        let conn = yamux::Connection::new(stream.compat(), yamux::Config::default(), yamux::Mode::Server);
        let (conn_handle, mut inbound_rx) = spawn_yamux_driver_with_inbound(conn);

        // Single stream: registration handshake, then reused for command traffic.
        // yamux only sends a SYN when data is first written to a stream, so using
        // two separate streams caused the second SYN to never reach the server.
        let peer_stream = inbound_rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("no inbound stream from peer"))?;

        let stream_compat = peer_stream.compat();
        let (mut read_half, mut write_half) = tokio::io::split(stream_compat);
        let msg = PeerSession::read_message(&mut read_half).await;

        if let Ok(Some(msg)) = msg {
            if let Some(crate::proto::wraith::wraith_message::Payload::WraithRegistration(reg)) = msg.payload {
                let wraith_id = reg.wraith_id.clone();
                let hostname = reg.hostname.clone();
                info!("Received registration from peer: {}", wraith_id);

                let (local_id, local_hostname_val, local_os_val) = {
                    let local_state = state.lock().expect("state lock poisoned");
                    (local_state.wraith_id.clone(), local_state.hostname.clone(), local_state.os.clone())
                };
                let local_reg = crate::proto::wraith::WraithRegistration {
                    wraith_id: local_id,
                    hostname: local_hostname_val,
                    os: local_os_val,
                    connected_at: chrono::Utc::now().timestamp_millis(),
                };

                let local_reg_msg = crate::proto::wraith::WraithMessage {
                    msg_type: crate::proto::wraith::MessageType::WraithRegistration as i32,
                    payload: Some(crate::proto::wraith::wraith_message::Payload::WraithRegistration(local_reg)),
                    message_id: uuid::Uuid::new_v4().to_string(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    target_wraith_id: String::new(),
                };
                PeerSession::write_message(&mut write_half, &local_reg_msg).await?;

                // Same stream now carries command traffic
                let (tx, rx) = mpsc::channel::<crate::proto::wraith::WraithMessage>(100);

                let session = PeerSession::new(
                    wraith_id.clone(),
                    hostname.clone(),
                    conn_handle.clone(),
                    tx,
                );

                session.spawn_writer(write_half, rx);
                tunnel_manager.add_session(wraith_id.clone(), session).await;
                info!("Registered peer: {}", wraith_id);

                let stream_arc = Arc::new(tokio::sync::Mutex::new(read_half));

                run_peer_message_loop(
                    stream_arc,
                    state,
                    tunnel_manager,
                    wraith_id.clone(),
                ).await;
            }
        }

        Ok(())
    }

    /// Connect to a remote peer wraith.
    ///
    /// Exchanges registrations with the remote peer: sends local registration on
    /// Stream 0, reads the remote's registration back, then opens Stream 1 for
    /// bidirectional command traffic.
    pub async fn connect_to_peer(
        self: Arc<Self>,
        addr: String,
        local_wraith_id: String,
        local_hostname: String,
        local_os: String,
    ) -> anyhow::Result<()> {
        let stream = TcpStream::connect(&addr).await?;
        let peer_addr = stream.peer_addr()?;
        info!("Connecting to peer at {}", peer_addr);

        let config = yamux::Config::default();
        let conn = yamux::Connection::new(stream.compat(), config, yamux::Mode::Client);
        let conn_handle = Arc::new(tokio::sync::Mutex::new(conn));

        // Open a single stream before spawning the driver. The driver holds
        // conn_handle's lock for its entire lifetime, so we must open all
        // outbound streams first. Using one stream for both registration and
        // command traffic avoids the issue where yamux never sends a SYN for
        // an unused stream.
        let peer_stream = {
            let mut conn_lock = conn_handle.lock().await;
            poll_fn(|cx| Pin::new(&mut conn_lock).poll_new_outbound(cx)).await?
        };
        info!("Opened outbound stream for peer communication");

        // Spawn driver — it takes the lock but we're done opening streams
        {
            let driver_handle = Arc::clone(&conn_handle);
            tokio::spawn(async move {
                let mut c = driver_handle.lock().await;
                loop {
                    match poll_fn(|cx| Pin::new(&mut c).poll_next_inbound(cx)).await {
                        Some(Ok(_)) => { /* inbound — not needed on client side */ }
                        Some(Err(e)) => {
                            warn!("Yamux connection error: {}", e);
                            break;
                        }
                        None => {
                            info!("Yamux connection closed");
                            break;
                        }
                    }
                }
                info!("Yamux driver finished");
            });
        }

        // Exchange registrations, then reuse the same stream for commands
        let reg = crate::proto::wraith::WraithRegistration {
            wraith_id: local_wraith_id.clone(),
            hostname: local_hostname.clone(),
            os: local_os,
            connected_at: chrono::Utc::now().timestamp_millis(),
        };

        let reg_msg = crate::proto::wraith::WraithMessage {
            msg_type: crate::proto::wraith::MessageType::WraithRegistration as i32,
            payload: Some(crate::proto::wraith::wraith_message::Payload::WraithRegistration(reg)),
            message_id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            target_wraith_id: String::new(),
        };

        let stream_compat = peer_stream.compat();
        let (mut read_half, mut write_half) = tokio::io::split(stream_compat);

        PeerSession::write_message(&mut write_half, &reg_msg).await?;
        info!("Sent WraithRegistration to peer at {}", addr);

        let remote_id = match PeerSession::read_message(&mut read_half).await {
            Ok(Some(resp)) => {
                match resp.payload {
                    Some(crate::proto::wraith::wraith_message::Payload::WraithRegistration(remote_reg)) => {
                        info!("Received registration from remote peer: {}", remote_reg.wraith_id);
                        remote_reg.wraith_id
                    }
                    other => return Err(anyhow::anyhow!("Expected WraithRegistration response, got {:?}", other)),
                }
            }
            Ok(None) => return Err(anyhow::anyhow!("Connection closed before receiving registration")),
            Err(e) => return Err(anyhow::anyhow!("Failed to read registration response: {}", e)),
        };

        // Same stream now carries command traffic
        let (tx, rx) = mpsc::channel::<crate::proto::wraith::WraithMessage>(100);

        let session = PeerSession::new(
            remote_id.clone(),
            addr.clone(),
            conn_handle.clone(),
            tx,
        );

        session.spawn_writer(write_half, rx);

        self.add_session(remote_id.clone(), session).await;
        info!("Registered peer: {}", remote_id);

        let stream_arc = Arc::new(tokio::sync::Mutex::new(read_half));

        let state = Arc::clone(&self.state);
        let tunnel_manager = Arc::clone(&self);
        let remote_id_for_loop = remote_id.clone();
        tokio::spawn(async move {
            run_peer_message_loop(stream_arc, state, tunnel_manager, remote_id_for_loop).await;
        });

        info!("Established peer connection: {}", remote_id);
        Ok(())
    }
}
