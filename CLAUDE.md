# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Wraith is a pentesting tunnel tool with two components:
- **Rust Agent (`src/`)**: The core wraith agent that runs on target systems
- **Python Client (`home/PyWraith/`)**: A Python package used to command the wraith agent

## Build Commands

### Rust
```bash
cargo build          # Debug build
cargo build --release # Release build
cargo run -- --help  # Run with arguments
```

### Python
```bash
cd home && ./setup.sh  # Install PyWraith
```

## Run Commands

### Rust Agent (C2 Mode)
```bash
# Connect mode (client) - wraith connects TO C2
cargo run -- --host 127.0.0.1 --port 4444

# Listen mode (server) - wraith listens for C2
cargo run -- --host 0.0.0.0 --port 4444 --listen

# With logging
cargo run -- --host 127.0.0.1 --port 4444 --debug --log-file wraith.log
```

### Rust Agent (Agent Mode - Stage 2)
```bash
# Agent listen - wraith listens for C2 AND peer wraith connections
cargo run -- --agent-listen 0.0.0.0:4445

# Agent connect - wraith connects to C2 and optionally peers
cargo run -- --agent-connect 10.0.0.1:4444
```

### PyWraith Client
```bash
pywraith --host 127.0.0.1 --port 4444
```

### Relay Commands
```bash
# Single-hop relay (legacy)
create_relay <listen_host> <listen_port> <forward_host> <forward_port> <tcp|udp>

# Multi-hop relay chain (new)
# -t = TCP hop, -u = UDP hop
# hop[N] forwards to hop[N+1]'s listen addr; last hop forwards to explicit final addr
create_relay -t <host> <port> -u <host> <port> ... [final_host] [final_port]

# Examples:
# TCP -> UDP chain:
create_relay -t 127.0.0.1 6666 -u 127.0.0.1 7777 10.0.0.1 443

# 6-hop chain: TCP TCP UDP TCP TCP UDP
create_relay -t A B -t C D -u E F -t G H -t I J -u K L 10.0.0.1 443
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         PyWraith                                 │
│  cli.py → client.py → protocol.py → socket (TCP)                │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                         Wraith (Rust)                            │
│  connection/tcp.rs ←→ message/codec.rs ←→ protobuf               │
│         │                                                       │
│         ▼                                                       │
│  wraith/wraith.rs (main loop + dispatcher)                       │
│         │                                                       │
│         ├── commands/ (command handlers)                        │
│         │     └── relay.rs → relay/mod.rs (all relay impl)     │
│         ├── tunnel/ (peer session management)                   │
│         └── wraith/state.rs (shared state with Mutex)           │
└─────────────────────────────────────────────────────────────────┘
```

### Key Modules (Rust)

| Module | Purpose |
|--------|---------|
| `src/wraith/wraith.rs` | Main entry point, connection lifecycle, message dispatch loop, agent listener/connect modes |
| `src/connection/` | TCP connection handling (client/server modes), YamuxConnection for peer communication |
| `src/message/codec.rs` | Protobuf message creation and parsing |
| `src/commands/` | Command handlers - maps action strings to implementations |
| `src/relay/mod.rs` | Relay implementations (TCP/UDP), `RelayManager` owns active relays; session-based UDP relay with persistent sockets |
| `src/wraith/tunnel/` | Peer session management via Yamux - `TunnelManager` and `PeerSession` for agent-to-agent communication |

### Protobuf Definition
`proto/wraith.proto` defines `WraithMessage` with `oneof payload` containing:
- `Registration` - hostname, username, os, ip
- `Command` / `CommandResult` - request/response for actions
- `RelayCreate` / `RelayDelete` / `RelayList` / `RelayListResponse` - relay management
- `WraithRegistration` - peer wraith registration with wraith_id
- `PeerUpdate` - peer connect/disconnect notifications
- `PeerList` / `PeerListResponse` - peer discovery commands

### Message Flow
1. PyWraith sends `WraithMessage` with `Command` payload
2. Wraith's `connection.read_message()` reads framed data
3. `Wraith::run()` dispatches to `MessageDispatcher`
4. `CommandHandler` routes by `action` field to appropriate handler
5. Response sent back via same connection

## Commands

The wraith agent supports these actions via `Command.action`:

### Relay Commands
| Action | Params | Description |
|--------|--------|-------------|
| `create_relay` | `listen_host`, `listen_port`, `forward_host`, `forward_port`, `protocol` | Create TCP/UDP relay |
| `delete_relay` | `relay_id` | Delete relay by ID |
| `list_relays` | (none) | List all active relays |

### Agent Commands (Stage 2)
| Action | Params | Description |
|--------|--------|-------------|
| `set_id` | `wraith_id` | Set wraith's ID at runtime |
| `list_peers` | (none) | List direct neighbor wraiths |

### Remote Relay (Stage 2)
| Action | Params | Description |
|--------|--------|-------------|
| `create_relay` | `target_wraith_id`, `listen_host`, `listen_port`, `forward_host`, `forward_port` | Create relay on remote wraith via chain |

## Agent Network (Stage 2)

Wraiths can connect to each other forming a chain: `C2 → Wraith A → Wraith B`

### Network Topology
- Each wraith maintains one connection to C2
- Each wraith can maintain zero or more connections to peer wraiths (full mesh capable)
- Commands include `target_wraith_id` for routing through the chain

### Command Routing
1. Check local wraith_id - if matches, execute locally
2. Check peer table - if target is direct peer, forward via Yamux
3. Otherwise broadcast to all peers (with loop prevention via message_id tracking)

### Yamux Integration
- Stream 0: Command/control traffic (bidirectional)
- Stream 1+: Relay data (one stream per active relay)

## Protobuf Code Generation

### Rust
`build.rs` compiles `proto/wraith.proto` → `src/proto/wraith.rs` using `prost-build`. Regenerate with:
```bash
cargo build
```

### Python
Regenerate Python protobuf:
```bash
protoc --python_out=PyWraith/proto_gen -I../proto ../proto/wraith.proto
```

## Testing

```bash
cargo test
```

## Key Files

- `proto/wraith.proto` - Protocol buffer message definitions (includes WraithRegistration, PeerUpdate, PeerList for agent network)
- `src/main.rs` - Rust entry point with clap CLI parsing (includes --agent-listen, --agent-connect)
- `src/wraith/wraith.rs` - Core Wraith struct and main run loop (includes agent listener/connect modes)
- `src/wraith/tunnel/` - Peer session management (TunnelManager, PeerSession)
- `src/relay/mod.rs` - All relay implementations (TCP/UDP, session-based UDP, relay chains)
- `src/commands/relay.rs` - Command handler for relay operations
- `src/commands/agent.rs` - Agent commands (set_id, list_peers)
- `home/PyWraith/client.py` - Python client class (includes set_id, list_peers, list_peers_recursive)
- `home/PyWraith/protocol.py` - Python protobuf framing/encoding
- `home/PyWraith/cli.py` - Python CLI implementation
