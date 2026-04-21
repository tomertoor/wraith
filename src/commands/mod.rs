use crate::proto::wraith::{Command, CommandResult};
use crate::relay::RelayManager;
use anyhow::Result;
use log::info;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct CommandHandler {
    relay_manager: Arc<Mutex<RelayManager>>,
}

impl CommandHandler {
    pub fn new(relay_manager: Arc<Mutex<RelayManager>>) -> Self {
        Self { relay_manager }
    }

    pub async fn handle_command(&self, cmd: Command) -> Result<CommandResult> {
        info!("Handling command: {} (id: {})", cmd.action, cmd.command_id);

        match cmd.action.as_str() {
            "create_relay" => self.handle_create_relay(cmd).await,
            "delete_relay" => self.handle_delete_relay(cmd).await,
            "list_relays" => self.handle_list_relays(cmd).await,
            "wraith_listen" => self.handle_wraith_listen(cmd).await,
            "wraith_connect" => self.handle_wraith_connect(cmd).await,
            _ => Ok(CommandResult {
                command_id: cmd.command_id,
                status: "error".to_string(),
                output: "".to_string(),
                exit_code: -1,
                duration_ms: 0,
                error: format!("Unknown action: {}", cmd.action),
            }),
        }
    }

    async fn handle_create_relay(&self, cmd: Command) -> Result<CommandResult> {
        let listen_host = cmd.params.get("listen_host").cloned().unwrap_or_default();
        let listen_port: i32 = cmd.params.get("listen_port").and_then(|s| s.parse().ok()).unwrap_or(0);
        let forward_host = cmd.params.get("forward_host").cloned().unwrap_or_default();
        let forward_port: i32 = cmd.params.get("forward_port").and_then(|s| s.parse().ok()).unwrap_or(0);

        let config = crate::relay::RelayConfig {
            listen_host,
            listen_port: listen_port as u16,
            forward_host,
            forward_port: forward_port as u16,
        };

        let relay_id = {
            let mut manager = self.relay_manager.lock().await;
            manager.create_relay(config)
        };

        Ok(CommandResult {
            command_id: cmd.command_id,
            status: "success".to_string(),
            output: relay_id,
            exit_code: 0,
            duration_ms: 0,
            error: "".to_string(),
        })
    }

    async fn handle_delete_relay(&self, cmd: Command) -> Result<CommandResult> {
        let relay_id = cmd.params.get("relay_id").cloned().unwrap_or_default();

        let deleted = {
            let mut manager = self.relay_manager.lock().await;
            manager.delete_relay(&relay_id)
        };

        Ok(CommandResult {
            command_id: cmd.command_id,
            status: if deleted { "success" } else { "not_found" }.to_string(),
            output: "".to_string(),
            exit_code: if deleted { 0 } else { -1 },
            duration_ms: 0,
            error: if deleted { "".to_string() } else { "Relay not found".to_string() },
        })
    }

    async fn handle_list_relays(&self, cmd: Command) -> Result<CommandResult> {
        let relays = {
            let manager = self.relay_manager.lock().await;
            manager.list_relays()
        };

        let output = serde_json::to_string(&relays).unwrap_or_default();

        Ok(CommandResult {
            command_id: cmd.command_id,
            status: "success".to_string(),
            output,
            exit_code: 0,
            duration_ms: 0,
            error: "".to_string(),
        })
    }

    async fn handle_wraith_listen(&self, cmd: Command) -> Result<CommandResult> {
        // Stage 2 - placeholder for now
        Ok(CommandResult {
            command_id: cmd.command_id,
            status: "error".to_string(),
            output: "".to_string(),
            exit_code: -1,
            duration_ms: 0,
            error: "wraith_listen not implemented in Stage 1".to_string(),
        })
    }

    async fn handle_wraith_connect(&self, cmd: Command) -> Result<CommandResult> {
        // Stage 2 - placeholder for now
        Ok(CommandResult {
            command_id: cmd.command_id,
            status: "error".to_string(),
            output: "".to_string(),
            exit_code: -1,
            duration_ms: 0,
            error: "wraith_connect not implemented in Stage 1".to_string(),
        })
    }
}