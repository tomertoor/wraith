use crate::commands::agent::AgentCommands;
use crate::commands::command::Command;
use crate::commands::relay::RelayCommands;
use crate::message::codec::MessageCodec;
use crate::proto::wraith::{MessageType, WraithMessage};
use crate::wraith::state::WraithState;
use log::info;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

type Handler = Box<dyn Fn(WraithMessage, Arc<Mutex<WraithState>>) -> Pin<Box<dyn Future<Output = WraithMessage> + Send>> + Send>;

pub struct MessageDispatcher {
    handlers: std::collections::HashMap<MessageType, Handler>,
    relay_commands: Arc<Mutex<RelayCommands>>,
    agent_commands: Arc<Mutex<AgentCommands>>,
}

impl MessageDispatcher {
    pub fn new(relay_commands: RelayCommands, agent_commands: AgentCommands) -> Self {
        Self {
            handlers: std::collections::HashMap::new(),
            relay_commands: Arc::new(Mutex::new(relay_commands)),
            agent_commands: Arc::new(Mutex::new(agent_commands)),
        }
    }

    pub fn register<F, Fut>(&mut self, msg_type: MessageType, handler: F)
    where
        F: Fn(WraithMessage, Arc<Mutex<WraithState>>) -> Pin<Box<dyn Future<Output = WraithMessage> + Send>> + Send + 'static,
        Fut: Future<Output = WraithMessage> + Send + 'static,
    {
        self.handlers.insert(msg_type, Box::new(handler));
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

        if let Some(handler) = self.handlers.get(&MessageType::try_from(msg_type).unwrap()) {
            Some(handler(msg, state).await)
        } else {
            None
        }
    }
}

impl Default for MessageDispatcher {
    fn default() -> Self {
        use std::sync::Mutex;
        Self::new(
            RelayCommands::new(
                Arc::new(Mutex::new(crate::relay::RelayManager::new())),
                Arc::new(crate::wraith::TunnelManager::new()),
            ),
            AgentCommands::new(Arc::new(crate::wraith::TunnelManager::new())),
        )
    }
}
