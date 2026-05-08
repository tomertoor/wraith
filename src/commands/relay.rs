use crate::message::codec::MessageCodec;
use crate::proto::wraith::{Command as ProtoCommand, CommandResult};
use crate::relay::{RelayConfig, RelayEndpoint, RelayManager};
use crate::wraith::session::TunnelManager;
use log::{debug, info};
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone)]
pub struct RelayCommands {
    relay_manager: Arc<Mutex<RelayManager>>,
    #[allow(dead_code)]
    tunnel_manager: Option<Arc<TunnelManager>>,
}

impl RelayCommands {
    pub fn new(relay_manager: Arc<Mutex<RelayManager>>, tunnel_manager: Arc<TunnelManager>) -> Self {
        Self { relay_manager, tunnel_manager: Some(tunnel_manager) }
    }

    pub fn new_without_tunnel(relay_manager: Arc<Mutex<RelayManager>>) -> Self {
        Self { relay_manager, tunnel_manager: None }
    }

    pub fn handle_create_relay(&self, cmd: &ProtoCommand, local_wraith_id: &str) -> CommandResult {
        let mut hops: Vec<RelayConfig> = Vec::new();

        if let Some(first_listen_host) = cmd.params.get("hop_0_listen_host") {
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
            return MessageCodec::command_result_error(
                cmd.command_id.clone(),
                "Invalid relay configuration: no hops provided".to_string(),
            );
        }

        if let Some(target_id) = cmd.params.get("target_wraith_id") {
            if target_id != local_wraith_id {
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
            let mut manager = self.relay_manager.lock().expect("relay_manager lock poisoned");
            manager.create_relay(hops.remove(0))
        };

        info!("Created relay with id: {}", relay_id);
        MessageCodec::command_result_success(cmd.command_id.clone(), relay_id)
    }

    pub fn handle_delete_relay(&self, cmd: &ProtoCommand) -> CommandResult {
        let relay_id = cmd.params.get("relay_id").cloned().unwrap_or_default();
        debug!("delete_relay: id={}", relay_id);

        let deleted = {
            let mut manager = self.relay_manager.lock().expect("relay_manager lock poisoned");
            manager.delete_relay(&relay_id)
        };

        if deleted {
            info!("Deleted relay: {}", relay_id);
            MessageCodec::command_result_success(cmd.command_id.clone(), String::new())
        } else {
            info!("Relay not found: {}", relay_id);
            MessageCodec::command_result_error(cmd.command_id.clone(), "Relay not found".to_string())
        }
    }

    pub fn handle_list_relays(&self, cmd: &ProtoCommand) -> CommandResult {
        let relays = {
            let manager = self.relay_manager.lock().expect("relay_manager lock poisoned");
            manager.list_relays()
        };

        let output = serde_json::to_string(&relays).unwrap_or_default();
        MessageCodec::command_result_success(cmd.command_id.clone(), output)
    }
}
