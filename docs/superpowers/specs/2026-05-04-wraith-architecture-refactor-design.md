# Wraith Architecture Refactoring Design

**Date:** 2026-05-04
**Status:** Draft
**Goal:** Simplify codebase, remove duplication, fix bugs, make it more maintainable

---

## 1. Executive Summary

The Wraith codebase has accumulated **240+ lines of duplicated code** across 8 major patterns. This design proposes a refactoring to consolidate all shared logic into single, well-defined locations.

### Key Problems
1. **State duplication** — Two peer tables kept in sync via fragile callbacks
2. **Routing duplication** — `MessageDispatcher` and `TunnelManager` both route messages
3. **Code duplication** — 8 distinct patterns repeated 2-4 times each
4. **Critical bugs** — `await` while holding `MutexGuard`, `TunnelManager` double-creation

---

## 2. Code Duplication (240+ Lines)

### 2.1 Message Reading Loop (~60 lines, 4 places)

**Locations:** `wraith/wraith.rs:128-148`, `wraith/wraith.rs:210-229`, `wraith/tunnel/mod.rs:270-335`, `wraith/tunnel/mod.rs:559-626`

```rust
loop {
    match connection.read_message().await {
        Ok(msg) => {
            let msg_type = msg.msg_type;
            match msg_type {
                x if x == MessageType::Command as i32 => {
                    if let Some(response) = tunnel_manager.route_message(msg, ...).await {
                        connection.send_message(&response).await?;
                    }
                }
                _ => info!("Received message type: {}", msg_type),
            }
        }
        Err(e) => { error!("Read failed: {}", e); break; }
    }
}
```

**Extract:** `read_message_loop(connection, on_message, on_error)` async function

---

### 2.2 Length-Prefix Framing (~30 lines, 3 places)

**Locations:** `connection/tcp.rs:53-72`, `connection/tcp.rs:135-153` (duplicate!), `tunnel/session.rs:49-72`

```rust
let mut len_buf = [0u8; 4];
stream.read_exact(&mut len_buf).await?;
let len = u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as usize;
if len > 10 * 1024 * 1024 {
    return Err(Error::new(ErrorKind::InvalidData, "message too large"));
}
let mut data = vec![0u8; len];
stream.read_exact(&mut data).await?;
let msg = WraithMessage::decode(data.as_slice())?;
```

**Extract:** `read_framed_message<R>(stream: &mut R) -> Result<WraithMessage>` in `message/codec.rs`

---

### 2.3 Dedup Checking Block (~30 lines, 2 places)

**Locations:** `tunnel/mod.rs:288-312`, `tunnel/mod.rs:578-602`

```rust
let msg_id = msg.message_id.clone();
let pending_tx = {
    let state_guard = state.lock().unwrap();
    state_guard.as_ref().and_then(|s| s.lock().unwrap().take_pending_response(&msg_id))
};
let already_seen = {
    let state_guard = state.lock().unwrap();
    if let Some(ref s) = *state_guard {
        let mut s = s.lock().unwrap();
        if s.has_seen_message(&msg_id) { true }
        else { s.mark_message_seen(msg_id.clone()); false }
    } else { false }
};
if already_seen { info!("Skipping duplicate message: {}", msg_id); continue; }
```

**Extract:** `check_and_mark_message_seen(state, msg_id) -> (bool, Option<oneshot::Sender<WraithMessage>>)`

---

### 2.4 Yamux Driver Spawn (~20 lines, 2 places)

**Locations:** `tunnel/mod.rs:181-196`, `tunnel/mod.rs:471-485`

```rust
let conn_handle = Arc::new(tokio::sync::Mutex::new(conn));
let conn_handle_for_spawn = conn_handle.clone();
tokio::spawn(async move {
    let mut c = conn_handle_for_spawn.lock().await;
    loop {
        match futures::future::poll_fn(|cx| Pin::new(&mut c).poll_next_inbound(cx)).await {
            Some(Ok(_)) => { /* handled elsewhere */ }
            Some(Err(e)) => { warn!("error: {}", e); break; }
            None => break;
        }
    }
});
```

**Extract:** `spawn_yamux_driver(conn: Connection) -> Arc<tokio::sync::Mutex<Connection>>`

---

### 2.5 Peer Session Registration (~50 lines, 2 places)

**Locations:** `tunnel/mod.rs:208-252`, `tunnel/mod.rs:516-541`

Both contain:
1. Read registration message
2. Create mpsc channel
3. Create PeerSession
4. Insert into sessions map
5. Call peer_add_callback
6. Spawn writer task
7. Get new stream for message loop
8. Set up message loop

**Extract:** `register_peer_session(...) -> PeerSession` async function

---

### 2.6 Command Dispatch Match (~30 lines, 3 places)

**Locations:** `wraith/wraith.rs:244-260`, `wraith/wraith.rs:419-433`, `tunnel/mod.rs:419-435`

```rust
if cmd.action == "create_relay" {
    relay_commands.lock().unwrap().handle_create_relay(cmd, &local_wraith_id)
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
} else { return None; };
```

**Extract:** `dispatch_command(cmd, relay_commands, agent_commands, state) -> CommandResult`

---

### 2.7 State Lock + add_peer (~8 lines, 2 places)

**Locations:** `wraith/wraith.rs:55-58`, `wraith/wraith.rs:71-74`

```rust
let mut s = state_clone.lock().unwrap();
s.add_peer(wraith_id.to_string(), hostname.to_string(), sender.clone());
```

**Extract:** `add_peer_to_state(state, wraith_id, hostname, sender)`

---

### 2.8 CommandResult Creation (~15 lines, 3 places)

**Locations:** `wraith/wraith.rs:264-271`, `wraith/wraith.rs:439-446`, `tunnel/mod.rs:439-446`

```rust
return Some(MessageCodec::create_command_result(
    result.command_id, result.status, result.output,
    result.exit_code, result.duration_ms, result.error,
));
```

**Already extracted:** This is in `MessageCodec::create_command_result()` - just use it consistently.

---

## 3. Design Principles

1. **Single source of truth** — One peer state, one routing implementation
2. **DRY** — Every piece of logic in exactly one place
3. **Explicit over implicit** — No hidden callbacks, clear data flow
4. **Idiomatic Rust** — Proper async/sync separation, `tokio::sync` primitives where appropriate
5. **Fearless refactoring** — Code that's easy to understand and change

---

## 4. Architecture Changes

### 4.1 State Consolidation

**Current (two peer tables):**
```rust
// WraithState has peer_table
pub struct WraithState {
    pub peer_table: HashMap<String, PeerConnection>,
}

// TunnelManager has sessions (DUPLICATE)
pub struct TunnelManager {
    sessions: Arc<RwLock<HashMap<String, PeerSession>>>,
}
```

**Proposed (single peer table):**
```rust
// WraithState is the ONLY peer table
pub struct WraithState {
    pub peer_table: HashMap<String, PeerSession>,  // Unified
}

// TunnelManager accesses state directly
pub struct TunnelManager {
    state: Arc<Mutex<WraithState>>,
    relay_commands: Arc<Mutex<RelayCommands>>,
    agent_commands: Arc<Mutex<AgentCommands>>,
}
```

### 4.2 Routing Unification

**Current:** `MessageDispatcher::route_message()` AND `TunnelManager::route_message()` (duplicated)

**Proposed:** Only `TunnelManager::route_message()`

- Delete `MessageDispatcher` entirely
- All routing goes through `TunnelManager`

### 4.3 Simplified Initialization

**Current (double creation bug):**
```rust
let tunnel_manager = Arc::new(TunnelManager::new());  // First
tunnel_manager.set_peer_add_callback(...);             // Registered on #1
// ...
let tunnel_manager = Arc::new(TunnelManager::with_commands(...));  // Second (drops first!)
tunnel_manager.set_peer_add_callback(...);                     // Registered on #2
```

**Proposed:**
```rust
let state = Arc::new(Mutex::new(WraithState::new(wraith_id)));
let tunnel_manager = Arc::new(TunnelManager::new(Arc::clone(&state)));
// Single registration
```

---

## 5. Shared Functions to Create

| Function | Location | Purpose |
|----------|----------|---------|
| `read_framed_message()` | `message/codec.rs` | Unified length-prefix message reading |
| `spawn_yamux_driver()` | `wraith/tunnel/mod.rs` | Spawn Yamux connection driver |
| `register_peer_session()` | `wraith/tunnel/mod.rs` | Register new peer session |
| `check_and_mark_seen()` | `wraith/state.rs` | Message deduplication |
| `dispatch_command()` | `wraith/tunnel/mod.rs` | Route command to handler |
| `add_peer_to_state()` | `wraith/state.rs` | Add peer to state |
| `read_message_loop()` | `wraith/wraith.rs` | Generic message loop |

---

## 6. File Changes

### 6.1 Delete

| File | Reason |
|------|--------|
| `src/wraith/dispatcher.rs` | Duplicates TunnelManager routing |
| `src/connection/connection.rs` | Trait unused, also async-fn-in-trait lint |
| `src/connection/yamux.rs` | Unused struct |

### 6.2 Simplify

| File | Changes |
|------|---------|
| `src/wraith/tunnel/mod.rs` | Remove `sessions`, use `WraithState::peer_table` directly |
| `src/wraith/tunnel/session.rs` | Use shared `read_framed_message()` |
| `src/connection/tcp.rs` | Use shared `read_framed_message()`, remove duplicate impl |
| `src/message/codec.rs` | Add `read_framed_message()`, remove unused `create_relay_*` |
| `src/commands/command.rs` | Remove trait (or fix to accept state) |
| `src/relay/mod.rs` | Remove unused `SessionState` fields |

---

## 7. Bug Fixes

### 7.1 Await-While-Holding-Lock

**File:** `wraith/wraith.rs`

```rust
// BROKEN:
let state = self.state.lock().unwrap();
conn.send_message(&msg).await?;  // Await while holding lock!

// FIXED:
let (hostname, username, os, ip) = {
    let state = self.state.lock().unwrap();
    (state.hostname.clone(), state.username.clone(), state.os.clone(), state.ip.clone())
};
// Lock dropped
conn.send_message(&msg).await?;  // Safe
```

### 7.2 TunnelManager Double-Creation

```rust
// BROKEN: Creates two TunnelManagers, first is discarded

// FIXED: Single TunnelManager
pub fn new(wraith_id: &String) -> Self {
    let state = Arc::new(Mutex::new(WraithState::new(wraith_id)));
    let tunnel_manager = Arc::new(TunnelManager::new(Arc::clone(&state)));
    Self { state, tunnel_manager, ... }
}
```

---

## 8. Clippy Compliance

| Issue | Fix |
|-------|-----|
| `await_holding_lock` | Drop lock before await |
| `async-fn-in-trait` | Remove `Connection` trait |
| Unused imports | Remove from `tunnel/mod.rs`, `session.rs` |
| Unused `mut` | Remove `mut` where not needed |
| Unused variables | Prefix with `_` or remove |
| `dead_code` | Remove unused structs/methods |

---

## 9. Implementation Order

```
Phase 1: Fix Critical Bugs
├── Fix await-while-holding-lock
└── Fix TunnelManager double-creation

Phase 2: Create Shared Functions
├── Add read_framed_message() to codec.rs
├── Add spawn_yamux_driver()
├── Add check_and_mark_seen() to state.rs
├── Add dispatch_command()
└── Add add_peer_to_state()

Phase 3: Eliminate Duplication
├── Replace duplicate read_message implementations
├── Replace duplicate command dispatch
├── Replace duplicate dedup checking
└── Replace duplicate session registration

Phase 4: Remove Dead Code
├── Delete dispatcher.rs
├── Delete connection/connection.rs
├── Delete connection/yamux.rs
└── Remove unused codec methods

Phase 5: Cleanup
├── Remove Command trait
├── Simplify TunnelManager struct
├── Remove duplicate peer tables
└── Fix all clippy warnings
```

---

## 10. Success Criteria

- [ ] `cargo clippy -- -D warnings` passes
- [ ] Zero code duplication (all 8 patterns consolidated)
- [ ] Single peer table (no `peer_table` AND `sessions`)
- [ ] Single routing implementation (no `MessageDispatcher`)
- [ ] No locks held across `.await` points
- [ ] All tests pass
- [ ] Single source for each piece of logic

---

## 11. Out of Scope

- Protobuf definition changes
- Python client changes (`home/PyWraith/`)
- Nexus integration changes
- Feature additions

---

*End of Design*
