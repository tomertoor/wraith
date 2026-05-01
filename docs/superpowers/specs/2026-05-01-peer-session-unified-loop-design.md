# Peer Session Unified Message Loop — Refactoring Spec

## Context

`handle_peer_connection` (server side) and `connect_to_peer` (client side) in `src/wraith/tunnel/mod.rs` share similar structure but have divergent implementations. Both create a Yamux connection, spawn a forwarder task, and for the server, run a message loop. The client side lacks a message loop and cannot receive independent peer commands.

The goal is to unify these code paths and introduce a shared `PeerSession` abstraction with a single task handling both inbound and outbound traffic.

## Current Problems

1. **Duplicated setup code** — Yamux connection setup, background driver spawn, forwarder spawn are duplicated
2. **Asymmetric behavior** — client has no message loop to handle inbound commands from peers
3. **Broken forwarder protocol** — client forwarder uses raw `write_all` while server uses `PeerSession::write_message`, but since they're equivalent this was never a bug, just a DRY violation
4. **No clean API for local → peer communication** — local components use the `command_tx` channel directly with fire-and-forget semantics

## Design

### Core abstraction

Each `PeerSession` owns **one task** that handles all communication with a connected peer. The task reads from two sources:
- The Yamux stream (inbound messages from peer)
- A local mpsc channel (outbound requests from local components)

Local components send messages to peers via `peer_session.send_and_wait(msg)` which returns a future that resolves with the peer's response.

### Data structures in `PeerSession`

```rust
pub struct PeerSession {
    wraith_id: String,
    hostname: String,
    conn_handle: Arc<tokio::sync::Mutex<yamux::Connection>>,
    local_tx: mpsc::Sender<(WraithMessage, oneshot::Receiver<WraithMessage>)>,
    local_rx: mpsc::Receiver<(WraithMessage, oneshot::Receiver<WraithMessage>)>,
    pending_outbound: HashMap<String, oneshot::Sender<WraithMessage>>,
}
```

Note: The forwarder task is replaced entirely by the message loop. The `local_tx` replaces what was previously `command_tx`.

### `send_and_wait` API

```rust
impl PeerSession {
    pub async fn send_and_wait(
        &self,
        msg: WraithMessage,
    ) -> Result<WraithMessage, RecvError> {
        // Creates oneshot channel
        // Stores sender in pending_outbound keyed by msg.message_id
        // Sends (msg, oneshot_rx) via local_tx
        // Awaits and returns the response
    }
}
```

### Message loop flow

```rust
async fn run_message_loop(&self, mut stream: yamux::Stream) {
    loop {
        tokio::select! {
            // Inbound from peer
            msg = Self::read_message(&mut stream) => {
                match msg {
                    Ok(Some(msg)) => {
                        let msg_id = msg.message_id.clone();

                        // Check if this is a response to our outbound request
                        if let Some(tx) = self.pending_outbound.remove(&msg_id) {
                            let _ = tx.send(msg);
                            continue;
                        }

                        // Dedup check
                        if self.state.has_seen_message(&msg_id) {
                            continue;
                        }
                        self.state.mark_message_seen(msg_id);

                        // Route
                        if msg.target_wraith_id == self.wraith_id {
                            // Dispatch locally
                            self.dispatcher.route_message(msg, self.state.clone()).await;
                        } else {
                            // Forward to another peer via send_and_wait on that peer
                            self.route_to_peer(msg).await;
                        }
                    }
                    Ok(None) => break, // Stream ended
                    Err(_) => break,
                }
            }

            // Outbound from local components
            (msg, resp_rx) = self.local_rx.recv() => {
                // Track the response channel so response gets routed back
                self.pending_outbound.insert(msg.message_id.clone(), resp_rx);
                // Write to peer stream
                Self::write_message(&mut stream, &msg).await;
                // Loop continues — response will arrive via the stream read arm
            }
        }
    }
}
```

### Shared helper functions

1. **`spawn_yamux_driver(conn: yamux::Connection) -> Arc<tokio::sync::Mutex<yamux::Connection>>`**
   - Drives the Yamux connection in background (polls both inbound and outbound)
   - Returns the handle shared by the message loop and session

2. **`open Yamux stream helper`**
   - Server: `poll_next_inbound` on connection
   - Client: `poll_new_outbound` on connection
   - Both return a `yamux::Stream`

3. **`PeerSession::new(...)` constructor**
   - Creates local_tx/local_rx channel pair
   - Spawns the message loop task
   - Returns ready-to-use PeerSession

### How `handle_peer_connection` and `connect_to_peer` change

Both functions shrink to:
1. Create Yamux connection (server vs client mode)
2. Call `spawn_yamux_driver`
3. Open stream 0
4. Server: read registration, Client: send registration
5. Create PeerSession via constructor (which spawns message loop)
6. Add to tunnel manager

The message loop is now fully encapsulated in `PeerSession`.

### Refactoring steps

1. Add `pending_outbound` HashMap and `local_rx` channel to `PeerSession`
2. Rename `command_tx` to `local_tx` for clarity
3. Implement `send_and_wait` method
4. Implement `run_message_loop` method
5. Extract shared `spawn_yamux_driver` helper
6. Add `route_to_peer` method for forwarding to other peers
7. Refactor `handle_peer_connection` to use shared helpers and new `PeerSession`
8. Refactor `connect_to_peer` to use shared helpers and new `PeerSession` (add message loop)
9. Verify tests pass

## Open questions

1. What happens when `send_and_wait` is called but the peer loop is dead? The oneshot will be dropped. Should we return an error?
2. Should there be a timeout on `send_and_wait`? The current state infrastructure doesn't track timeouts.
3. How does `route_to_peer` look up the target peer? Via `TunnelManager::get_session` — need to pass a reference to the manager into the loop somehow. This needs clarification.