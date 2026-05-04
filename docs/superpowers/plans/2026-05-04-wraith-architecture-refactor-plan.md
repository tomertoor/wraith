# Wraith Architecture Refactoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Simplify Wraith codebase by eliminating 240+ lines of duplicated code, fixing critical bugs, and consolidating state management.

**Architecture:** Consolidate all routing through TunnelManager, use WraithState as single peer table source of truth, extract shared functions to eliminate duplication.

**Tech Stack:** Rust, tokio async, yamux, prost (protobuf)

---

## File Map

| File | Responsibility |
|------|----------------|
| `src/wraith/wraith.rs` | Core Wraith struct, main loops, C2 handling |
| `src/wraith/state.rs` | WraithState struct with peer_table |
| `src/wraith/tunnel/mod.rs` | TunnelManager, peer routing, Yamux session management |
| `src/wraith/tunnel/session.rs` | PeerSession wrapper for Yamux streams |
| `src/connection/tcp.rs` | TCP connection framing |
| `src/message/codec.rs` | Message encoding/decoding |
| `src/commands/relay.rs` | Relay command handlers |
| `src/commands/agent.rs` | Agent command handlers |
| `src/commands/command.rs` | Command trait (to be removed) |
| `src/relay/mod.rs` | Relay management |
| `src/wraith/dispatcher.rs` | MessageDispatcher (to be deleted) |
| `src/connection/connection.rs` | Connection trait (to be deleted) |
| `src/connection/yamux.rs` | YamuxConnection struct (to be deleted) |

---

## PHASE 1: Fix Critical Bugs

### Task 1: Fix await-while-holding-lock in wraith.rs::register()

**Files:**
- Modify: `src/wraith/wraith.rs:304-319`

- [ ] **Step 1: Read current register() method**

Run: `cat -n src/wraith/wraith.rs | sed -n '304,320p'`
Expected: Shows current code with lock held across await

- [ ] **Step 2: Fix register() to drop lock before await**

```rust
async fn register(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    let (hostname, username, os, ip_address) = {
        let state = self.state.lock().unwrap();
        (state.hostname.clone(), state.username.clone(), state.os.clone(), state.ip_address.clone())
    };
    // Lock dropped here - safe to await

    let msg = MessageCodec::create_registration(hostname, username, os, ip_address);
    if let Some(conn) = &mut self.connection {
        conn.send_message(&msg).await?;
        info!("Registration sent");
    }
    Ok(())
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: No errors related to register()

- [ ] **Step 4: Commit**

```bash
git add src/wraith/wraith.rs
git commit -m "fix(wraith): drop lock before await in register()

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 2: Fix TunnelManager double-creation in Wraith::new()

**Files:**
- Modify: `src/wraith/wraith.rs:48-90`

- [ ] **Step 1: Read current new() method**

Run: `cat -n src/wraith/wraith.rs | sed -n '48,90p'`
Expected: Shows double TunnelManager creation

- [ ] **Step 2: Rewrite new() to create single TunnelManager**

Replace the entire `new()` method with:

```rust
pub fn new(wraith_id: &String) -> Self {
    let relay_manager = Arc::new(Mutex::new(RelayManager::new()));
    let state = Arc::new(Mutex::new(WraithState::new_with_relay_manager(
        wraith_id.clone(),
        Arc::clone(&relay_manager),
    )));

    let tunnel_manager = Arc::new(TunnelManager::new());
    tunnel_manager.set_state(Arc::clone(&state));

    let relay_commands = RelayCommands::new(Arc::clone(&relay_manager), Arc::clone(&tunnel_manager));
    let agent_commands = AgentCommands::new(Arc::clone(&tunnel_manager));

    tunnel_manager.set_commands(relay_commands, agent_commands);

    tunnel_manager.set_peer_add_callback(move |wraith_id, hostname, sender| {
        let mut s = state.lock().unwrap();
        s.add_peer(wraith_id.to_string(), hostname.to_string(), sender.clone());
    });

    Self {
        connection: None,
        state,
        dispatcher: MessageDispatcher::new(
            RelayCommands::new(relay_manager, Arc::clone(&tunnel_manager)),
            AgentCommands::new(Arc::clone(&tunnel_manager)),
        ),
        tunnel_manager,
        agent_mode: false,
        peer_listen_addr: None,
        peer_connect_addr: None,
    }
}
```

- [ ] **Step 3: Add set_commands() to TunnelManager**

Modify `src/wraith/tunnel/mod.rs` - add method to TunnelManager impl:

```rust
pub fn set_commands(&self, relay_commands: RelayCommands, agent_commands: AgentCommands) {
    *self.relay_commands.lock().unwrap() = relay_commands;
    *self.agent_commands.lock().unwrap() = agent_commands;
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -50`
Expected: No errors

- [ ] **Step 5: Commit**

```bash
git add src/wraith/wraith.rs src/wraith/tunnel/mod.rs
git commit -m "fix(wraith): single TunnelManager creation in new()

Previously created two TunnelManagers, first was discarded with its
callback registration. Now creates single TunnelManager.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## PHASE 2: Create Shared Functions

### Task 3: Add read_framed_message() to codec.rs

**Files:**
- Modify: `src/message/codec.rs`

- [ ] **Step 1: Read current codec.rs**

Run: `cat src/message/codec.rs`
Expected: Shows existing codec methods

- [ ] **Step 2: Add read_framed_message function**

Add this function to the MessageCodec impl block:

```rust
use std::io::{Error, ErrorKind};
use tokio::io::{AsyncRead, AsyncReadExt};

impl MessageCodec {
    /// Read a length-prefixed WraithMessage from a stream.
    /// Format: [4 bytes: length as big-endian u32][N bytes: protobuf]
    pub async fn read_framed_message<R>(stream: &mut R) -> Result<WraithMessage>
    where
        R: AsyncRead + Unpin,
    {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;

        let len = u32::from_be_bytes(len_buf) as usize;

        if len > 10 * 1024 * 1024 {
            return Err(Error::new(ErrorKind::InvalidData, "message too large").into());
        }

        let mut data = vec![0u8; len];
        stream.read_exact(&mut data).await?;

        WraithMessage::decode(data.as_slice())
            .map_err(|e| Error::new(ErrorKind::InvalidData, e).into())
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add src/message/codec.rs
git commit -m "feat(codec): add read_framed_message() for unified framing

Extracts length-prefix message reading from TcpConnection and
PeerSession into single shared function.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 4: Add check_and_mark_seen() to state.rs

**Files:**
- Modify: `src/wraith/state.rs`

- [ ] **Step 1: Read current state.rs**

Run: `cat src/wraith/state.rs`
Expected: Shows WraithState struct

- [ ] **Step 2: Add check_and_mark_seen() method to WraithState**

Add this method to the WraithState impl block:

```rust
use tokio::sync::oneshot;

/// Result of checking a message for deduplication.
pub struct DedupResult {
    pub already_seen: bool,
    pub pending_tx: Option<oneshot::Sender<crate::proto::wraith::WraithMessage>>,
}

/// Check if a message has been seen, and if so return the pending response sender.
/// Marks the message as seen if this is the first time.
pub fn check_and_mark_seen(&self, msg_id: &str) -> DedupResult {
    let pending_tx = self
        .pending_responses
        .lock()
        .unwrap()
        .remove(msg_id);

    let already_seen = if self.seen_message_ids.lock().unwrap().contains(msg_id) {
        true
    } else {
        self.seen_message_ids.lock().unwrap().insert(msg_id.to_string());
        false
    };

    DedupResult {
        already_seen,
        pending_tx,
    }
}
```

- [ ] **Step 3: Update WraithState to use tokio sync primitives for async context**

Change the mutex types in WraithState:

```rust
pub struct WraithState {
    pub relay_manager: Arc<Mutex<RelayManager>>,
    pub wraith_id: String,
    pub peer_table: HashMap<String, PeerConnection>,
    pub seen_message_ids: std::sync::Mutex<HashSet<String>>,  // Keep std for now
    pub pending_responses: std::sync::Mutex<HashMap<String, oneshot::Sender<crate::proto::wraith::WraithMessage>>>,
    pub hostname: String,
    pub username: String,
    pub os: String,
    pub ip_address: String,
    pub commands_executed: i64,
    pub last_command_time: i64,
    pub connected: bool,
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: No errors

- [ ] **Step 5: Commit**

```bash
git add src/wraith/state.rs
git commit -m "feat(state): add check_and_mark_seen() for message deduplication

Extracts dedup checking logic from tunnel/mod.rs into WraithState
method. Returns both already_seen flag and pending response sender.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 5: Add spawn_yamux_driver() to tunnel/mod.rs

**Files:**
- Modify: `src/wraith/tunnel/mod.rs`

- [ ] **Step 1: Read current tunnel/mod.rs imports and structure**

Run: `head -40 src/wraith/tunnel/mod.rs`
Expected: Shows imports and TunnelManager struct

- [ ] **Step 2: Add spawn_yamux_driver() function**

Add this standalone function before the TunnelManager impl:

```rust
use futures::future::poll_fn;
use std::pin::Pin;

/// Spawn a background task to drive a yamux Connection.
/// Returns a handle (Arc<Mutex<Connection>>) for opening streams.
pub fn spawn_yamux_driver(
    conn: yamux::Connection<Compat<TcpStream>>,
) -> Arc<tokio::sync::Mutex<yamux::Connection<Compat<TcpStream>>>> {
    let conn_handle = Arc::new(tokio::sync::Mutex::new(conn));
    let conn_handle_for_spawn = Arc::clone(&conn_handle);

    tokio::spawn(async move {
        let mut c = conn_handle_for_spawn.lock().await;
        loop {
            match poll_fn(|cx| Pin::new(&mut c).poll_next_inbound(cx)).await {
                Some(Ok(_)) => { /* stream handled elsewhere */ }
                Some(Err(e)) => {
                    log::warn!("Yamux connection error: {}", e);
                    break;
                }
                None => {
                    log::info!("Yamux connection closed");
                    break;
                }
            }
        }
        log::info!("Yamux driver finished");
    });

    conn_handle
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: No errors related to spawn_yamux_driver

- [ ] **Step 4: Commit**

```bash
git add src/wraith/tunnel/mod.rs
git commit -m "feat(tunnel): add spawn_yamux_driver() helper

Extracts yamux Connection driver spawning into shared function.
Used by both handle_peer_connection and connect_to_peer.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 6: Add dispatch_command() to tunnel/mod.rs

**Files:**
- Modify: `src/wraith/tunnel/mod.rs`

- [ ] **Step 1: Add dispatch_command() method to TunnelManager**

Add this method to TunnelManager impl:

```rust
use crate::message::codec::MessageCodec;
use crate::proto::wraith::MessageType;

/// Dispatch a Command message to the appropriate handler.
/// Returns CommandResult wrapped in WraithMessage.
pub fn dispatch_command(
    cmd: &crate::proto::wraith::Command,
    relay_commands: &Arc<Mutex<RelayCommands>>,
    agent_commands: &Arc<Mutex<AgentCommands>>,
    state: &Arc<Mutex<WraithState>>,
) -> Option<crate::proto::wraith::WraithMessage> {
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

    Some(MessageCodec::create_command_result(
        result.command_id,
        result.status,
        result.output,
        result.exit_code,
        result.duration_ms,
        result.error,
    ))
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add src/wraith/tunnel/mod.rs
git commit -m "feat(tunnel): add dispatch_command() helper

Extracts command dispatch match logic into shared function.
Eliminates duplication across wraith.rs and tunnel/mod.rs.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 7: Add add_peer_to_state() to state.rs

**Files:**
- Modify: `src/wraith/state.rs`

- [ ] **Step 1: Add add_peer_to_state() method to WraithState**

Add this method to the WraithState impl block:

```rust
use tokio::sync::mpsc;

/// Add a peer connection to the peer table.
pub fn add_peer(
    &mut self,
    wraith_id: String,
    hostname: String,
    sender: mpsc::Sender<crate::proto::wraith::WraithMessage>,
) {
    let connected_at = chrono::Utc::now().timestamp_millis();
    self.peer_table.insert(
        wraith_id.clone(),
        PeerConnection {
            wraith_id,
            hostname,
            connected_at,
            sender,
        },
    );
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add src/wraith/state.rs
git commit -m "feat(state): add add_peer_to_state() helper

Moves peer addition logic from wraith.rs callbacks into WraithState
method for clearer ownership.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## PHASE 3: Eliminate Duplication

### Task 8: Replace TcpConnection::read_message with codec version

**Files:**
- Modify: `src/connection/tcp.rs`

- [ ] **Step 1: Read current TcpConnection methods**

Run: `cat -n src/connection/tcp.rs | sed -n '40,75p'`
Expected: Shows existing send_message and read_message

- [ ] **Step 2: Simplify TcpConnection using codec**

Replace the read_message method body with a call to the codec:

```rust
use crate::message::codec::MessageCodec;

impl TcpConnection {
    pub async fn read_message(&mut self) -> Result<WraithMessage> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            Error::new(ErrorKind::NotConnected, "not connected")
        })?;

        MessageCodec::read_framed_message(stream).await
    }
}
```

- [ ] **Step 3: Keep send_message but use FramedWriter directly**

```rust
impl TcpConnection {
    pub async fn send_message(&mut self, msg: &WraithMessage) -> Result<()> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            Error::new(ErrorKind::NotConnected, "not connected")
        })?;

        let data = msg.encode_to_vec();
        let framed = crate::connection::framing::FramedWriter::write_frame(&data);

        stream.write_all(&framed).await?;
        Ok(())
    }
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: No errors

- [ ] **Step 5: Commit**

```bash
git add src/connection/tcp.rs
git commit -m "refactor(tcp): use MessageCodec::read_framed_message()

Eliminates duplicate length-prefix framing code in TcpConnection.
Now uses shared codec function.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 9: Replace PeerSession::read_message with codec version

**Files:**
- Modify: `src/wraith/tunnel/session.rs`

- [ ] **Step 1: Read current PeerSession**

Run: `cat src/wraith/tunnel/session.rs`
Expected: Shows PeerSession struct with read_message

- [ ] **Step 2: Update PeerSession::read_message to use codec**

Replace the read_message implementation:

```rust
use crate::message::codec::MessageCodec;
use anyhow::Result;
use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncReadExt};

impl PeerSession {
    /// Read a WraithMessage from a Yamux stream.
    pub async fn read_message<R>(stream: &mut R) -> Result<Option<WraithMessage>>
    where
        R: AsyncRead + Unpin,
    {
        let mut length_buf = [0u8; 4];

        match stream.read_exact(&mut length_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None);
            }
            Err(e) => return Err(anyhow::anyhow!("read error: {}", e)),
        }

        let len = u32::from_be_bytes(length_buf) as usize;

        if len > 10 * 1024 * 1024 {
            return Err(anyhow::anyhow!("message too large"));
        }

        let mut data = vec![0u8; len];
        stream.read_exact(&mut data).await?;

        let msg = MessageCodec::decode(&data)?;
        Ok(Some(msg))
    }

    /// Write a WraithMessage to a Yamux stream.
    pub async fn write_message<W>(writer: &mut W, msg: &WraithMessage) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        use tokio::io::AsyncWriteExt;

        let data = MessageCodec::encode(msg);
        let len = data.len() as u32;

        writer.write_all(&len.to_be_bytes()).await?;
        writer.write_all(&data).await?;
        writer.flush().await?;

        Ok(())
    }
}
```

- [ ] **Step 3: Remove unused imports**

Update imports at top of file:

```rust
use crate::message::codec::MessageCodec;
use crate::proto::wraith::WraithMessage;
use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use yamux::{Config, Connection, Mode};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::io::{AsyncRead, AsyncReadExt};
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: No errors

- [ ] **Step 5: Commit**

```bash
git add src/wraith/tunnel/session.rs
git commit -m "refactor(session): use MessageCodec for read_message

Consolidates framing logic. PeerSession now uses shared codec.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 10: Update tunnel/mod.rs to use shared functions

**Files:**
- Modify: `src/wraith/tunnel/mod.rs`

- [ ] **Step 1: Update handle_peer_connection to use spawn_yamux_driver**

Replace the yamux connection setup section (lines ~181-196) with:

```rust
let conn = yamux::Connection::new(stream.compat(), yamux::Config::default(), yamux::Mode::Server);
let conn_handle = spawn_yamux_driver(conn);
```

- [ ] **Step 2: Update connect_to_peer to use spawn_yamux_driver**

Replace similar section (lines ~466-485) with same pattern.

- [ ] **Step 3: Update dedup checking to use check_and_mark_seen**

Replace the dedup block (lines ~288-312) with:

```rust
let dedup = {
    let state_guard = state.lock().unwrap();
    if let Some(ref s) = *state_guard {
        s.check_and_mark_seen(&msg_id)
    } else {
        return DedupResult { already_seen: false, pending_tx: None };
    }
};

if dedup.already_seen {
    info!("Skipping duplicate message: {}", msg_id);
    continue;
}

if let Some(tx) = dedup.pending_tx {
    if tx.send(msg).is_err() {
        info!("Failed to send response for {}", msg_id);
    }
    continue;
}
```

- [ ] **Step 4: Update command dispatch to use dispatch_command**

Replace the dispatch block (lines ~419-435) with call to shared function.

- [ ] **Step 5: Verify compilation**

Run: `cargo build 2>&1 | head -50`
Expected: No errors

- [ ] **Step 6: Commit**

```bash
git add src/wraith/tunnel/mod.rs
git commit -m "refactor(tunnel): use shared functions throughout

Replaces duplicate code with calls to spawn_yamux_driver,
check_and_mark_seen, and dispatch_command.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 11: Update wraith.rs to use shared functions

**Files:**
- Modify: `src/wraith/wraith.rs`

- [ ] **Step 1: Update Wraith::run() to use shared dispatch**

Replace the message handling loop (lines ~128-148) to use TunnelManager::dispatch_command.

- [ ] **Step 2: Update handle_c2_connection to use shared functions**

Replace duplicate message loop (lines ~210-229) with call to shared dispatcher.

- [ ] **Step 3: Remove dispatch_command method from wraith.rs**

This method is now duplicated in tunnel/mod.rs.

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -50`
Expected: No errors

- [ ] **Step 5: Commit**

```bash
git add src/wraith/wraith.rs
git commit -m "refactor(wraith): use shared dispatch_command

Eliminates duplicate command dispatch logic.
All routing now goes through TunnelManager.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## PHASE 4: Remove Dead Code

### Task 12: Delete dispatcher.rs

**Files:**
- Delete: `src/wraith/dispatcher.rs`

- [ ] **Step 1: Verify dispatcher.rs is no longer used**

Run: `grep -r "dispatcher" src/ --include="*.rs" | grep -v "dispatcher.rs"`
Expected: No references to MessageDispatcher

- [ ] **Step 2: Remove dispatcher from wraith.rs**

Remove dispatcher field and MessageDispatcher::new() call from wraith.rs.

- [ ] **Step 3: Delete file**

```bash
git rm src/wraith/dispatcher.rs
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: No errors related to dispatcher

- [ ] **Step 5: Commit**

```bash
git commit -m "refactor: remove MessageDispatcher

All routing now goes through TunnelManager.
MessageDispatcher was duplicating TunnelManager::route_message.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 13: Delete connection/connection.rs

**Files:**
- Delete: `src/connection/connection.rs`

- [ ] **Step 1: Verify connection trait is not used**

Run: `grep -r "impl.*Connection" src/ --include="*.rs"`
Expected: No results

- [ ] **Step 2: Delete file**

```bash
git rm src/connection/connection.rs
```

- [ ] **Step 3: Update connection/mod.rs**

Remove the `pub mod connection;` line from mod.rs

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: No errors related to Connection trait

- [ ] **Step 5: Commit**

```bash
git commit -m "refactor: remove unused Connection trait

Trait was defined but never used polymorphically.
TcpConnection is used directly.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 14: Delete connection/yamux.rs

**Files:**
- Delete: `src/connection/yamux.rs`

- [ ] **Step 1: Verify YamuxConnection is not used**

Run: `grep -r "YamuxConnection" src/ --include="*.rs"`
Expected: No results

- [ ] **Step 2: Delete file**

```bash
git rm src/connection/yamux.rs
```

- [ ] **Step 3: Update connection/mod.rs**

Remove the `pub mod yamux;` line from mod.rs

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: No errors related to YamuxConnection

- [ ] **Step 5: Commit**

```bash
git commit -m "refactor: remove unused YamuxConnection struct

PeerSession handles Yamux directly.
YamuxConnection was dead code.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 15: Remove unused codec methods

**Files:**
- Modify: `src/message/codec.rs`

- [ ] **Step 1: Identify unused codec methods**

Run: `grep -r "create_relay_create\|create_relay_delete\|create_relay_list" src/ --include="*.rs"`
Expected: Only definitions, no usages

- [ ] **Step 2: Remove unused methods from codec.rs**

Delete these methods from MessageCodec:
- `create_relay_create()`
- `create_relay_delete()`
- `create_relay_list()`
- `create_relay_list_response()`

Keep:
- `create_registration()`
- `create_heartbeat()`
- `create_command()`
- `create_command_result()`
- `create_message()`

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git commit -m "refactor: remove unused relay codec methods

create_relay_create/delete/list/response were never dispatched.
These payload types are defined in protobuf but not handled.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## PHASE 5: Cleanup

### Task 16: Remove Command trait

**Files:**
- Modify: `src/commands/command.rs`, `src/commands/relay.rs`, `src/commands/agent.rs`

- [ ] **Step 1: Check Command trait usage**

Run: `grep -r "impl Command\|execute(" src/commands/ --include="*.rs"`
Expected: Shows relay.rs and agent.rs implementations

- [ ] **Step 2: Simplify RelayCommands**

Remove the `impl Command for RelayCommands` block. Keep the struct and methods as plain methods.

- [ ] **Step 3: Simplify AgentCommands**

Remove the `impl Command for AgentCommands` block.

- [ ] **Step 4: Delete command.rs**

```bash
git rm src/commands/command.rs
```

- [ ] **Step 5: Update mod.rs**

Remove `pub mod command;` from commands/mod.rs

- [ ] **Step 6: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: No errors related to Command trait

- [ ] **Step 7: Commit**

```bash
git commit -m "refactor: remove Command trait

Trait was forcing sync execute() into async contexts.
RelayCommands and AgentCommands now use plain methods.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 17: Simplify TunnelManager struct

**Files:**
- Modify: `src/wraith/tunnel/mod.rs`

- [ ] **Step 1: Read current TunnelManager**

Run: `cat -n src/wraith/tunnel/mod.rs | sed -n '26,65p'`
Expected: Shows struct with sessions field

- [ ] **Step 2: Remove sessions field and callbacks**

Simplify TunnelManager to:
```rust
pub struct TunnelManager {
    relay_commands: Arc<Mutex<RelayCommands>>,
    agent_commands: Arc<Mutex<AgentCommands>>,
    state: Arc<Mutex<WraithState>>,
}
```

- [ ] **Step 3: Update methods to use state directly**

Replace `self.sessions` with direct state access via `self.state.lock().unwrap().peer_table`

- [ ] **Step 4: Remove callback fields and methods**

Remove:
- `peer_add_callback`
- `peer_remove_callback`
- `set_peer_add_callback()`
- `set_peer_remove_callback()`
- `notify_peer_added()`
- `notify_peer_removed()`

- [ ] **Step 5: Verify compilation**

Run: `cargo build 2>&1 | head -50`
Expected: No errors

- [ ] **Step 6: Commit**

```bash
git commit -m "refactor: simplify TunnelManager struct

Remove sessions HashMap - uses WraithState::peer_table directly.
Remove callback fields - no longer needed with single state.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 18: Fix all clippy warnings

**Files:**
- Modify: Various files as needed

- [ ] **Step 1: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1 | head -80`
Expected: List of warnings

- [ ] **Step 2: Fix unused imports**

For each unused import, remove from the use statement.

- [ ] **Step 3: Fix unused variables**

Prefix unused variables with `_` or remove if truly unused.

- [ ] **Step 4: Fix await_holding_lock if any remain**

Ensure no MutexGuard is held across await points.

- [ ] **Step 5: Repeat until clean**

Run: `cargo clippy -- -D warnings 2>&1`
Expected: No warnings or errors

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "fix: resolve all clippy warnings

- Remove unused imports
- Fix unused variables  
- Ensure no locks held across await

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 19: Final verification

- [ ] **Step 1: Run tests**

Run: `cargo test 2>&1`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`
Expected: No warnings or errors

- [ ] **Step 3: Verify no dispatcher.rs**

Run: `ls src/wraith/dispatcher.rs 2>&1`
Expected: No such file

- [ ] **Step 4: Verify no duplicate peer tables**

Run: `grep -r "peer_table\|sessions" src/wraith/ --include="*.rs" | grep -v "_table:" | grep -v "//"`
Expected: Only peer_table in state.rs

- [ ] **Step 5: Final commit**

```bash
git commit -m "chore: complete architecture refactoring

- 240+ lines of duplicated code eliminated
- Critical bugs fixed (await-while-lock, double-creation)
- Single routing through TunnelManager
- Single peer table in WraithState
- All clippy warnings resolved

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Success Criteria Checklist

- [ ] `cargo build` passes
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` passes with zero warnings
- [ ] `src/wraith/dispatcher.rs` deleted
- [ ] `src/connection/connection.rs` deleted
- [ ] `src/connection/yamux.rs` deleted
- [ ] No `sessions: Arc<RwLock<HashMap<...>>>` in TunnelManager
- [ ] No `MessageDispatcher` references
- [ ] No duplicate message reading loops (all use shared codec)
- [ ] No duplicate dedup checking (all use check_and_mark_seen)
- [ ] No duplicate command dispatch (all use dispatch_command)

---

*End of Plan*
