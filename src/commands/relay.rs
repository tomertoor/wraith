use crate::commands::command::Command;
use crate::proto::wraith::{Command as ProtoCommand, CommandResult};
use crate::relay::{RelayConfig, RelayEndpoint, RelayManager, Transport};
use crate::wraith::state::WraithState;
use crate::wraith::tunnel::TunnelManager;
use log::{debug, info};
use std::sync::Arc;
use std::sync::Mutex;

pub struct RelayCommands {
    relay_manager: Arc<Mutex<RelayManager>>,
    tunnel_manager: Arc<TunnelManager>,
}

impl RelayCommands {
    pub fn new(relay_manager: Arc<Mutex<RelayManager>>, tunnel_manager: Arc<TunnelManager>) -> Self {
        Self { relay_manager, tunnel_manager }
    }

    pub fn handle_create_relay(&self, cmd: &ProtoCommand, local_wraith_id: &str) -> CommandResult {
        // Hops come as: hop_0_listen_host, hop_0_listen_port, hop_0_forward_host, hop_0_forward_port, hop_0_protocol, hop_1_...
        // Or legacy single-hop: listen_host, listen_port, forward_host, forward_port, protocol
        let mut hops: Vec<RelayConfig> = Vec::new();

        // Check if we have hop-based params (new format) or legacy format
        if let Some(first_listen_host) = cmd.params.get("hop_0_listen_host") {
            // New hop-based format
            let mut i = 0;
            while let Some(listen_host) = cmd.params.get(&format!("hop_{}_listen_host", i)) {
                let listen_port: u16 = cmd.params
                    .get(&format!("hop_{}_listen_port", i))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let forward_host = cmd.params
                    .get(&format!("hop_{}_forward_host", i))
                    .cloned()
                    .unwrap_or_default();
                let forward_port: u16 = cmd.params
                    .get(&format!("hop_{}_forward_port", i))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let protocol_str = cmd.params
                    .get(&format!("hop_{}_protocol", i))
                    .cloned()
                    .unwrap_or_else(|| "tcp".to_string());

                hops.push(RelayConfig::new(
                    RelayEndpoint::from_str(listen_host, listen_port, &protocol_str),
                    RelayEndpoint::from_str(&forward_host, forward_port, &protocol_str),
                ));
                i += 1;
            }
        } else {
            // Legacy single-hop format (backward compatible)
            let listen_host = cmd.params.get("listen_host").cloned().unwrap_or_default();
            let listen_port: u16 = cmd.params.get("listen_port").and_then(|s| s.parse().ok()).unwrap_or(0);
            let forward_host = cmd.params.get("forward_host").cloned().unwrap_or_default();
            let forward_port: u16 = cmd.params.get("forward_port").and_then(|s| s.parse().ok()).unwrap_or(0);
            let listen_protocol = cmd.params.get("listen_protocol").cloned().unwrap_or_else(|| "tcp".to_string());
            let forward_protocol = cmd.params.get("forward_protocol").cloned().unwrap_or_else(|| "tcp".to_string());

            hops.push(RelayConfig::new(
                RelayEndpoint::from_str(&listen_host, listen_port, &listen_protocol),
                RelayEndpoint::from_str(&forward_host, forward_port, &forward_protocol),
            ));
        }

        if hops.len() < 1 || (hops.len() == 1 && hops[0].listen.port == 0) {
            return CommandResult {
                command_id: cmd.command_id.clone(),
                status: "error".to_string(),
                output: String::new(),
                exit_code: -1,
                duration_ms: 0,
                error: "Invalid relay configuration: no hops provided".to_string(),
            };
        }

        // Check if this relay should be created on a remote wraith
        if let Some(target_id) = cmd.params.get("target_wraith_id") {
            if target_id != local_wraith_id {
                // Return status indicating this needs to be routed
                return CommandResult {
                    command_id: cmd.command_id.clone(),
                    status: "route_to_peer".to_string(),
                    output: target_id.clone(),
                    exit_code: 0,
                    duration_ms: 0,
                    error: format!("forward_to_peer:{}", target_id),
                };
            }
        }

        debug!("create_relay: {} hop(s)", hops.len());

        let relay_id = {
            let mut manager = self.relay_manager.lock().unwrap();
            manager.create_relay(hops.remove(0))
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
            "create_relay" => self.handle_create_relay(cmd, ""),
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