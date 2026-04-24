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

### Rust Agent
```bash
# Connect mode (client) - connect TO C2
cargo run -- --host 127.0.0.1 --port 4444

# Listen mode (server) - wraith listens for C2
cargo run -- --host 0.0.0.0 --port 4444 --listen

# With logging
cargo run -- --host 127.0.0.1 --port 4444 --debug --log-file wraith.log
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
│         └── wraith/state.rs (shared state with Mutex)           │
└─────────────────────────────────────────────────────────────────┘
```

### Key Modules (Rust)

| Module | Purpose |
|--------|---------|
| `src/wraith/wraith.rs` | Main entry point, connection lifecycle, message dispatch loop |
| `src/connection/` | TCP connection handling (client/server modes) |
| `src/message/codec.rs` | Protobuf message creation and parsing |
| `src/commands/` | Command handlers - maps action strings to implementations |
| `src/relay/mod.rs` | Relay implementations (TCP/UDP), `RelayManager` owns active relays; session-based UDP relay with persistent sockets |

### Protobuf Definition
`proto/wraith.proto` defines `WraithMessage` with `oneof payload` containing:
- `Registration` - hostname, username, os, ip
- `Command` / `CommandResult` - request/response for actions
- `RelayCreate` / `RelayDelete` / `RelayList` / `RelayListResponse` - relay management
- `RelayHop` - defines a single hop in a relay chain (listen_host, listen_port, forward_host, forward_port, protocol)

### Message Flow
1. PyWraith sends `WraithMessage` with `Command` payload
2. Wraith's `connection.read_message()` reads framed data
3. `Wraith::run()` dispatches to `MessageDispatcher`
4. `CommandHandler` routes by `action` field to appropriate handler
5. Response sent back via same connection

## Commands

The wraith agent supports these actions via `Command.action`:

### Single-Hop Relay (Legacy)
| Action | Params | Description |
|--------|--------|-------------|
| `create_relay` | `listen_host`, `listen_port`, `forward_host`, `forward_port`, `protocol` | Create TCP/UDP relay |
| `delete_relay` | `relay_id` | Delete relay by ID |
| `list_relays` | (none) | List all active relays |

### Multi-Hop Relay Chain (New)
| Action | Params | Description |
|--------|--------|-------------|
| `create_relay` | `hop_N_listen_host`, `hop_N_listen_port`, `hop_N_forward_host`, `hop_N_forward_port`, `hop_N_protocol` | Create relay chain with N hops |
| `delete_relay` | `relay_id` | Delete relay (or chain) by ID |
| `list_relays` | (none) | List all active relays/relay chains |

**Example multi-hop chain:** `hop_0_listen_host=127.0.0.1`, `hop_0_listen_port=6666`, `hop_0_protocol=tcp`, `hop_1_listen_host=127.0.0.1`, `hop_1_listen_port=7777`, `hop_1_protocol=udp` creates a TCP→UDP relay chain.

## Protobuf Code Generation

`build.rs` compiles `proto/wraith.proto` → `src/proto/wraith.rs` using `prost-build`. Regenerate with:
```bash
cargo build
```

## Testing

```bash
cargo test
```

## Key Files

- `proto/wraith.proto` - Protocol buffer message definitions (includes `RelayHop` for multi-hop chains)
- `src/main.rs` - Rust entry point with clap CLI parsing
- `src/wraith/wraith.rs` - Core Wraith struct and main run loop
- `src/relay/mod.rs` - All relay implementations (TCP/UDP, session-based UDP, relay chains)
- `src/commands/relay.rs` - Command handler for relay operations (single-hop and chain)
- `home/PyWraith/client.py` - Python client class (includes `create_relay_chain` method)
- `home/PyWraith/protocol.py` - Python protobuf framing/encoding (includes `create_relay_chain_command`)
- `home/PyWraith/cli.py` - Python CLI implementation (supports `-t`/`-u` flag syntax for relay chains)
