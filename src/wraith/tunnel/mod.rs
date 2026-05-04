pub mod session;

pub use session::PeerSession;

use anyhow::Result;
use futures::future::poll_fn;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock, oneshot};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use yamux::{Config, Connection, Mode, Stream};

use crate::commands::relay::RelayCommands;
use crate::commands::agent::AgentCommands;
use crate::commands::command::Command;
use crate::message::codec::MessageCodec;
use crate::proto::wraith::{MessageType, WraithMessage};

type PeerAddCallback = Box<dyn Fn(&str, &str, &tokio::sync::mpsc::Sender<crate::proto::wraith::WraithMessage>) + Send + 'static>;
type PeerRemoveCallback = Box<dyn Fn(&str) + Send + 'static>;

pub struct TunnelManager {
    sessions: Arc<RwLock<HashMap<String, PeerSession>>>,
    peer_add_callback: Arc<Mutex<Option<PeerAddCallback>>>,
    peer_remove_callback: Arc<Mutex<Option<PeerRemoveCallback>>>,
    relay_commands: Arc<Mutex<RelayCommands>>,
    agent_commands: Arc<Mutex<AgentCommands>>,
    state: Arc<Mutex<Option<Arc<Mutex<crate::wraith::state::WraithState>>>>>,
}

impl TunnelManager {
    /// Creates a TunnelManager for basic session management
    /// Command handlers are NOT stored in TunnelManager - use with_commands() to add them
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            peer_add_callback: Arc::new(Mutex::new(None)),
            peer_remove_callback: Arc::new(Mutex::new(None)),
            relay_commands: Arc::new(Mutex::new(RelayCommands::new_without_tunnel(
                Arc::new(Mutex::new(crate::relay::RelayManager::new())),
            ))),
            agent_commands: Arc::new(Mutex::new(AgentCommands::new_without_tunnel())),
            state: Arc::new(Mutex::new(None)),
        }
    }

    /// Create TunnelManager with command handlers for routing messages
    /// This should be used instead of new() when proper routing is needed
    pub fn with_commands(relay_commands: RelayCommands, agent_commands: AgentCommands) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            peer_add_callback: Arc::new(Mutex::new(None)),
            peer_remove_callback: Arc::new(Mutex::new(None)),
            relay_commands: Arc::new(Mutex::new(relay_commands)),
            agent_commands: Arc::new(Mutex::new(agent_commands)),
            state: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the state for dedup checking
    pub fn set_state(&self, state: Arc<Mutex<crate::wraith::state::WraithState>>) {
        *self.state.lock().unwrap() = Some(state);
    }

    /// Register a callback invoked when a peer is added.
    pub fn set_peer_add_callback<F>(&self, callback: F)
    where
        F: Fn(&str, &str, &tokio::sync::mpsc::Sender<crate::proto::wraith::WraithMessage>) + Send + 'static,
    {
        let mut cb = self.peer_add_callback.lock().unwrap();
        *cb = Some(Box::new(callback));
    }

    /// Register a callback invoked when a peer is removed.
    pub fn set_peer_remove_callback<F>(&self, callback: F)
    where
        F: Fn(&str) + Send + 'static,
    {
        let mut cb = self.peer_remove_callback.lock().unwrap();
        *cb = Some(Box::new(callback));
    }

    fn notify_peer_added(&self, wraith_id: &str, hostname: &str, sender: &tokio::sync::mpsc::Sender<crate::proto::wraith::WraithMessage>) {
        let cb = self.peer_add_callback.lock().unwrap();
        if let Some(ref callback) = *cb {
            callback(wraith_id, hostname, sender);
        }
    }

    fn notify_peer_removed(&self, wraith_id: &str) {
        let cb = self.peer_remove_callback.lock().unwrap();
        if let Some(ref callback) = *cb {
            callback(wraith_id);
        }
    }

    pub async fn add_session(&self, wraith_id: String, session: PeerSession) {
        let hostname = session.hostname.clone();
        let command_tx = session.command_tx.clone();
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(wraith_id.clone(), session);
        }
        self.notify_peer_added(&wraith_id, &hostname, &command_tx);
        info!("Added peer session: {}", wraith_id);
    }

    pub async fn remove_session(&self, wraith_id: &str) {
        {
            let mut sessions = self.sessions.write().await;
            sessions.remove(wraith_id);
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

    pub async fn start_peer_listener(self: Arc<Self>, addr: &str) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        info!("Listening for peer connections on {}", addr);
        let sessions = Arc::clone(&self.sessions);
        let peer_add_callback = Arc::clone(&self.peer_add_callback);
        let relay_commands = Arc::clone(&self.relay_commands);
        let agent_commands = Arc::clone(&self.agent_commands);
        let state = Arc::clone(&self.state);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    info!("Peer connection from: {}", peer_addr);
                    let sessions = Arc::clone(&sessions);
                    let peer_add_callback = Arc::clone(&peer_add_callback);
                    let relay_commands = Arc::clone(&relay_commands);
                    let agent_commands = Arc::clone(&agent_commands);
                    let state = Arc::clone(&state);
                    let this = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_peer_connection(stream, sessions, peer_add_callback, relay_commands, agent_commands, state, this).await {
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

    async fn handle_peer_connection(
        stream: TcpStream,
        sessions: Arc<RwLock<HashMap<String, PeerSession>>>,
        peer_add_callback: Arc<Mutex<Option<PeerAddCallback>>>,
        relay_commands: Arc<Mutex<RelayCommands>>,
        agent_commands: Arc<Mutex<AgentCommands>>,
        state: Arc<Mutex<Option<Arc<Mutex<crate::wraith::state::WraithState>>>>>,
        tunnel_manager: Arc<TunnelManager>,
    ) -> Result<()> {
        use crate::wraith::tunnel::PeerSession;

        let conn = yamux::Connection::new(stream.compat(), yamux::Config::default(), yamux::Mode::Server);

        // Drive connection in background so frames get written to socket
        let conn_handle = Arc::new(tokio::sync::Mutex::new(conn));
        let conn_handle_for_spawn = conn_handle.clone();
        tokio::spawn(async move {
            let mut c = conn_handle_for_spawn.lock().await;
            loop {
                match futures::future::poll_fn(|cx| Pin::new(&mut c).poll_next_inbound(cx)).await {
                    Some(Ok(_)) => { /* stream handled elsewhere */ }
                    Some(Err(e)) => { warn!("Peer connection server error: {}", e); break; }
                    None => { info!("Peer connection server: no more inbound streams"); break; }
                }
            }
            info!("Peer connection server driver finished");
        });

        // Use poll_next_inbound to get the first stream from the connection
        let mut conn_lock = conn_handle.lock().await;
        let yamux_stream = match futures::future::poll_fn(|cx| Pin::new(&mut conn_lock).poll_next_inbound(cx)).await {
            Some(Ok(s)) => s,
            Some(Err(e)) => return Err(anyhow::anyhow!("yamux error: {}", e)),
            None => return Ok(()),
        };
        drop(conn_lock);

        // Read WraithRegistration from Stream 0
        {
            let stream_compat = yamux_stream.compat();
            let (mut read_half, mut write_half) = tokio::io::split(stream_compat);

            // --- Read initial registration ---
            let msg = PeerSession::read_message(&mut read_half).await;

            if let Ok(Some(msg)) = msg {
                if let Some(crate::proto::wraith::wraith_message::Payload::WraithRegistration(reg)) = msg.payload {
                    let wraith_id = reg.wraith_id.clone();
                    let hostname = reg.hostname.clone();

                    let (tx, mut rx) = mpsc::channel::<crate::proto::wraith::WraithMessage>(100);

                    let session = PeerSession::new(
                        wraith_id.clone(),
                        hostname.clone(),
                        conn_handle.clone(),
                        tx.clone(),
                    );

                    {
                        let mut sessions_write = sessions.write().await;
                        sessions_write.insert(wraith_id.clone(), session);
                    }

                    if let Some(ref cb) = *peer_add_callback.lock().unwrap() {
                        cb(&wraith_id, &hostname, &tx);
                    }

                    // --- Writer task (OWNS write_half) ---
                    let wraith_id_for_fwd = wraith_id.clone();

                    tokio::spawn(async move {
                        while let Some(msg) = rx.recv().await {
                            if let Err(e) = PeerSession::write_message(&mut write_half, &msg).await {
                                warn!("Failed to forward message: {}", e);
                                break;
                            }
                        }

                        info!("Writer task finished for {}", wraith_id_for_fwd);
                    });

                    info!("Registered peer: {}", wraith_id);

                    // Re-open the stream for the message loop
                    // Get a new inbound stream from the same connection
                    let conn_handle2 = conn_handle.clone();
                    let mut conn_lock2 = conn_handle2.lock().await;
                    let stream_for_loop = match futures::future::poll_fn(|cx| Pin::new(&mut conn_lock2).poll_next_inbound(cx)).await {
                        Some(Ok(s)) => s,
                        Some(Err(e)) => return Err(anyhow::anyhow!("yamux error in message loop: {}", e)),
                        None => return Ok(()),
                    };
                    drop(conn_lock2);

                    // Create Arc<Mutex> for message loop
                    let stream_compat = stream_for_loop.compat();
                    let stream_arc = Arc::new(tokio::sync::Mutex::new(stream_compat));

                    // Message loop: read commands from stream 0 and dispatch with dedup
                    loop {
                        // --- READ MESSAGE (lock scope isolated) ---
                        let msg = {
                            let mut stream_lock = stream_arc.lock().await;

                            match PeerSession::read_message(&mut *stream_lock).await {
                                Ok(Some(msg)) => msg,
                                Ok(None) => {
                                    info!("Peer stream ended");
                                    break;
                                }
                                Err(e) => {
                                    warn!("Error reading from peer stream: {}", e);
                                    break;
                                }
                            }
                        };

                        // --- STATE ACCESS (no clone, minimal locking) ---
                        let msg_id = msg.message_id.clone();

                        let pending_tx = {
                            let state_guard = state.lock().unwrap();
                            state_guard
                                .as_ref()
                                .and_then(|s| s.lock().unwrap().take_pending_response(&msg_id))
                        };

                        let already_seen = {
                            let state_guard = state.lock().unwrap();
                            if let Some(ref s) = *state_guard {
                                let mut s = s.lock().unwrap();

                                if s.has_seen_message(&msg_id) {
                                    true
                                } else {
                                    s.mark_message_seen(msg_id.clone());
                                    false
                                }
                            } else {
                                false
                            }
                        };

                        if already_seen {
                            info!("Skipping duplicate message: {}", msg_id);
                            continue;
                        }

                        // --- Pending response handling ---
                        if let Some(tx) = pending_tx {
                            if tx.send(msg).is_err() {
                                info!("Failed to send response for {}", msg_id);
                            }
                            continue;
                        }

                        // --- Route normal message ---
                        let state_ref = {
                            let state_guard = state.lock().unwrap();
                            state_guard.clone()
                        };
                        if let Some(state_ref) = state_ref {
                            let _ = tunnel_manager.route_message(msg, state_ref).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Route a message: check target_wraith_id, forward to peer or dispatch locally
    pub async fn route_message(
        &self,
        msg: WraithMessage,
        state: Arc<Mutex<crate::wraith::state::WraithState>>,
    ) -> Option<WraithMessage> {
        let target = msg.target_wraith_id.clone();
        let local_id = state.lock().unwrap().wraith_id.clone();
        let msg_id = msg.message_id.clone();

        // If targeted to us, dispatch locally
        if target.is_empty() || target == local_id {
            return self.dispatch(msg, state).await;
        }

        // If targeted to a direct peer, forward to that peer and wait for response
        let peer = {
            let peer_table = state.lock().unwrap().peer_table.clone();
            peer_table.get(&target).cloned()
        };

        if let Some(peer) = peer {
            let (response_tx, response_rx) = oneshot::channel::<WraithMessage>();

            state.lock().unwrap().register_pending_response(msg_id.clone(), response_tx);

            let msg_clone = msg.clone();
            debug!("Passing message to direct peer {}", peer.wraith_id);
            let send_result = peer.sender.send(msg_clone).await;
            if send_result.is_ok() {
                match response_rx.await {
                    Ok(response) => {
                        debug!("Received answer for peer forwarding from {}.", peer.wraith_id);
                        state.lock().unwrap().take_pending_response(&msg_id);
                        return Some(response);
                    }
                    Err(_) => {
                        info!("Peer response channel closed for message: {}", msg_id);
                        state.lock().unwrap().take_pending_response(&msg_id);
                    }
                }
            } else {
                info!("Failed to send to peer {}: {:?}", peer.wraith_id, send_result.err());
                state.lock().unwrap().take_pending_response(&msg_id);
            }
        }

        debug!("Sending command to all peers");
        // Broadcast to all peers
        let peers: Vec<_> = {
            let sessions = self.sessions.read().await;
            sessions.values().cloned().collect()
        };
        for peer in peers {
            let _ = peer.command_tx.send(msg.clone()).await;
        }

        Some(MessageCodec::create_command_result(
            "".to_string(),
            "broadcast".to_string(),
            "".to_string(),
            0, 0, "".to_string(),
        ))
    }

    /// Dispatch a message locally
    async fn dispatch(
        &self,
        msg: WraithMessage,
        state: Arc<Mutex<crate::wraith::state::WraithState>>,
    ) -> Option<WraithMessage> {
        let msg_type = msg.msg_type;
        info!("Dispatching message of type: {:?}", msg_type);

        if msg_type == MessageType::Command as i32 {
            if let Some(crate::proto::wraith::wraith_message::Payload::Command(cmd)) = &msg.payload {
                let result = if cmd.action == "create_relay" {
                    let relay_commands = self.relay_commands.lock().unwrap();
                    let local_wraith_id = state.lock().unwrap().wraith_id.clone();
                    relay_commands.handle_create_relay(cmd, &local_wraith_id)
                } else if cmd.action == "delete_relay" || cmd.action == "list_relays" {
                    self.relay_commands.lock().unwrap().execute(cmd)
                } else if cmd.action == "set_id" {
                    self.agent_commands.lock().unwrap().handle_set_id(cmd, &mut state.lock().unwrap())
                } else if cmd.action == "list_peers" {
                    self.agent_commands.lock().unwrap().handle_list_peers(cmd, &state.lock().unwrap())
                } else if cmd.action == "wraith_listen" {
                    self.agent_commands.lock().unwrap().handle_wraith_listen(cmd)
                } else if cmd.action == "wraith_connect" {
                    self.agent_commands.lock().unwrap().handle_wraith_connect(cmd, &state.lock().unwrap())
                } else {
                    return None;
                };

                state.lock().unwrap().increment_commands();

                return Some(MessageCodec::create_command_result(
                    result.command_id,
                    result.status,
                    result.output,
                    result.exit_code,
                    result.duration_ms,
                    result.error,
                ));
            }
        }
        None
    }

    /// Connect to a remote peer wraith
    pub async fn connect_to_peer(
        self: Arc<Self>,
        addr: String,
        wraith_id: String,
        hostname: String,
        os: String,
    ) -> anyhow::Result<()> {
        use futures::io::AsyncWriteExt;

        let stream = TcpStream::connect(&addr).await?;
        let peer_addr = stream.peer_addr()?;
        info!("Connecting to peer at {}", peer_addr);

        let config = yamux::Config::default();
        let conn = yamux::Connection::new(stream.compat(), config, yamux::Mode::Client);

        let conn_handle = Arc::new(tokio::sync::Mutex::new(conn));
        let conn_handle_for_spawn = conn_handle.clone();
        tokio::spawn(async move {
            let mut c = conn_handle_for_spawn.lock().await;
            loop {
                match futures::future::poll_fn(|cx| Pin::new(&mut c).poll_next_inbound(cx)).await {
                    Some(Ok(_)) => { /* handle incoming */ }
                    Some(Err(e)) => { warn!("Peer connection client error: {}", e); break; }
                    None => { info!("Peer connection client: connection closed"); break; }
                }
                match futures::future::poll_fn(|cx| Pin::new(&mut c).poll_new_outbound(cx)).await {
                    Ok(_stream) => { /* outbound stream ready */ }
                    Err(e) => { warn!("Peer connection client outbound error: {}", e); break; }
                }
            }
            info!("Peer connection client driver finished");
        });

        let mut stream = {
            let mut conn_lock = conn_handle.lock().await;
            poll_fn(|cx| Pin::new(&mut conn_lock).poll_new_outbound(cx)).await?
        };
        info!("Opened outbound stream, sending Wraith registration");

        let reg = crate::proto::wraith::WraithRegistration {
            wraith_id: wraith_id.clone(),
            hostname: hostname.clone(),
            os,
            connected_at: chrono::Utc::now().timestamp_millis(),
        };

        let reg_msg = crate::proto::wraith::WraithMessage {
            msg_type: crate::proto::wraith::MessageType::WraithRegistration as i32,
            payload: Some(crate::proto::wraith::wraith_message::Payload::WraithRegistration(reg)),
            message_id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            target_wraith_id: String::new(),
        };

        let data = crate::message::codec::MessageCodec::encode(&reg_msg);
        let len = data.len() as u32;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&data).await?;
        stream.flush().await?;

        debug!("Sent WraithRegistration to peer at {}", addr);

        // Split stream into read/write halves - writer task takes write_half
        let stream_compat = stream.compat();
        let (read_half, mut write_half) = tokio::io::split(stream_compat);
        let (tx, mut rx) = mpsc::channel::<crate::proto::wraith::WraithMessage>(100);

        let session = PeerSession::new(
            wraith_id.clone(),
            hostname.clone(),
            conn_handle.clone(),
            tx,
        );

        self.add_session(wraith_id.clone(), session).await;

        let wraith_id_for_fwd = wraith_id.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if let Err(e) = PeerSession::write_message(&mut write_half, &msg).await {
                    warn!("Failed to forward message: {}", e);
                    break;
                }
            }
            info!("Writer task finished for {}", wraith_id_for_fwd);
        });

        info!("Registered peer: {}", wraith_id);

        // Get a new outbound stream for the message loop (Stream 1)
        let conn_handle2 = conn_handle.clone();
        let mut conn_lock2 = conn_handle2.lock().await;
        let stream_for_loop = match poll_fn(|cx| Pin::new(&mut conn_lock2).poll_new_outbound(cx)).await {
            Ok(s) => s,
            Err(e) => return Err(anyhow::anyhow!("yamux error getting message loop stream: {}", e)),
        };
        drop(conn_lock2);

        let stream_compat = stream_for_loop.compat();
        let stream_arc = Arc::new(tokio::sync::Mutex::new(stream_compat));

        // Message loop: read commands from stream and dispatch with dedup
        let state = Arc::clone(&self.state);
        let tunnel_manager = self;
        let wraith_id_for_loop = wraith_id.clone();
        tokio::spawn(async move {
            loop {
                // --- READ MESSAGE (lock scope isolated) ---
                let msg = {
                    let mut stream_lock = stream_arc.lock().await;

                    match PeerSession::read_message(&mut *stream_lock).await {
                        Ok(Some(msg)) => msg,
                        Ok(None) => {
                            info!("Peer stream ended for {}", wraith_id_for_loop);
                            break;
                        }
                        Err(e) => {
                            warn!("Error reading from peer stream: {}", e);
                            break;
                        }
                    }
                };

                // --- STATE ACCESS (no clone, minimal locking) ---
                let msg_id = msg.message_id.clone();

                let pending_tx = {
                    let state_guard = state.lock().unwrap();
                    state_guard
                        .as_ref()
                        .and_then(|s| s.lock().unwrap().take_pending_response(&msg_id))
                };

                let already_seen = {
                    let state_guard = state.lock().unwrap();
                    if let Some(ref s) = *state_guard {
                        let mut s = s.lock().unwrap();

                        if s.has_seen_message(&msg_id) {
                            true
                        } else {
                            s.mark_message_seen(msg_id.clone());
                            false
                        }
                    } else {
                        false
                    }
                };

                if already_seen {
                    info!("Skipping duplicate message: {}", msg_id);
                    continue;
                }

                // --- Pending response handling ---
                if let Some(tx) = pending_tx {
                    if tx.send(msg).is_err() {
                        info!("Failed to send response for {}", msg_id);
                    }
                    continue;
                }

                // --- Route normal message ---
                let state_ref = {
                    let state_guard = state.lock().unwrap();
                    state_guard.clone()
                };
                if let Some(state_ref) = state_ref {
                    let _ = tunnel_manager.route_message(msg, state_ref).await;
                }
            }
        });

        info!("Established peer connection: {}", wraith_id);
        Ok(())
    }
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}
