# Wraith Structural Refactoring Design

**Date:** 2026-05-07
**Status:** Approved
**Scope:** Error handling, state simplification, dedup, module split, selective traits

## Goals

1. Improve error handling (thiserror, remove unwraps, fix dead-code bugs)
2. Flatten state wrapping, simplify constructors, eliminate optionality hacks
3. Eliminate duplicated message loops, writer tasks, CommandResult construction
4. Split large files into focused modules (tunnel/mod.rs 652 lines, relay/mod.rs 557 lines)
5. Add traits only where polymorphic dispatch exists (YAGNI for others)

## Constraints

- CLI interface remains backward-compatible
- External behavior preserved (no protocol changes)
- All existing tests must pass

---

## Implementation Phases (Correct Order)

Phases are ordered by dependency: each builds on the previous.

### Phase 1: Error Handling & Bug Fixes (Low Risk)

**1a. Add thiserror for WraithError**
- Replace manual `Display` and `Error` impls in `wraith/wraith.rs` with `#[derive(thiserror::Error)]`

**1b. Replace `.lock().unwrap()` with `.expect("reason")`**
- Every `lock().unwrap()` gets a descriptive message
- Target: ~48 instances across all files

**1c. Delete the `Command` trait**
- `commands/command.rs` contains `trait Command` with `fn execute(&self, cmd) -> CommandResult`
- This trait has no way to access `WraithState`, making it fundamentally broken
- `AgentCommands::execute()` creates `WraithState::new()` (dummy state) — dead code with latent bug
- `RelayCommands::execute()` passes `""` as `local_wraith_id` — dead code
- Real routing is done by `dispatch_command()` which calls `handle_*` methods directly
- Action: Delete `commands/command.rs`, remove `impl Command for AgentCommands`, `impl Command for RelayCommands`

**1d. Add CommandResult builders**
- Replace 12+ repeated `CommandResult { command_id: ..., status: ..., output: ..., exit_code: ..., duration_ms: 0, error: ... }` constructions
- Add `CommandResult::success(id, output)` and `CommandResult::error(id, msg)`
- Apply across `relay.rs` and `agent.rs`

**1e. Add `CancellationToken` to PeerSession**
- Writer tasks currently hang indefinitely on `rx.recv()` after peer disconnect
- Add `tokio_util::sync::CancellationToken` to `PeerSession`
- Cancel on drop to signal writer task termination

### Phase 2: State Simplification (Medium Risk)

**2a. Flatten `Arc<Mutex<Option<Arc<Mutex<WraithState>>>>>`**
- Current: `TunnelManager` stores `state: Arc<Mutex<Option<Arc<Mutex<WraithState>>>>>`
- Target: `state: Arc<Mutex<WraithState>>` (required at construction)
- Constructor injection: `TunnelManager::new(state, relay_commands, agent_commands)`
- Remove `set_state()`, `set_commands()`, `new()` without params

**2b. Remove `new_without_tunnel()` constructors**
- `RelayCommands::new_without_tunnel()` and `AgentCommands::new_without_tunnel()` exist only for `TunnelManager::new()` dummy init
- With proper constructor injection, they are unnecessary

**2c. Extract DedupState**
- Merge `seen_message_ids: std::sync::Mutex<HashSet<String>>` and `pending_responses: std::sync::Mutex<HashMap<String, oneshot::Sender<WraithMessage>>>` into one struct
- Use `DashSet` (already a dependency via `dashmap`) for `seen_ids`
- Single lock scope eliminates double-lock in `check_and_mark_seen()`

**2d. Replace callbacks with channels**
- Replace `Box<dyn Fn(&str, &str, &Sender)>` callbacks with `mpsc::Sender<PeerEvent>`
- `PeerEvent` enum: `Added { wraith_id, hostname }`, `Removed { wraith_id }`

**2e. Remove duplicate state methods**
- Delete `add_peer_to_state()` — keep `add_peer()` only
- Delete `new_with_relay_manager()` — single constructor pattern

**2f. Fix `Wraith::clone()` dropping connection**
- Current: `clone()` sets `connection: None`, producing a broken object
- Option: Remove `Clone` impl from `Wraith`, use `Arc<Wraith>` where needed
- Or: Split `Wraith` into `WraithHandle` (clonable, Arc state) and `C2Connection` (non-clonable, owns stream)

### Phase 3: Deduplication (Medium Risk)

**3a. Extract shared peer message loop**
- `tunnel/mod.rs` lines 329-394 (inbound peer) and 570-640 (outbound peer) are nearly identical
- Extract to `run_peer_message_loop(stream, state, router)` free function
- Both `handle_peer_connection` and `connect_to_peer` call this

**3b. Move writer task into PeerSession**
- Lines 300-309 and 546-554: identical writer task spawning
- Add `PeerSession::spawn_writer(write_half)` method

**3c. Extract C2 connection loop**
- `wraith.rs` has three nearly identical loops: `run()` lines 115-135, `handle_c2_connection()` lines 197-217
- Extract to `run_c2_message_loop(conn: &mut TcpConnection, router: &Router, state: &Arc<Mutex<WraithState>>)`
- Registration is NOT part of this loop (it happens before, differently for each mode)

### Phase 4: Module Split (Medium Risk)

**4a. Rename `wraith/tunnel/` to `session/`**
- Current `tunnel/mod.rs` (652 lines) splits into:
  - `session/mod.rs` — re-exports
  - `session/peer.rs` — `PeerSession` struct + writer task + CancellationToken
  - `session/message_loop.rs` — shared `run_peer_message_loop()`
  - `session/registry.rs` — `SessionRegistry` (renamed from TunnelManager), manages peer sessions + callbacks

**4b. Extract `router/` from session and dispatch**
- `router/mod.rs` — re-exports
- `router/dispatcher.rs` — `dispatch_command()` + `route_message()` + `Router` struct
- Router is the bridge between C2 and peer sessions

**4c. Split `relay/mod.rs` (557 lines)**
- `relay/mod.rs` — re-exports + types (Transport, RelayEndpoint, RelayConfig, RelayInfo)
- `relay/manager.rs` — `RelayManager` (stores relays, creates/deletes/lists)
- `relay/tcp_relay.rs` — TCP relay implementation
- `relay/udp_relay.rs` — UDP relay implementation (SessionState + session_task)
- `relay/forward.rs` — Generic bidirectional relay functions (`relay_bidirectional`, `relay_stream_to_datagram`, `relay_connection`)

### Phase 5: Selective Traits (Lower Risk, Only If Needed)

**5a. Connection trait — DEFERRED**
- The Rust review found no polymorphic dispatch site for connections
- C2 uses `TcpConnection`, peers use yamux streams — never stored in the same collection
- If needed in future, use generic function `fn run_loop(conn: impl Connection)` (monomorphized, zero-cost)
- Do NOT add this trait now

**5b. Relay trait — SIMPLIFY**
- Remove `RelayTrait` trait object and `Arc<dyn RelayTrait>`
- Store `ProtocolRelay` directly in `RelayManager`
- The `#[async_trait]` attribute on `RelayTrait` becomes unnecessary
- Use native async fn for the relay's start method

**5c. Session trait — DEFERRED**
- Only one implementation (`PeerSession`), YAGNI
- Extract later if a second transport type is added

---

## Mutex Strategy (Per Review)

| Field | Current | Target | Reason |
|-------|---------|--------|--------|
| `WraithState` (outer) | `std::sync::Mutex` | `std::sync::Mutex` | Never held across `.await` |
| `RelayManager` | `std::sync::Mutex` | `std::sync::Mutex` | Same |
| `seen_message_ids` | `std::sync::Mutex<HashSet>` | `DashSet` | Already have dashmap dep, eliminates locking |
| `pending_responses` | `std::sync::Mutex<HashMap>` | `std::sync::Mutex<HashMap>` | Brief holds, keep simple |
| `yamux::Connection` handle | `tokio::sync::Mutex` | `tokio::sync::Mutex` | Held across `.await` |
| `peer_add/remove_callback` | `std::sync::Mutex<Option<Box<...>>>` | `mpsc::Sender<PeerEvent>` | Channel-based events |
| `relay/agent_commands` | `Arc<Mutex<...>>` in TunnelManager | Direct fields, set once | Constructor injection |

---

## New Module Structure

```
src/
├── lib.rs
├── main.rs                          (unchanged)
├── commands/
│   ├── mod.rs
│   ├── relay.rs                     (simplified: CommandResult builders, no Command trait)
│   └── agent.rs                     (simplified: no Command trait, no dummy state)
├── connection/
│   ├── mod.rs                       (re-export)
│   ├── framing.rs                   (kept)
│   ├── tcp.rs                       (TcpConnection)
│   └── yamux.rs                     (yamux helpers + spawn_yamux_driver)
├── message/
│   ├── mod.rs
│   └── codec.rs                     (unchanged)
├── proto/                           (unchanged - generated)
├── relay/
│   ├── mod.rs                       (re-export + types: Transport, RelayEndpoint, RelayConfig, RelayInfo)
│   ├── manager.rs                   (RelayManager — stores ProtocolRelay directly)
│   ├── tcp_relay.rs                 (ProtocolRelay TCP impl)
│   ├── udp_relay.rs                 (ProtocolRelay UDP impl + SessionState)
│   └── forward.rs                   (relay_bidirectional, relay_stream_to_datagram, relay_connection)
├── session/
│   ├── mod.rs                       (re-export)
│   ├── peer.rs                      (PeerSession + writer task + CancellationToken)
│   ├── message_loop.rs              (shared run_peer_message_loop)
│   └── registry.rs                  (SessionRegistry — renamed from TunnelManager)
├── router/
│   ├── mod.rs                       (re-export)
│   └── dispatcher.rs                (dispatch_command + route_message + Router struct)
└── wraith/
    ├── mod.rs                       (re-export)
    ├── config.rs                    (review for removal — may be unused)
    ├── state.rs                     (WraithState + DedupState, simplified constructors)
    └── wraith.rs                    (Wraith struct, C2 connection management, shared C2 loop)
```

---

## Deleted Code

| What | Why |
|------|-----|
| `commands/command.rs` | Dead trait — `dispatch_command` is the real router |
| `impl Command for AgentCommands` | Creates dummy `WraithState::new()`, never called |
| `impl Command for RelayCommands` | Passes `""` as `local_wraith_id`, never called |
| `TunnelManager::new()` | Creates dummy commands + orphaned RelayManager |
| `set_state()`, `set_commands()` | Replaced by constructor injection |
| `RelayTrait` trait + `Arc<dyn RelayTrait>` | Only one impl, trait object is overhead |
| `async-trait` dependency | Use native async fn in traits (Rust 1.75+) |
| `WraithState::new_with_relay_manager()` | Duplicate constructor |
| `WraithState::add_peer_to_state()` | Duplicate of `add_peer()` |

---

## Key Architectural Decision: C2 vs Peer Separation

```
C2 Connection (wraith.rs)  ──┐
                              ├──► Router (router/dispatcher.rs) ──► Commands
Peer Sessions (session/)  ───┘         │
                                       ▼
                              Session Registry (forward to peer)
```

- **C2**: Owned by `wraith.rs`, uses `TcpConnection`, single connection
- **Peers**: Owned by `session/registry.rs`, uses yamux multiplexed streams
- **Router**: Shared message routing layer that both C2 and peers feed into
- **Relay**: Independent subsystem, only accessed through commands

## Cross-Protocol Relay Support

The refactored relay module preserves cross-protocol capability:
- `RelayEndpoint` carries a `Transport` (Tcp/Udp) for both listen and forward
- `RelayManager` creates `ProtocolRelay` instances where listen and forward transports can differ
- `relay_connection()` dispatches based on forward transport: TCP→TCP, TCP→UDP, UDP→TCP all supported
- Splitting into `tcp_relay.rs` and `udp_relay.rs` organizes by listen-side transport; the forward side is handled in `forward.rs`

## Graceful Shutdown

Add `CancellationToken` from `tokio_util`:
- Each `PeerSession` owns a `CancellationToken`
- Writer task checks `token.cancelled()` in its recv loop
- On session drop or disconnect, token is cancelled → writer terminates
- `RelayManager` already uses `oneshot::Sender` for shutdown — this is fine
- Future: add a top-level `CancellationToken` to `Wraith` for SIGTERM handling
