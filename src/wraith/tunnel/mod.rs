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
use tokio::sync::{mpsc, RwLock};
use tokio_util::compat::TokioAsyncReadCompatExt;

type PeerAddCallback = Box<dyn Fn(&str, &str, &tokio::sync::mpsc::Sender<crate::proto::wraith::WraithMessage>) + Send + 'static>;
type PeerRemoveCallback = Box<dyn Fn(&str) + Send + 'static>;

pub struct TunnelManager {
    sessions: Arc<RwLock<HashMap<String, PeerSession>>>,
    peer_add_callback: Arc<Mutex<Option<PeerAddCallback>>>,
    peer_remove_callback: Arc<Mutex<Option<PeerRemoveCallback>>>,
    dispatcher: Arc<Mutex<Option<crate::wraith::dispatcher::MessageDispatcher>>>,
    state: Arc<Mutex<Option<Arc<Mutex<crate::wraith::state::WraithState>>>>>,
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            peer_add_callback: Arc::new(Mutex::new(None)),
            peer_remove_callback: Arc::new(Mutex::new(None)),
            dispatcher: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the dispatcher for routing peer messages
    pub fn set_dispatcher(&self, dispatcher: crate::wraith::dispatcher::MessageDispatcher) {
        *self.dispatcher.lock().unwrap() = Some(dispatcher);
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

    pub async fn start_peer_listener(&self, addr: &str) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        info!("Listening for peer connections on {}", addr);
        let sessions = Arc::clone(&self.sessions);
        let peer_add_callback = Arc::clone(&self.peer_add_callback);
        let dispatcher = Arc::clone(&self.dispatcher);
        let state = Arc::clone(&self.state);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    info!("Peer connection from: {}", peer_addr);
                    let sessions = Arc::clone(&sessions);
                    let peer_add_callback = Arc::clone(&peer_add_callback);
                    let dispatcher = Arc::clone(&dispatcher);
                    let state = Arc::clone(&state);
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_peer_connection(stream, sessions, peer_add_callback, dispatcher, state).await {
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
        dispatcher: Arc<Mutex<Option<crate::wraith::dispatcher::MessageDispatcher>>>,
        state: Arc<Mutex<Option<Arc<Mutex<crate::wraith::state::WraithState>>>>>,
    ) -> Result<()> {
        use crate::wraith::tunnel::PeerSession;

        let conn = yamux::Connection::new(stream.compat(), yamux::Config::default(), yamux::Mode::Server);

        // Drive connection in background so frames get written to socket
        let conn_handle = Arc::new(tokio::sync::Mutex::new(conn));
        let conn_handle_for_spawn = conn_handle.clone();
        tokio::spawn(async move {
            let mut c = conn_handle_for_spawn.lock().await;
            loop {
                // poll_next_inbound returns Option<Result<Stream>>
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
        let mut stream = match futures::future::poll_fn(|cx| Pin::new(&mut conn_lock).poll_next_inbound(cx)).await {
            Some(Ok(s)) => { info!("TEST2: got inbound stream"); s }
            Some(Err(e)) => return Err(anyhow::anyhow!("yamux error: {}", e)),
            None => { info!("TEST: no stream available"); return Ok(()); }
        };
        drop(conn_lock);

        // Wrap stream in Arc<Mutex> so both message loop and forwarder can access it
        let stream_arc = Arc::new(tokio::sync::Mutex::new(stream));

        // Read WraithRegistration from Stream 0
        {
            let mut stream_lock = stream_arc.lock().await;
            if let Ok(Some(msg)) = PeerSession::read_message(&mut stream_lock).await {
                if let Some(crate::proto::wraith::wraith_message::Payload::WraithRegistration(reg)) = msg.payload {
                    let wraith_id = reg.wraith_id.clone();
                    let hostname = reg.hostname.clone();
                    let (tx, mut rx) = mpsc::channel::<crate::proto::wraith::WraithMessage>(100);

                    let session = PeerSession::new(wraith_id.clone(), hostname.clone(), conn_handle, tx.clone());

                    {
                        let mut sessions_write = sessions.write().await;
                        sessions_write.insert(wraith_id.clone(), session);
                    }

                    // Notify observer (e.g., WraithState.peer_table)
                    if let Some(ref cb) = *peer_add_callback.lock().unwrap() {
                        cb(&wraith_id, &hostname, &tx);
                    }

                    // Spawn task to forward messages from channel to peer via Yamux stream 0
                    let stream_arc_for_fwd = Arc::clone(&stream_arc);
                    let wraith_id_for_fwd = wraith_id.clone();
                    tokio::spawn(async move {
                        while let Some(msg) = rx.recv().await {
                            let mut stream = stream_arc_for_fwd.lock().await;
                            if let Err(e) = PeerSession::write_message(&mut stream, &msg).await {
                                warn!("Failed to forward message to peer: {}", e);
                                break;
                            }
                        }
                        info!("Peer command forwarder task finished for {}", wraith_id_for_fwd);
                    });

                    info!("Registered peer: {}", wraith_id);
                }
            }
        }

        // Message loop: read commands from stream 0 and dispatch with dedup
        loop {
            let msg = {
                let mut stream_lock = stream_arc.lock().await;
                match PeerSession::read_message(&mut stream_lock).await {
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

            // Check if this is a response to a pending forwarded command
            // Extract state early to avoid holding locks across await
            let (pending_tx, disp, state_arc) = {
                let disp = dispatcher.lock().unwrap().clone();
                let state_val = state.lock().unwrap().clone();
                let msg_id = msg.message_id.clone();

                // Check if this message has a pending response waiting
                let pending_tx = state_val.as_ref().and_then(|s| {
                    s.lock().unwrap().take_pending_response(&msg_id)
                });

                (pending_tx, disp, state_val)
            };

            // Check dedup before processing
            let msg_id = msg.message_id.clone();
            if let Some(ref state_ref) = state_arc {
                if state_ref.lock().unwrap().has_seen_message(&msg_id) {
                    info!("Skipping duplicate message: {}", msg_id);
                    continue;
                }
                state_ref.lock().unwrap().mark_message_seen(msg_id.clone());
            }

            // Route message via dispatcher if available
            if let Some(disp) = disp {
                if let Some(state_ref) = state_arc {
                    // Check if this is a response destined for a pending oneshot
                    if let Some(tx) = pending_tx {
                        // This is a response to a forwarded command - send to the waiting caller
                        if tx.send(msg.clone()).is_err() {
                            info!("Failed to send response to waiting caller for message: {}", msg_id);
                        }
                        // Don't re-dispatch - this message is already the final response
                        continue;
                    }

                    // Dispatch the message (route_message handles target_wraith_id routing)
                    let _ = disp.route_message(msg, state_ref).await;
                }
            }
        }

        Ok(())
    }

    /// Connect to a remote peer wraith
    pub async fn connect_to_peer(
        &self,
        addr: String,
        wraith_id: String,
        hostname: String,
        os: String,
    ) -> anyhow::Result<()> {
        use futures::io::AsyncWriteExt;

        // Connect to peer via TCP
        let stream = TcpStream::connect(&addr).await?;
        let peer_addr = stream.peer_addr()?;
        info!("Connecting to peer at {}", peer_addr);

        // Create Yamux connection as client
        let config = yamux::Config::default();
        let conn = yamux::Connection::new(stream.compat(), config, yamux::Mode::Client);

        // Drive the connection in the background so frames get written to the socket
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
            }
            info!("Peer connection client driver finished");
        });

        // Open stream 0 for registration using poll_new_outbound
        info!("Before Sending Wraith registration");
        let mut stream = {
            let mut conn_lock = conn_handle.lock().await;
            poll_fn(|cx| Pin::new(&mut conn_lock).poll_new_outbound(cx)).await?
        };
        info!("Opened outbound stream, sending Wraith registration");

        // Send WraithRegistration
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

        // Encode and send registration message
        let data = crate::message::codec::MessageCodec::encode(&reg_msg);
        let len = data.len() as u32;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&data).await?;
        stream.flush().await?;

        debug!("Sent WraithRegistration to peer at {}", addr);

        // Wrap stream in Arc<Mutex> for shared access by forwarding task
        let stream_arc = Arc::new(tokio::sync::Mutex::new(stream));

        // Create channels for command communication
        let (tx, mut rx) = mpsc::channel::<crate::proto::wraith::WraithMessage>(100);

        // Create peer session and add to tunnel manager
        let session = PeerSession::new(
            wraith_id.clone(),
            hostname.clone(),
            conn_handle,
            tx,
        );

        self.add_session(wraith_id.clone(), session).await;

        // Spawn task to forward messages from channel to peer via Yamux stream 0
        let stream_arc_for_fwd = Arc::clone(&stream_arc);
        let wraith_id_for_fwd = wraith_id.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let mut stream = stream_arc_for_fwd.lock().await;
                if let Err(e) = PeerSession::write_message(&mut stream, &msg).await {
                    warn!("Failed to forward message to peer: {}", e);
                    break;
                }
            }
            info!("Peer command forwarder task finished for {}", wraith_id_for_fwd);
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