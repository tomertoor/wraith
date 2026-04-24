use crate::commands::command::Command;
use crate::proto::wraith::{Command as ProtoCommand, CommandResult};
use crate::relay::{RelayConfig, RelayManager, Transport};
use log::{debug, info};
use std::sync::Arc;
use std::sync::Mutex;

pub struct RelayCommands {
    relay_manager: Arc<Mutex<RelayManager>>,
}

impl RelayCommands {
    pub fn new(relay_manager: Arc<Mutex<RelayManager>>) -> Self {
        Self { relay_manager }
    }

    pub fn handle_create_relay(&self, cmd: &ProtoCommand) -> CommandResult {
        let listen_host = cmd.params.get("listen_host").cloned().unwrap_or_default();
        let listen_port: i32 = cmd.params.get("listen_port").and_then(|s| s.parse().ok()).unwrap_or(0);
        let forward_host = cmd.params.get("forward_host").cloned().unwrap_or_default();
        let forward_port: i32 = cmd.params.get("forward_port").and_then(|s| s.parse().ok()).unwrap_or(0);
        let protocol_str = cmd.params.get("protocol").cloned().unwrap_or_else(|| "tcp".to_string());
        let protocol = Transport::from_str(&protocol_str);

        debug!("create_relay: listen={}:{}, forward={}:{}, protocol={}", listen_host, listen_port, forward_host, forward_port, protocol);

        let config = RelayConfig::new(
            listen_host.clone(),
            listen_port as u16,
            forward_host.clone(),
            forward_port as u16,
            protocol,
        );

        let relay_id = {
            let mut manager = self.relay_manager.lock().unwrap();
            manager.create_relay(config)
        };

        info!("Created relay with id: {}", relay_id);
        CommandResult {
            command_id: cmd.command_id.clone(),
            status: "success".to_string(),
            output: relay_id,
            exit_code: 0,
            duration_ms: 0,
            error: String::new(),
        }
    }

    pub fn handle_delete_relay(&self, cmd: &ProtoCommand) -> CommandResult {
        let relay_id = cmd.params.get("relay_id").cloned().unwrap_or_default();
        debug!("delete_relay: id={}", relay_id);

        let deleted = {
            let mut manager = self.relay_manager.lock().unwrap();
            manager.delete_relay(&relay_id)
        };

        if deleted {
            info!("Deleted relay: {}", relay_id);
        } else {
            info!("Relay not found: {}", relay_id);
        }

        CommandResult {
            command_id: cmd.command_id.clone(),
            status: if deleted { "success" } else { "not_found" }.to_string(),
            output: String::new(),
            exit_code: if deleted { 0 } else { -1 },
            duration_ms: 0,
            error: if deleted { String::new() } else { "Relay not found".to_string() },
        }
    }

    pub fn handle_list_relays(&self, cmd: &ProtoCommand) -> CommandResult {
        let relays = {
            let manager = self.relay_manager.lock().unwrap();
            manager.list_relays()
        };

        let output = serde_json::to_string(&relays).unwrap_or_default();

        CommandResult {
            command_id: cmd.command_id.clone(),
            status: "success".to_string(),
            output,
            exit_code: 0,
            duration_ms: 0,
            error: String::new(),
        }
    }
}

impl Command for RelayCommands {
    fn execute(&self, cmd: &ProtoCommand) -> CommandResult {
        match cmd.action.as_str() {
            "create_relay" => self.handle_create_relay(cmd),
            "delete_relay" => self.handle_delete_relay(cmd),
            "list_relays" => self.handle_list_relays(cmd),
            _ => CommandResult {
                command_id: cmd.command_id.clone(),
                status: "error".to_string(),
                output: String::new(),
                exit_code: -1,
                duration_ms: 0,
                error: format!("Unknown action: {}", cmd.action),
            },
        }
    }
}