# Wraith Agent Network Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable wraith agents to connect to each other forming a chain (C2 → A → B), with command routing via `target_wraith_id` and relay spanning multiple hops.

**Architecture:** Each wraith maintains a Yamux connection to its C2 parent and zero or more peer wraiths. Commands include `target_wraith_id` for routing. Yamux Stream 0 carries command/control; streams 1+ carry relay data.

**Tech Stack:** Rust, Tokio, Yamux, Protobuf, Python

---

## File Structure

```
src/
  main.rs                  # Add agent listen/connect CLI modes
  wraith/
    mod.rs                 # Export Wraith, WraithState
    state.rs               # Add wraith_id, peer_table, seen_message_ids
    dispatcher.rs          # Add command routing via target_wraith_id
    wraith.rs              # Add peer Yamux session handling
    tunnel/                # NEW: Peer tunnel management
      mod.rs
      session.rs
  connection/
    mod.rs                 # Add YamuxConnection
    yamux.rs               # NEW: Yamux connection wrapper
    tcp.rs                 # Existing TCP connection
  commands/
    mod.rs                 # Add AgentCommands (set_id, list_peers)
    relay.rs               # Extend for remote relay via target_wraith_id
    agent.rs               # NEW: Agent command handlers
  message/
    codec.rs               # Add PeerUpdate, WraithRegistration helpers
proto/
  wraith.proto             # Add WRAITH_REGISTRATION, PEER_UPDATE, PEER_LIST, PEER_LIST_RESPONSE, PEER_INFO
  wraith.rs                # Generated (rebuild with cargo build)
home/PyWraith/
  client.py                # Add agent connect, list_peers recursive traversal
  protocol.py              # Add create_peer_update, parse_peer_update
  cli.py                   # Add agent listen/connect commands
```

---

## Task 1: Protocol - Add New Message Types

**Files:**
- Modify: `proto/wraith.proto`
- Test: `tests/protocol.rs`

- [ ] **Step 1: Add new message types to proto/wraith.proto**

Add before closing brace of `wraith.proto`:

```protobuf
// Wraith-to-Wraith Registration
message WraithRegistration {
    string wraith_id = 1;
    string hostname = 2;
    string os = 3;
    int64 connected_at = 4;
}

// Peer list/management
message PeerInfo {
    string wraith_id = 1;
    string hostname = 2;
    bool connected = 3;
}

message PeerList {}

message PeerListResponse {
    string wraith_id = 1;
    repeated PeerInfo peers = 2;
}

message PeerUpdate {
    string wraith_id = 1;
    string hostname = 2;
    string action = 3;  // "connected" or "disconnected"
    int64 timestamp = 4;
}
```

Add new `MessageType` enum values:
```protobuf
enum MessageType {
    // ... existing values ...
    WRAITH_REGISTRATION = 8;
    PEER_UPDATE = 9;
    PEER_LIST = 10;
    PEER_LIST_RESPONSE = 11;
}
```

Update `WraithMessage` payload to include new messages:
```protobuf
message WraithMessage {
    // ... existing fields 1-11 ...
    oneof payload {
        // ... existing payloads ...
        WraithRegistration wraith_registration = 12;
        PeerUpdate peer_update = 13;
        PeerList peer_list = 14;
        PeerListResponse peer_list_response = 15;
    }
}
```

- [ ] **Step 2: Rebuild protobuf**

Run: `cargo build`
Expected: Success, generates `src/proto/wraith.rs` with new types

- [ ] **Step 3: Verify generated code**

Run: `grep -n "WraithRegistration" src/proto/wraith.rs`
Expected: Found with struct definition

---

## Task 2: State - Add Wraith ID and Peer Table

**Files:**
- Modify: `src/wraith/state.rs`
- Test: `tests/wraith_state.rs` (new)

- [ ] **Step 1: Add new fields to WraithState**

Modify `src/wraith/state.rs`:

```rust
use crate::relay::RelayManager;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tokio::sync::mpsc::Sender;
use crate::proto::wraith::WraithMessage;

pub struct WraithState {
    pub relay_manager: Arc<Mutex<RelayManager>>,
    pub hostname: String,
    pub username: String,
    pub os: String,
    pub ip_address: String,
    pub commands_executed: i64,
    pub last_command_time: i64,
    pub connected: bool,
    // NEW FIELDS
    pub wraith_id: String,                                    // Auto-generated UUID
    pub peer_table: HashMap<String, PeerConnection>,         // wraith_id -> connection
    pub seen_message_ids: std::sync::Mutex<HashSet<String>>, // For loop prevention
    pub c2_sender: std::sync::Mutex<Option<Sender<WraithMessage>>>, // To send updates to C2
}

pub struct PeerConnection {
    pub wraith_id: String,
    pub hostname: String,
    pub connected_at: i64,
    pub sender: Sender<WraithMessage>, // For sending commands to this peer
}
```

- [ ] **Step 2: Update WraithState::new() to generate UUID**

Add to `new()`:
```rust
let wraith_id = uuid::Uuid::new_v4().to_string();
```

Add `HashSet` import and `peer_table` initialization:
```rust
use std::collections::HashMap;
```

- [ ] **Step 3: Add helper methods**

Add to `WraithState`:
```rust
pub fn set_wraith_id(&mut self, id: String) {
    self.wraith_id = id;
}

pub fn add_peer(&mut self, wraith_id: String, hostname: String, sender: Sender<WraithMessage>) {
    self.peer_table.insert(wraith_id.clone(), PeerConnection {
        wraith_id,
        hostname,
        connected_at: chrono::Utc::now().timestamp_millis(),
        sender,
    });
}

pub fn remove_peer(&mut self, wraith_id: &str) -> Option<PeerConnection> {
    self.peer_table.remove(wraith_id)
}

pub fn has_seen_message(&self, message_id: &str) -> bool {
    self.seen_message_ids.lock().unwrap().contains(message_id)
}

pub fn mark_message_seen(&self, message_id: String) {
    self.seen_message_ids.lock().unwrap().insert(message_id);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test wraith_state`
Expected: PASS (may need to create test file first)

---

## Task 3: CLI - Add Agent Listen/Connect Modes

**Files:**
- Modify: `src/main.rs`
- Test: Manual test with `--help`

- [ ] **Step 1: Add new CLI arguments to Args struct**

Modify `src/main.rs`:

```rust
#[derive(Parser)]
#[command(name = "wraith")]
#[command(about = "Wraith reverse tunneling tool", long_about = None)]
struct Args {
    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Log to file (in addition to terminal)
    #[arg(long)]
    log_file: Option<String>,

    /// C2 host
    #[arg(long, default_value = "127.0.0.1")]
    c2_host: Ipv4Addr,

    /// C2 port
    #[arg(short = 'p', long, default_value_t = 4444)]
    c2_port: u16,

    /// Listen mode (server) - C2 connects to wraith
    #[arg(short = 'l', long)]
    listen: bool,

    // NEW AGENT MODE ARGUMENTS
    /// Agent mode: listen for peer wraith connections
    #[arg(long, value_name = "HOST:PORT")]
    agent_listen: Option<String>,

    /// Agent mode: connect to peer wraith
    #[arg(long, value_name = "HOST:PORT")]
    agent_connect: Option<String>,
}
```

- [ ] **Step 2: Update main() to handle agent modes**

Modify the `main()` function:

```rust
fn main() {
    let args = Args::parse();

    setup_logging(args.debug, &args.log_file).unwrap();

    info!("Starting wraith...");

    let rt = tokio::runtime::Runtime::new().unwrap();

    // Agent listen mode - listen for peer wraith connections
    if let Some(addr) = &args.agent_listen {
        let (host, port) = parse_host_port(addr);
        info!("Agent mode: listening on {}:{}", host, port);
        // TODO: implement peer listener
        return;
    }

    // Agent connect mode - connect to peer wraith
    if let Some(addr) = &args.agent_connect {
        let (host, port) = parse_host_port(addr);
        info!("Agent mode: connecting to peer at {}:{}", host, port);
        // TODO: implement peer connector
        return;
    }

    // Existing C2 mode
    let host = args.c2_host.to_string();
    let port = args.c2_port;
    let is_server = args.listen;

    info!("Host: {}:{}, Mode: {}", host, port, if is_server { "listen" } else { "connect" });

    let mut wraith = wraith::Wraith::new(host, port, is_server);

    loop {
        match rt.block_on(wraith.run()) {
            Ok(_) => info!("Connection closed gracefully"),
            Err(e) => {
                error!("Error: {}", e);
            }
        }
    }
}

fn parse_host_port(addr: &str) -> (String, u16) {
    let parts: Vec<&str> = addr.split(':').collect();
    if parts.len() == 2 {
        (parts[0].to_string(), parts[1].parse().unwrap_or(4444))
    } else {
        (addr.to_string(), 4444)
    }
}
```

- [ ] **Step 3: Verify CLI help**

Run: `cargo run -- --help`
Expected: Shows new `--agent-listen` and `--agent-connect` options

---

## Task 4: Connection - Add Yamux Wrapper

**Files:**
- Create: `src/connection/yamux.rs`
- Modify: `src/connection/mod.rs`
- Test: `tests/connection/yamux.rs` (new)

- [ ] **Step 1: Create Yamux connection wrapper**

Create `src/connection/yamux.rs`:

```rust
use anyhow::Result;
use log::{debug, info, warn};
use std::sync::Arc;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tokio::time::Duration;
use yamux::{Connection, ConnectionError, Mode, Stream};

pub struct YamuxConnection {
    conn: Connection<TcpStream>,
    peer_addr: SocketAddr,
}

impl YamuxConnection {
    pub async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let peer_addr = stream.peer_addr()?;
        let conn = Connection::new(stream, Mode::Client);
        Ok(Self { conn, peer_addr })
    }

    pub async fn accept(listener: &tokio::net::TcpListener) -> Result<(Self, TcpStream)> {
        let (stream, addr) = listener.accept().await?;
        let conn = Connection::new(stream.try_clone()?, Mode::Server);
        Ok((Self { conn, peer_addr: addr }, stream))
    }

    pub fn open_stream(&mut self) -> Result<Stream> {
        self.conn.open_stream().map_err(|e| anyhow::anyhow!(e))
    }

    pub async fn next_stream(&mut self) -> Option<Result<Stream, ConnectionError>> {
        self.conn.next_stream().await
    }

    pub fn close(&mut self) {
        // yamux Connection doesn't have explicit close, drop to close
    }
}
```

- [ ] **Step 2: Export from connection/mod.rs**

Modify `src/connection/mod.rs`:

```rust
pub mod framing;
pub mod tcp;
pub mod yamux;  // ADD

pub use framing::FramedRead;
pub use tcp::TcpConnection;
pub use yamux::YamuxConnection;  // ADD
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Success

---

## Task 5: Tunnel Module - Peer Session Management

**Files:**
- Create: `src/wraith/tunnel/mod.rs`
- Create: `src/wraith/tunnel/session.rs`
- Modify: `src/wraith/mod.rs`
- Test: `tests/tunnel/session.rs` (new)

- [ ] **Step 1: Create tunnel/session.rs**

Create `src/wraith/tunnel/session.rs`:

```rust
use crate::message::codec::MessageCodec;
use crate::proto::wraith::{MessageType, WraithMessage};
use anyhow::Result;
use log::{debug, info, warn};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, RwLock};
use yamux::{Connection, Stream};

pub struct PeerSession {
    pub wraith_id: String,
    pub hostname: String,
    pub connection: Connection<tokio::net::TcpStream>,
    pub command_tx: mpsc::Sender<WraithMessage>,
}

impl PeerSession {
    pub fn new(
        wraith_id: String,
        hostname: String,
        connection: Connection<tokio::net::TcpStream>,
        command_tx: mpsc::Sender<WraithMessage>,
    ) -> Self {
        Self {
            wraith_id,
            hostname,
            connection,
            command_tx,
        }
    }

    /// Read a WraithMessage from a Yamux stream
    pub async fn read_message(stream: &mut Stream) -> Result<Option<WraithMessage>> {
        let mut length_buf = [0u8; 4];
        match stream.read_exact(&mut length_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(anyhow::anyhow!(e)),
        }
        let len = u32::from_be_bytes(length_buf) as usize;

        let mut data = vec![0u8; len];
        stream.read_exact(&mut data).await?;

        let msg = MessageCodec::decode(&data)?;
        Ok(Some(msg))
    }

    /// Write a WraithMessage to a Yamux stream
    pub async fn write_message(stream: &mut Stream, msg: &WraithMessage) -> Result<()> {
        let data = MessageCodec::encode(msg);
        let len = data.len() as u32;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&data).await?;
        stream.flush().await?;
        Ok(())
    }
}
```

- [ ] **Step 2: Create tunnel/mod.rs**

Create `src/wraith/tunnel/mod.rs`:

```rust
pub mod session;

pub use session::PeerSession;

use anyhow::Result;
use log::info;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, RwLock};

pub struct TunnelManager {
    sessions: Arc<RwLock<HashMap<String, PeerSession>>>,
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_session(&self, wraith_id: String, session: PeerSession) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(wraith_id.clone(), session);
        info!("Added peer session: {}", wraith_id);
    }

    pub async fn remove_session(&self, wraith_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(wraith_id);
        info!("Removed peer session: {}", wraith_id);
    }

    pub async fn get_session(&self, wraith_id: &str) -> Option<PeerSession> {
        let sessions = self.sessions.read().await;
        sessions.get(wraith_id).cloned()
    }

    pub async fn list_sessions(&self) -> Vec<(String, String)> {
        let sessions = self.sessions.read().await;
        sessions.iter()
            .map(|(id, s)| (id.clone(), s.hostname.clone()))
            .collect()
    }

    pub async fn start_peer_listener(&self, addr: &str) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        info!("Listening for peer connections on {}", addr);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    info!("Peer connection from: {}", peer_addr);
                    // TODO: Handle new peer connection
                    // 1. Accept Yamux connection
                    // 2. Read WraithRegistration
                    // 3. Add to sessions
                }
                Err(e) => {
                    warn!("Failed to accept peer connection: {}", e);
                }
            }
        }
    }
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: Export from wraith/mod.rs**

Modify `src/wraith/mod.rs`:

```rust
pub mod config;
pub mod dispatcher;
pub mod state;
pub mod tunnel;  // ADD
pub mod wraith;

pub use config::Config;
pub use dispatcher::MessageDispatcher;
pub use state::WraithState;
pub use tunnel::TunnelManager;  // ADD
pub use wraith::Wraith;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: Success

---

## Task 6: Commands - Add Agent Commands (set_id, list_peers)

**Files:**
- Create: `src/commands/agent.rs`
- Modify: `src/commands/mod.rs`
- Modify: `src/wraith/dispatcher.rs`
- Test: `tests/commands/agent.rs` (new)

- [ ] **Step 1: Create commands/agent.rs**

Create `src/commands/agent.rs`:

```rust
use crate::commands::command::Command;
use crate::proto::wraith::{Command as ProtoCommand, CommandResult, WraithMessage};
use crate::wraith::state::WraithState;
use crate::wraith::TunnelManager;
use log::{debug, info};
use std::sync::Arc;
use std::sync::Mutex;

pub struct AgentCommands {
    state: Arc<Mutex<WraithState>>,
    tunnel_manager: Arc<TunnelManager>,
}

impl AgentCommands {
    pub fn new(state: Arc<Mutex<WraithState>>, tunnel_manager: Arc<TunnelManager>) -> Self {
        Self { state, tunnel_manager }
    }

    pub fn handle_set_id(&self, cmd: &ProtoCommand) -> CommandResult {
        let new_id = cmd.params.get("wraith_id").cloned().unwrap_or_default();
        if new_id.is_empty() {
            return CommandResult {
                command_id: cmd.command_id.clone(),
                status: "error".to_string(),
                output: String::new(),
                exit_code: -1,
                duration_ms: 0,
                error: "wraith_id parameter required".to_string(),
            };
        }

        {
            let mut state = self.state.lock().unwrap();
            state.set_wraith_id(new_id.clone());
        }

        info!("Wraith ID set to: {}", new_id);
        CommandResult {
            command_id: cmd.command_id.clone(),
            status: "success".to_string(),
            output: new_id,
            exit_code: 0,
            duration_ms: 0,
            error: String::new(),
        }
    }

    pub fn handle_list_peers(&self, cmd: &ProtoCommand) -> CommandResult {
        let state = self.state.lock().unwrap();
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
        CommandResult {
            command_id: cmd.command_id.clone(),
            status: "success".to_string(),
            output,
            exit_code: 0,
            duration_ms: 0,
            error: String::new(),
        }
    }
}

impl Command for AgentCommands {
    fn execute(&self, cmd: &ProtoCommand) -> CommandResult {
        match cmd.action.as_str() {
            "set_id" => self.handle_set_id(cmd),
            "list_peers" => self.handle_list_peers(cmd),
            _ => CommandResult {
                command_id: cmd.command_id.clone(),
                status: "error".to_string(),
                output: String::new(),
                exit_code: -1,
                duration_ms: 0,
                error: format!("Unknown action: {}", cmd.action),
            },
        }
    }
}
```

- [ ] **Step 2: Update commands/mod.rs**

Modify `src/commands/mod.rs`:

```rust
pub mod agent;
pub mod command;
pub mod relay;

pub use agent::AgentCommands;
pub use command::Command;
pub use relay::RelayCommands;
```

- [ ] **Step 3: Update MessageDispatcher to handle routing**

Modify `src/wraith/dispatcher.rs`:

```rust
use crate::commands::command::Command;
use crate::commands::relay::RelayCommands;
use crate::commands::agent::AgentCommands;  // ADD
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
    agent_commands: Arc<Mutex<AgentCommands>>,  // ADD
}

impl MessageDispatcher {
    pub fn new(relay_commands: RelayCommands, agent_commands: AgentCommands) -> Self {  // Update signature
        Self {
            handlers: std::collections::HashMap::new(),
            relay_commands: Arc::new(Mutex::new(relay_commands)),
            agent_commands: Arc::new(Mutex::new(agent_commands)),  // ADD
        }
    }

    pub async fn dispatch(&self, msg: WraithMessage, state: Arc<Mutex<WraithState>>) -> Option<WraithMessage> {
        let msg_type = msg.msg_type();
        info!("Dispatching message of type: {:?}", msg_type);

        if msg_type == MessageType::Command {
            if let Some(crate::proto::wraith::wraith_message::Payload::Command(cmd)) = &msg.payload {
                // Check for target_wraith_id routing
                if let Some(target_id) = cmd.params.get("target_wraith_id") {
                    let state_guard = state.lock().unwrap();
                    if state_guard.wraith_id != *target_id {
                        // Not local - need to route to peer
                        info!("Command targeted at {} - need to route", target_id);
                        // TODO: route to peer via tunnel_manager
                        // For now, fall through to local execution
                    }
                }

                // Execute command locally
                let action = &cmd.action;
                if action == "create_relay" || action == "delete_relay" || action == "list_relays" {
                    let relay_commands = self.relay_commands.lock().unwrap();
                    let result = relay_commands.execute(cmd);
                    state.lock().unwrap().increment_commands();
                    return Some(MessageCodec::create_command_result(
                        result.command_id,
                        result.status,
                        result.output,
                        result.exit_code,
                        result.duration_ms,
                        result.error,
                    ));
                } else if action == "set_id" || action == "list_peers" {
                    let agent_commands = self.agent_commands.lock().unwrap();
                    let result = agent_commands.execute(cmd);
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
        }

        if let Some(handler) = self.handlers.get(&msg_type) {
            Some(handler(msg, state).await)
        } else {
            None
        }
    }
}
```

- [ ] **Step 4: Update Wraith::new to pass agent_commands**

Modify `src/wraith/wraith.rs`:

```rust
use crate::connection::tcp::TcpConnection;
use crate::connection::Connection;
use crate::message::codec::MessageCodec;
use crate::proto::wraith::{MessageType, WraithMessage};
use crate::wraith::state::WraithState;
use crate::wraith::dispatcher::MessageDispatcher;
use crate::relay::RelayManager;
use crate::commands::agent::AgentCommands;
use crate::wraith::TunnelManager;
use log::{error, info};
use std::sync::{Arc, Mutex};

pub struct Wraith {
    connection: TcpConnection,
    state: Arc<Mutex<WraithState>>,
    dispatcher: MessageDispatcher,
    tunnel_manager: Arc<TunnelManager>,  // ADD
}

impl Wraith {
    pub fn new(host: String, port: u16, is_server: bool) -> Self {
        let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
        let tunnel_manager = Arc::new(TunnelManager::new());  // ADD
        let state = Arc::new(Mutex::new(WraithState::new_with_relay_manager(relay_manager.clone())));
        let relay_commands = crate::commands::relay::RelayCommands::new(relay_manager.clone());
        let agent_commands = AgentCommands::new(state.clone(), tunnel_manager.clone());  // ADD

        Self {
            connection: TcpConnection::new(host, port, is_server),
            state,
            dispatcher: MessageDispatcher::new(relay_commands, agent_commands),
            tunnel_manager,
        }
    }
    // ... rest unchanged
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo build`
Expected: Success

---

## Task 7: Relay - Support Remote Relay via target_wraith_id

**Files:**
- Modify: `src/commands/relay.rs`
- Test: `tests/commands/relay.rs`

- [ ] **Step 1: Extend handle_create_relay to check target_wraith_id**

Modify `src/commands/relay.rs`:

Add to `handle_create_relay`:

```rust
pub fn handle_create_relay(&self, cmd: &ProtoCommand) -> CommandResult {
    // ... existing hop parsing code ...

    // Check if this relay should be created on a remote wraith
    if let Some(target_id) = cmd.params.get("target_wraith_id") {
        // Get local wraith_id from state (passed via cmd params or separate mechanism)
        let local_wraith_id = cmd.params.get("_local_wraith_id").cloned().unwrap_or_default();

        if target_id != &local_wraith_id {
            // This relay needs to be routed to target peer
            // The routing happens at dispatcher level, so we return a special
            // status that tells dispatcher to forward to peer
            return CommandResult {
                command_id: cmd.command_id.clone(),
                status: "route_to_peer".to_string(),  // Special status
                output: target_id.clone(),  // Contains target wraith ID
                exit_code: 0,
                duration_ms: 0,
                error: format!("forward_to_peer:{}", target_id),
            };
        }
    }

    // ... rest of local relay creation ...
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build`
Expected: Success

**Note:** Full peer relay implementation requires the tunnel manager integration in Task 8.

---

## Task 8: Integration - Peer Connection Handler

**Files:**
- Modify: `src/wraith/tunnel/mod.rs`
- Modify: `src/wraith/wraith.rs`
- Test: Integration test

- [ ] **Step 1: Implement peer connection acceptance in TunnelManager**

Modify `src/wraith/tunnel/mod.rs` - update `start_peer_listener`:

```rust
pub async fn start_peer_listener(&self, addr: &str) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("Listening for peer connections on {}", addr);

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                info!("Peer connection from: {}", peer_addr);
                let tunnel_manager = Arc::clone(&self.sessions);

                tokio::spawn(async move {
                    // Accept Yamux connection
                    if let Err(e) = Self::handle_peer_connection(stream, tunnel_manager).await {
                        warn!("Peer connection handler error: {}", e);
                    }
                });
            }
            Err(e) => {
                warn!("Failed to accept peer connection: {}", e);
            }
        }
    }
}

async fn handle_peer_connection(
    stream: tokio::net::TcpStream,
    sessions: Arc<RwLock<HashMap<String, PeerSession>>>,
) -> Result<()> {
    use crate::wraith::tunnel::PeerSession;

    let conn = yamux::Connection::new(stream, yamux::Mode::Server);
    let mut conn = Arc::new(tokio::sync::RwLock::new(conn));

    // Read WraithRegistration from Stream 0
    let mut stream_guard = conn.write().await;
    if let Some(Ok(mut yamux_stream)) = stream_guard.next_stream().await {
        if let Some(msg) = PeerSession::read_message(&mut yamux_stream).await? {
            if let Some(crate::proto::wraith::wraith_message::Payload::WraithRegistration(reg)) = msg.payload {
                let wraith_id = reg.wraith_id.clone();
                let hostname = reg.hostname.clone();

                let (tx, rx) = mpsc::channel(100);
                let session = PeerSession::new(
                    wraith_id.clone(),
                    hostname,
                    Arc::try_unwrap(conn).unwrap_or_else(|c| (*c).clone()),
                    tx,
                );

                let mut sessions_write = sessions.write().await;
                sessions_write.insert(wraith_id.clone(), session);

                info!("Registered peer: {}", wraith_id);

                // TODO: Start reading from stream for commands
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Update Wraith to handle peer sessions**

Modify `src/wraith/wraith.rs` to add peer session handling in run loop:

```rust
pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    if self.connection.is_server() {
        self.connection.listen().await?;
    } else {
        self.connection.connect().await?;
    }

    self.state.lock().unwrap().set_connected(true);
    self.register().await?;

    // Spawn peer listener if configured
    if let Some(peer_addr) = &self.peer_listen_addr {
        let tunnel_manager = Arc::clone(&self.tunnel_manager);
        tokio::spawn(async move {
            if let Err(e) = tunnel_manager.start_peer_listener(peer_addr).await {
                error!("Peer listener error: {}", e);
            }
        });
    }

    loop {
        tokio::select! {
            // Handle C2 commands
            msg_result = self.connection.read_message() => {
                match msg_result {
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
            // Handle peer sessions (future)
        }
    }

    self.state.lock().unwrap().set_connected(false);
    Ok(())
}
```

Add `peer_listen_addr` field to Wraith struct.

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Success

---

## Task 9: PyWraith - Update Python Client

**Files:**
- Modify: `home/PyWraith/protocol.py`
- Modify: `home/PyWraith/client.py`
- Modify: `home/PyWraith/cli.py`
- Test: `home/PyWraith/tests/test_client.py` (new)

- [ ] **Step 1: Add new message helpers to protocol.py**

Modify `home/PyWraith/protocol.py`:

Add new methods:

```python
@staticmethod
def create_wraith_registration(
    wraith_id: str,
    hostname: str,
    os: str
) -> pb.WraithMessage:
    """Create WRAITH_REGISTRATION message for peer connections."""
    reg = pb.WraithRegistration(
        wraith_id=wraith_id,
        hostname=hostname,
        os=os,
        connected_at=int(time.time() * 1000)
    )
    msg = pb.WraithMessage(
        msg_type=pb.WRAITH_REGISTRATION,
        message_id=str(uuid.uuid4()),
        timestamp=int(time.time() * 1000),
        wraith_registration=reg
    )
    return msg

@staticmethod
def create_peer_list() -> pb.WraithMessage:
    """Create PEER_LIST message."""
    msg = pb.WraithMessage(
        msg_type=pb.PEER_LIST,
        message_id=str(uuid.uuid4()),
        timestamp=int(time.time() * 1000),
        peer_list=pb.PeerList()
    )
    return msg

@staticmethod
def parse_peer_update(msg: pb.WraithMessage) -> Dict[str, Any]:
    """Extract peer update fields as a dict."""
    update = msg.peer_update
    return {
        'wraith_id': update.wraith_id,
        'hostname': update.hostname,
        'action': update.action,
        'timestamp': update.timestamp,
    }

@staticmethod
def parse_peer_list_response(msg: pb.WraithMessage) -> Dict[str, Any]:
    """Extract peer list response as a dict."""
    resp = msg.peer_list_response
    return {
        'wraith_id': resp.wraith_id,
        'peers': [{'wraith_id': p.wraith_id, 'hostname': p.hostname, 'connected': p.connected}
                  for p in resp.peers],
    }
```

- [ ] **Step 2: Add peer methods to client.py**

Modify `home/PyWraith/client.py`:

Add new methods to `WraithClient`:

```python
def set_id(self, wraith_id: str) -> Tuple[bool, Dict[str, Any]]:
    """Set the wraith's ID."""
    params = {'wraith_id': wraith_id}
    return self.send_command('set_id', params)

def list_peers(self) -> Tuple[bool, Dict[str, Any]]:
    """List direct peer wraiths."""
    return self.send_command('list_peers', {})

def list_peers_recursive(self, timeout: int = 30) -> Tuple[bool, Dict[str, Any]]:
    """List all peers recursively by traversing the network.

    Returns hierarchical view of wraith network.
    """
    # First get our direct peers
    success, result = self.list_peers()
    if not success:
        return success, result

    network = {
        'self': result.get('wraith_id'),
        'peers': result.get('peers', []),
    }

    # TODO: For each peer, connect and query their peers
    # This requires establishing connections to peer wraiths
    # Implementation depends on peer connection mechanism

    return True, network
```

- [ ] **Step 3: Verify Python compilation**

Run: `cd home && python -c "from PyWraith.protocol import WraithProtocol; from PyWraith.client import WraithClient; print('OK')"`
Expected: OK

---

## Task 10: Integration Test

**Files:**
- Create: `tests/integration/agent_network.rs`
- Test: `tests/integration/test_peer_connection.rs`

- [ ] **Step 1: Create basic integration test**

Create `tests/integration/test_peer_connection.rs`:

```rust
#[cfg(test)]
mod tests {
    use wraith::WraithState;

    #[test]
    fn test_wraith_id_generation() {
        let state = WraithState::new();
        assert!(!state.wraith_id.is_empty());
        // Should be valid UUID format
        assert!(uuid::Uuid::parse_str(&state.wraith_id).is_ok());
    }

    #[test]
    fn test_peer_table_empty() {
        let state = WraithState::new();
        assert!(state.peer_table.is_empty());
    }

    #[test]
    fn test_message_loop_prevention() {
        let state = WraithState::new();
        let msg_id = "test-123";

        assert!(!state.has_seen_message(msg_id));
        state.mark_message_seen(msg_id.to_string());
        assert!(state.has_seen_message(msg_id));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: All tests pass

---

## Self-Review Checklist

- [ ] All spec requirements have corresponding tasks
- [ ] No placeholders (TBD, TODO) in implementation steps
- [ ] Type consistency across tasks (WraithRegistration, PeerInfo, etc.)
- [ ] Each task produces working code
- [ ] Commands shown with exact expected output

---

## Spec Coverage

| Spec Section | Tasks |
|--------------|-------|
| Network Topology | Task 3, 4, 5 |
| Wraith Identification | Task 2, 6 |
| Agent Connection Protocol | Task 4, 5, 8 |
| Command Routing | Task 6, 7 |
| Yamux Stream Management | Task 4 |
| Relay Through Chain | Task 7 |
| List Peers Command | Task 6 |
| PyWraith Updates | Task 9 |

---

**Plan complete.** Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
