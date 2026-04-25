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
}

impl MessageDispatcher {
    pub fn new(relay_commands: RelayCommands) -> Self {
        Self {
            handlers: std::collections::HashMap::new(),
            relay_commands: Arc::new(Mutex::new(relay_commands)),
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
        let msg_type = msg.msg_type();
        info!("Dispatching message of type: {:?}", msg_type);

        if msg_type == MessageType::Command {
            if let Some(crate::proto::wraith::wraith_message::Payload::Command(cmd)) = &msg.payload {
                let relay_commands = self.relay_commands.lock().unwrap();
                let local_wraith_id = state.lock().unwrap().wraith_id.clone();

                // For create_relay, pass local_wraith_id to check if routing is needed
                let result = if cmd.action == "create_relay" {
                    relay_commands.handle_create_relay(cmd, &local_wraith_id)
                } else {
                    relay_commands.execute(cmd)
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

        if let Some(handler) = self.handlers.get(&msg_type) {
            Some(handler(msg, state).await)
        } else {
            None
        }
    }
}

impl Default for MessageDispatcher {
    fn default() -> Self {
        use std::sync::Mutex;
        Self::new(RelayCommands::new(Arc::new(Mutex::new(crate::relay::RelayManager::new()))))
    }
}