pub mod session;

pub use session::PeerSession;

use anyhow::Result;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tokio_util::compat::TokioAsyncReadCompatExt;

pub struct TunnelManager {
    sessions: Arc<RwLock<HashMap<String, PeerSession>>>,
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_session(&self, wraith_id: String, session: PeerSession) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(wraith_id.clone(), session);
        info!("Added peer session: {}", wraith_id);
    }

    pub async fn remove_session(&self, wraith_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(wraith_id);
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

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    info!("Peer connection from: {}", peer_addr);
                    let sessions = Arc::clone(&sessions);
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_peer_connection(stream, sessions).await {
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
    ) -> Result<()> {
        use crate::wraith::tunnel::PeerSession;
        use futures::io::{AsyncReadExt, AsyncWriteExt};

        let mut conn = yamux::Connection::new(stream.compat(), yamux::Config::default(), yamux::Mode::Server);

        // Use next_stream() to get the first stream from the connection
        let mut stream = match conn.next_stream().await {
            Ok(Some(s)) => s,
            Ok(None) => return Ok(()), // No stream available
            Err(e) => return Err(anyhow::anyhow!("yamux error: {}", e)),
        };

        // Read WraithRegistration from Stream 0
        if let Ok(Some(msg)) = PeerSession::read_message(&mut stream).await {
            if let Some(crate::proto::wraith::wraith_message::Payload::WraithRegistration(reg)) = msg.payload {
                let wraith_id = reg.wraith_id.clone();
                let hostname = reg.hostname.clone();
                let (tx, _rx) = mpsc::channel(100);

                let session = PeerSession::new(wraith_id.clone(), hostname, conn, tx);

                let mut sessions_write = sessions.write().await;
                sessions_write.insert(wraith_id.clone(), session);

                info!("Registered peer: {}", wraith_id);
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

        // Open stream 0 for registration
        let mut stream = conn.control().open_stream().await?;

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
        };

        // Encode and send registration message
        let data = crate::message::codec::MessageCodec::encode(&reg_msg);
        let len = data.len() as u32;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&data).await?;
        stream.flush().await?;

        debug!("Sent WraithRegistration to peer at {}", addr);

        // Create channels for command communication
        let (tx, _rx) = mpsc::channel::<crate::proto::wraith::WraithMessage>(100);

        // Create peer session and add to tunnel manager
        let session = PeerSession::new(
            wraith_id.clone(),
            hostname,
            conn,
            tx,
        );

        self.add_session(wraith_id.clone(), session).await;

        info!("Established peer connection: {}", wraith_id);
        Ok(())
    }
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}