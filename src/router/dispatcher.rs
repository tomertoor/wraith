use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use log::{debug, info};
use tokio::sync::{oneshot, RwLock};

use crate::commands::agent::AgentCommands;
use crate::commands::relay::RelayCommands;
use crate::message::codec::MessageCodec;
use crate::proto::wraith::{Command, MessageType, WraithMessage};
use crate::wraith::session::PeerSession;
use crate::wraith::state::WraithState;

/// Dispatches commands and routes messages between C2 connections and peer sessions.
///
/// `Router` holds references to command handlers and shared state. It does not own
/// peer sessions -- those are managed by [`TunnelManager`](crate::wraith::session::TunnelManager).
/// The [`route_message`](Router::route_message) method accepts a sessions map as a
/// parameter so that the session registry remains the single source of truth.
pub struct Router {
    relay_commands: Arc<Mutex<RelayCommands>>,
    agent_commands: Arc<Mutex<AgentCommands>>,
    state: Arc<Mutex<WraithState>>,
}

impl Router {
    /// Create a new Router with the given shared state and command handlers.
    ///
    /// Command handlers must already be fully constructed (including their
    /// `TunnelManager` references). Use [`TunnelManager::set_commands`] for
    /// the two-phase initialization that resolves the circular dependency
    /// between `TunnelManager` and command types.
    pub fn new(
        state: Arc<Mutex<WraithState>>,
        relay_commands: Arc<Mutex<RelayCommands>>,
        agent_commands: Arc<Mutex<AgentCommands>>,
    ) -> Self {
        Self {
            relay_commands,
            agent_commands,
            state,
        }
    }

    /// Dispatch a [`Command`] protobuf to the appropriate handler.
    ///
    /// Returns a `CommandResult` wrapped in `WraithMessage`, or `None` if the
    /// action string is unrecognized.
    pub fn dispatch_command(&self, cmd: &Command) -> Option<WraithMessage> {
        let result = if cmd.action == "create_relay" {
            let relay_cmds = self.relay_commands.lock().expect("relay_commands lock poisoned");
            let local_wraith_id = self.state.lock().expect("state lock poisoned").wraith_id.clone();
            relay_cmds.handle_create_relay(cmd, &local_wraith_id)
        } else if cmd.action == "delete_relay" {
            self.relay_commands.lock().expect("relay_commands lock poisoned").handle_delete_relay(cmd)
        } else if cmd.action == "list_relays" {
            self.relay_commands.lock().expect("relay_commands lock poisoned").handle_list_relays(cmd)
        } else if cmd.action == "set_id" {
            self.agent_commands.lock().expect("agent_commands lock poisoned")
                .handle_set_id(cmd, &mut self.state.lock().expect("state lock poisoned"))
        } else if cmd.action == "list_peers" {
            self.agent_commands.lock().expect("agent_commands lock poisoned")
                .handle_list_peers(cmd, &self.state.lock().expect("state lock poisoned"))
        } else if cmd.action == "wraith_listen" {
            self.agent_commands.lock().expect("agent_commands lock poisoned").handle_wraith_listen(cmd)
        } else if cmd.action == "wraith_connect" {
            self.agent_commands.lock().expect("agent_commands lock poisoned")
                .handle_wraith_connect(cmd, &self.state.lock().expect("state lock poisoned"))
        } else {
            return None;
        };

        self.state.lock().expect("state lock poisoned").increment_commands();

        Some(MessageCodec::create_command_result(
            result.command_id,
            result.status,
            result.output,
            result.exit_code,
            result.duration_ms,
            result.error,
        ))
    }

    /// Dispatch a message locally.
    ///
    /// Currently only [`MessageType::Command`] messages are handled; all other
    /// message types return `None`.
    pub fn dispatch(&self, msg: WraithMessage) -> Option<WraithMessage> {
        let original_msg_id = msg.message_id.clone();
        let msg_type = msg.msg_type;
        info!("Dispatching message of type: {:?}", msg_type);

        if msg_type == MessageType::Command as i32 {
            if let Some(crate::proto::wraith::wraith_message::Payload::Command(cmd)) = &msg.payload {
                let mut response = self.dispatch_command(cmd)?;
                response.message_id = original_msg_id;
                return Some(response);
            }
        }
        None
    }

    /// Route a message: check `target_wraith_id`, forward to a peer, or dispatch locally.
    ///
    /// Routing priority:
    /// 1. Empty target or target matching the local wraith ID -> local dispatch.
    /// 2. Target is a direct peer in the peer table -> forward via the peer's
    ///    command channel and await the response.
    /// 3. Otherwise -> broadcast to all known peer sessions and return a
    ///    broadcast acknowledgement.
    pub async fn route_message(
        &self,
        msg: WraithMessage,
        sessions: &Arc<RwLock<HashMap<String, PeerSession>>>,
    ) -> Option<WraithMessage> {
        let target = msg.target_wraith_id.clone();
        let local_id = self.state.lock().expect("state lock poisoned").wraith_id.clone();
        let msg_id = msg.message_id.clone();

        // If targeted to us, dispatch locally
        if target.is_empty() || target == local_id {
            return self.dispatch(msg);
        }

        // If targeted to a direct peer, forward and wait for response
        let peer = {
            let peer_table = self.state.lock().expect("state lock poisoned").peer_table.clone();
            peer_table.get(&target).cloned()
        };

        if let Some(peer) = peer {
            let (response_tx, response_rx) = oneshot::channel::<WraithMessage>();

            self.state.lock().expect("state lock poisoned")
                .register_pending_response(msg_id.clone(), response_tx);

            let msg_clone = msg.clone();
            debug!("Passing message to direct peer {}", peer.wraith_id);
            let send_result = peer.sender.send(msg_clone).await;
            if send_result.is_ok() {
                match response_rx.await {
                    Ok(response) => {
                        debug!("Received answer for peer forwarding from {}.", peer.wraith_id);
                        self.state.lock().expect("state lock poisoned").take_pending_response(&msg_id);
                        return Some(response);
                    }
                    Err(_) => {
                        info!("Peer response channel closed for message: {}", msg_id);
                        self.state.lock().expect("state lock poisoned").take_pending_response(&msg_id);
                    }
                }
            } else {
                info!("Failed to send to peer {}: {:?}", peer.wraith_id, send_result.err());
                self.state.lock().expect("state lock poisoned").take_pending_response(&msg_id);
            }
        }

        debug!("Sending command to all peers");
        // Broadcast to all sessions
        let peers: Vec<_> = {
            let sessions_guard = sessions.read().await;
            sessions_guard.values().cloned().collect()
        };
        for peer in peers {
            let _ = peer.command_tx.send(msg.clone()).await;
        }

        Some(MessageCodec::create_command_result(
            "".to_string(),
            "broadcast".to_string(),
            "".to_string(),
            0, 0, "".to_string(),
        ))
    }

    /// Replace the command handlers.
    ///
    /// Used for two-phase initialization: the `Router` is created with placeholder
    /// handlers via [`RelayCommands::new_without_tunnel`] and
    /// [`AgentCommands::new_without_tunnel`], then real handlers are injected once
    /// the `TunnelManager` (which the handlers reference) has been fully constructed.
    pub fn set_commands(&self, relay_commands: RelayCommands, agent_commands: AgentCommands) {
        *self.relay_commands.lock().expect("relay_commands lock poisoned") = relay_commands;
        *self.agent_commands.lock().expect("agent_commands lock poisoned") = agent_commands;
    }
}
