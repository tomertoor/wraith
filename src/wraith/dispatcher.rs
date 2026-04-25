use crate::commands::agent::AgentCommands;
use crate::commands::command::Command;
use crate::commands::relay::RelayCommands;
use crate::message::codec::MessageCodec;
use crate::proto::wraith::{MessageType, WraithMessage};
use crate::wraith::state::WraithState;
use log::info;
use std::sync::{Arc, Mutex};

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

        // If targeted to us, dispatch locally
        if target.is_empty() || target == local_id {
            return self.dispatch(msg, state).await;
        }

        // If targeted to a direct peer, forward to that peer
        {
            let peer = state.lock().unwrap().peer_table.get(&target).cloned();
            if let Some(peer) = peer {
                // Clone to allow falling through to broadcast if send fails
                if peer.sender.send(msg.clone()).await.is_ok() {
                    return Some(MessageCodec::create_command_result(
                        "".to_string(),
                        "forwarded".to_string(),
                        "".to_string(),
                        0, 0, "".to_string(),
                    ));
                }
            }
        }

        // Broadcast to all peers (except already-seen check done in caller)
        let peers: Vec<_> = state.lock().unwrap().peer_table.values().cloned().collect();
        for peer in peers {
            let _ = peer.sender.send(msg.clone()).await;
        }

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
