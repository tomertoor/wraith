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
│         │     └── relay.rs → relay/tcp.rs, relay/udp.rs          │
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
| `src/relay/` | Relay implementations (TCP/UDP), `RelayManager` owns active relays |

### Protobuf Definition
`proto/wraith.proto` defines `WraithMessage` with `oneof payload` containing:
- `Registration` - hostname, username, os, ip
- `Command` / `CommandResult` - request/response for actions
- `RelayCreate` / `RelayDelete` / `RelayList` / `RelayListResponse` - relay management

### Message Flow
1. PyWraith sends `WraithMessage` with `Command` payload
2. Wraith's `connection.read_message()` reads framed data
3. `Wraith::run()` dispatches to `MessageDispatcher`
4. `CommandHandler` routes by `action` field to appropriate handler
5. Response sent back via same connection

## Commands

The wraith agent supports these actions via `Command.action`:

| Action | Params | Description |
|--------|--------|-------------|
| `create_relay` | `listen_host`, `listen_port`, `forward_host`, `forward_port`, `protocol` | Create TCP/UDP relay |
| `delete_relay` | `relay_id` | Delete relay by ID |
| `list_relays` | (none) | List all active relays |
| `wraith_listen` | - | Stage 2: Listen for another wraith |
| `wraith_connect` | - | Stage 2: Connect to another wraith |

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

- `proto/wraith.proto` - Protocol buffer message definitions
- `src/main.rs` - Rust entry point with clap CLI parsing
- `src/wraith/wraith.rs` - Core Wraith struct and main run loop
- `home/PyWraith/client.py` - Python client class
- `home/PyWraith/protocol.py` - Python protobuf framing/enoding
- `home/PyWraith/cli.py` - Python CLI implementation
