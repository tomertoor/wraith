use log::{info, warn};
use std::sync::{Arc, Mutex};

use crate::wraith::state::WraithState;
use super::peer::PeerSession;
use super::registry::TunnelManager;

/// Shared message loop for peer connections.
/// Reads messages from a stream, deduplicates via state, and routes through the tunnel manager.
pub async fn run_peer_message_loop<S>(
    stream: Arc<tokio::sync::Mutex<S>>,
    state: Arc<Mutex<WraithState>>,
    tunnel_manager: Arc<TunnelManager>,
    wraith_id: String,
)
where
    S: tokio::io::AsyncRead + Unpin + Send,
{
    loop {
        let msg = {
            let mut stream_lock = stream.lock().await;
            match PeerSession::read_message(&mut *stream_lock).await {
                Ok(Some(msg)) => msg,
                Ok(None) => {
                    info!("Peer stream ended for {}", wraith_id);
                    break;
                }
                Err(e) => {
                    warn!("Error reading from peer stream: {}", e);
                    break;
                }
            }
        };

        let msg_id = msg.message_id.clone();
        let dedup_result = state.lock().expect("state lock poisoned").check_and_mark_seen(&msg_id);

        if dedup_result.already_seen {
            info!("Skipping duplicate message: {}", msg_id);
            continue;
        }

        if let Some(tx) = dedup_result.pending_tx {
            if tx.send(msg).is_err() {
                info!("Failed to send response for {}", msg_id);
            }
            continue;
        }

        if let Some(response) = tunnel_manager.route_message(msg).await {
            if let Some(session) = tunnel_manager.get_session(&wraith_id).await {
                if let Err(e) = session.command_tx.send(response).await {
                    warn!("Failed to send response back to peer {}: {}", wraith_id, e);
                }
            }
        }
    }
}
