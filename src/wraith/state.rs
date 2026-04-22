use crate::relay::RelayManager;
use std::sync::{Arc, Mutex};

pub struct WraithState {
    pub relay_manager: Arc<Mutex<RelayManager>>,
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

        Self {
            relay_manager: Arc::new(Mutex::new(RelayManager::new())),
            hostname,
            username,
            os,
            ip_address,
            commands_executed: 0,
            last_command_time: 0,
            connected: false,
        }
    }

    pub fn new_with_relay_manager(relay_manager: Arc<Mutex<RelayManager>>) -> Self {
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
}

impl Default for WraithState {
    fn default() -> Self {
        Self::new()
    }
}