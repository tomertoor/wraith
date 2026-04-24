# Protocol Relay Design

## Overview

Replace the existing relay system (TcpRelay, UdpRelay, RelayChain) with a single `ProtocolRelay` that supports independent transport protocols on listen and forward sides. Mimics socat's `PROTOCOL-LISTEN:port,fork PROTOCOL:target` behavior.

## Proto Changes

**File:** `proto/wraith.proto`

```protobuf
message RelayEndpoint {
    string host = 1;
    uint32 port = 2;
    string protocol = 3;  // "tcp" or "udp"
}

message RelayConfig {
    RelayEndpoint listen = 1;
    RelayEndpoint forward = 2;
}
```

Remove all existing relay-related fields. Backward compatibility is explicitly not a goal.

## Rust Side

### New Types — `src/relay/mod.rs`

```rust
pub struct RelayEndpoint {
    pub host: String,
    pub port: u16,
    pub protocol: Transport,
}

impl RelayEndpoint {
    pub fn new(host: String, port: u16, protocol: Transport) -> Self;
    pub fn from_str(host: &str, port: u16, protocol: &str) -> Self;
}

pub struct ProtocolRelay {
    id: String,
    listen: RelayEndpoint,
    forward: RelayEndpoint,
    active: Arc<AtomicBool>,
}

impl ProtocolRelay {
    pub fn new(listen: RelayEndpoint, forward: RelayEndpoint) -> Self;
}
```

### `RelayTrait` implementation

```rust
impl RelayTrait for ProtocolRelay {
    fn id(&self) -> &str;
    fn config(&self) -> &RelayConfig;
    fn is_active(&self) -> bool;

    async fn start_relay(self: Arc<Self>, shutdown: oneshot::Receiver<()>) -> Result<()>;
    fn to_relay_info(&self) -> RelayInfo;
}
```

`start_relay` logic:
1. If `listen.protocol == TCP`: bind `TcpListener`, loop accepting connections
2. If `listen.protocol == UDP`: bind `UdpSocket`, loop receiving datagrams
3. For each inbound connection/datagram, spawn a task:
   - If `forward.protocol == TCP`: `TcpStream::connect()` to forward address
   - If `forward.protocol == UDP`: `UdpSocket::bind()` + `send_to()` to forward address
   - Bidirectional copy between inbound and outbound sockets
4. On shutdown, set `active = false`, let tasks drain

For UDP→TCP: inbound packet arrives, send to forward TCP target, wait for response (500ms timeout), relay back to UDP source.

### `RelayManager` — `src/relay/mod.rs`

```rust
impl RelayManager {
    pub fn create_relay(&mut self, config: RelayConfig) -> String {
        let relay = Arc::new(ProtocolRelay::new(
            RelayEndpoint::from_str(&config.listen.host, config.listen.port as u16, &config.listen.protocol),
            RelayEndpoint::from_str(&config.forward.host, config.forward.port as u16, &config.forward.protocol),
        ));
        let id = relay.id().to_string();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.relays.insert(id.clone(), (relay, shutdown_tx));
        tokio::spawn(async move {
            let _ = relay_clone.start_relay(shutdown_rx).await;
        });
        id
    }
}
```

Remove `create_relay_chain` method.

### Command handler — `src/commands/relay.rs`

Simplified `handle_create_relay`:
```rust
pub fn handle_create_relay(&self, cmd: &ProtoCommand) -> CommandResult {
    let listen = RelayEndpoint::from_str(
        cmd.params.get("listen_host").unwrap(),
        cmd.params.get("listen_port").unwrap().parse().unwrap(),
        cmd.params.get("listen_protocol").unwrap(),
    );
    let forward = RelayEndpoint::from_str(
        cmd.params.get("forward_host").unwrap(),
        cmd.params.get("forward_port").unwrap().parse().unwrap(),
        cmd.params.get("forward_protocol").unwrap(),
    );

    let relay = ProtocolRelay::new(listen, forward);
    // ... spawn and return result
}
```

### Files to remove

- `src/relay/tcp.rs`
- `src/relay/udp.rs`
- `src/relay/chain.rs`
- Remove `pub use chain::RelayChain` from `mod.rs`
- Remove `pub mod chain` from `mod.rs`

## Python Side

### Protocol — `home/PyWraith/protocol.py`

```python
def create_relay_command(command_id, listen_host, listen_port, listen_protocol,
                         forward_host, forward_port, forward_protocol, timeout=30):
    # listen_protocol: "tcp" or "udp"
    # forward_protocol: "tcp" or "udp"
```

### Client — `home/PyWraith/client.py`

```python
def create_relay(self, listen_host, listen_port, listen_protocol,
                 forward_host, forward_port, forward_protocol):
```

### CLI — `home/PyWraith/cli.py`

New syntax:
```bash
create_relay -l tcp -L 0.0.0.0 2222 -f udp -F 127.0.0.1 3333
```

Flags:
- `-l tcp|udp` — listen protocol (e.g., `-l tcp`)
- `-L <host> <port>` — listen address, two args (e.g., `-L 0.0.0.0 2222`)
- `-f tcp|udp` — forward protocol (e.g., `-f udp`)
- `-F <host> <port>` — forward address, two args (e.g., `-F 127.0.0.1 3333`)

### Proto generated — `home/PyWraith/proto_gen/wraith_pb2.py`

Regenerate from `proto/wraith.proto`.

## Testing

- `cargo test` — Rust tests
- `home/tests/test_protocol.py` — Python tests for `create_relay_command`
- Manual test: TCP listen → UDP forward, UDP listen → TCP forward

## Scope

Single relay with protocol translation. External chaining (multiple relays connected) is the user's responsibility if multi-hop is needed.