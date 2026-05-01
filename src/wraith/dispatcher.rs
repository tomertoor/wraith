use crate::commands::agent::AgentCommands;
use crate::commands::command::Command;
use crate::commands::relay::RelayCommands;
use crate::message::codec::MessageCodec;
use crate::proto::wraith::{MessageType, WraithMessage};
use crate::wraith::state::WraithState;
use log::{debug, info};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct MessageDispatcher {
    relay_commands: Arc<Mutex<RelayCommands>>,
    agent_commands: Arc<Mutex<AgentCommands>>,
}

impl MessageDispatcher {
    pub fn new(relay_commands: RelayCommands, agent_commands: AgentCommands) -> Self {
        Self {
            relay_commands: Arc::new(Mutex::new(relay_commands)),
            agent_commands: Arc::new(Mutex::new(agent_commands)),
        }
    }

    pub fn relay_commands(&self) -> Arc<Mutex<RelayCommands>> {
        Arc::clone(&self.relay_commands)
    }

    pub fn agent_commands(&self) -> Arc<Mutex<AgentCommands>> {
        Arc::clone(&self.agent_commands)
    }

    /// Route a message: check target_wraith_id, forward to peer or broadcast
    pub async fn route_message(
        &self,
        msg: WraithMessage,
        state: Arc<Mutex<WraithState>>,
    ) -> Option<WraithMessage> {
        let target = msg.target_wraith_id.clone();
        let local_id = state.lock().unwrap().wraith_id.clone();
        let msg_id = msg.message_id.clone();

        // If targeted to us, dispatch locally
        if target.is_empty() || target == local_id {
            return self.dispatch(msg, state).await;
        }

        // If targeted to a direct peer, forward to that peer and wait for response
        {
            let peer = state.lock().unwrap().peer_table.get(&target).cloned();
            debug!("Peer table keys: {:?}", state.lock().unwrap().peer_table.keys().collect::<Vec<_>>());
            debug!("Looking for target: {}", target);
            if let Some(peer) = peer {
                // Create oneshot channel for response
                let (response_tx, response_rx) = oneshot::channel::<WraithMessage>();

                // Register the pending response
                state.lock().unwrap().register_pending_response(msg_id.clone(), response_tx);

                // Clone message for sending
                let msg_clone = msg.clone();

                // Send to peer
                debug!("Passing message to direct peer {}", peer.wraith_id);
                let send_result = peer.sender.try_send(msg_clone);
                if send_result.is_ok() {
                    debug!("Message queued for peer {}", peer.wraith_id);
                    // Wait for response from the peer
                    match response_rx.await {
                        Ok(response) => {
                            debug!("Received answer for peer forwarding from {}.", peer.wraith_id);
                            // Unregister the pending response
                            state.lock().unwrap().take_pending_response(&msg_id);
                            return Some(response);
                        }
                        Err(_) => {
                            // Peer died or response lost
                            info!("Peer response channel closed for message: {}", msg_id);
                            state.lock().unwrap().take_pending_response(&msg_id);
                        }
                    }
                } else {
                    // Send failed, remove pending response
                    info!("Failed to send to peer {}: {:?}", peer.wraith_id, send_result.err());
                    state.lock().unwrap().take_pending_response(&msg_id);
                }
            }
        }

        debug!("Sending command to all peers");
        // Broadcast to all peers (except already-seen check done in caller)
        let peers: Vec<_> = state.lock().unwrap().peer_table.values().cloned().collect();
        for peer in peers {
            let _ = peer.sender.send(msg.clone()).await;
        }

        // Broadcast doesn't wait for response
        Some(MessageCodec::create_command_result(
            "".to_string(),
            "broadcast".to_string(),
            "".to_string(),
            0, 0, "".to_string(),
        ))
    }

    pub async fn dispatch(&self, msg: WraithMessage, state: Arc<Mutex<WraithState>>) -> Option<WraithMessage> {
        let msg_type = msg.msg_type;
        info!("Dispatching message of type: {:?}", msg_type);

        if msg_type == MessageType::Command as i32 {
            if let Some(crate::proto::wraith::wraith_message::Payload::Command(cmd)) = &msg.payload {
                let result = if cmd.action == "create_relay" {
                    let relay_commands = self.relay_commands.lock().unwrap();
                    let local_wraith_id = state.lock().unwrap().wraith_id.clone();
                    relay_commands.handle_create_relay(cmd, &local_wraith_id)
                } else if cmd.action == "delete_relay" || cmd.action == "list_relays" {
                    self.relay_commands.lock().unwrap().execute(cmd)
                } else if cmd.action == "set_id" {
                    self.agent_commands.lock().unwrap().handle_set_id(cmd, &mut state.lock().unwrap())
                } else if cmd.action == "list_peers" {
                    self.agent_commands.lock().unwrap().handle_list_peers(cmd, &state.lock().unwrap())
                } else if cmd.action == "wraith_listen" {
                    self.agent_commands.lock().unwrap().handle_wraith_listen(cmd)
                } else if cmd.action == "wraith_connect" {
                    self.agent_commands.lock().unwrap().handle_wraith_connect(cmd, &state.lock().unwrap())
                } else {
                    return None;
                };

                state.lock().unwrap().increment_commands();

                return Some(MessageCodec::create_command_result(
                    result.command_id,
                    result.status,
                    result.output,
                    result.exit_code,
                    result.duration_ms,
                    result.error,
                ));
            }
        }
        None
    }
}

impl Default for MessageDispatcher {
    fn default() -> Self {
        Self::new(
            RelayCommands::new(
                Arc::new(Mutex::new(crate::relay::RelayManager::new())),
                Arc::new(crate::wraith::TunnelManager::new()),
            ),
            AgentCommands::new(Arc::new(crate::wraith::TunnelManager::new())),
        )
    }
}
