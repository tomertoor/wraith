# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Wraith is a modular reverse tunneling tool for penetration testing written in Rust. It supports three operation modes:

- `wraith agent` — Agent mode: connects to a Nexus C2 server and a relay server
- `wraith relay` — Relay server mode: listens for agents and creates a TUN interface for tunnel traffic
- `wraith socat` — Socat-style relay modes (tcp, udp, listener)

## Build & Run

```bash
cargo build --release        # Build
cargo test                   # Run tests
cargo run -- agent           # Run as agent
cargo run -- relay           # Run as relay
cargo run -- socat tcp       # Run socat in TCP mode
```

## Architecture

### Connection Model

The agent maintains two independent connections:
1. **C2 connection** (TCP to Nexus): sends `WraithRegistration` on connect, then reads `NexusCommand` messages and returns `WraithCommandResult`. Uses 30s heartbeat interval.
2. **Relay connection** (TCP to wraith relay): Yamux-muxed with channels 0 (tunnel data), 3 (relay data), 5 (relay control).

### Wire Format

- **C2/Nexus**: `[4-byte u32 BE length][protobuf bytes]`
- **Relay Yamux streams**: `[4-byte channel_id][4-byte length][protobuf bytes]`

Channel IDs are defined in `src/tunnel/channel.rs`:
- `TUNNEL_DATA = 0`, `TUNNEL_CONTROL = 1`, `RELAY_DATA = 3`, `RELAY_CONTROL = 5`

### Directory Structure

- `src/agent/` — Agent client: connects to C2 and relay, dispatches commands (execute/upload/download), manages tunnels
- `src/relay/` — Relay server: accepts agent connections via Yamux, manages tunnels, writes to TUN device (`relay/tun.rs`)
- `src/tunnel/` — Tunnel protocol: framing, multiplexing, channel IDs, encryption (chacha20)
- `src/transport/` — Transport layer (TCP only)
- `src/socat/` — Socat-style relay implementation
- `src/wraith/` — Configuration (`config.rs`)
- `src/proto/` — Generated protobuf code (from `proto/tunnel.proto`)
- `proto/tunnel.proto` — Protobuf message definitions

### Protobuf Compilation

Protobuf files are compiled at build time via `build.rs` using `prost-build`. The generated code goes to `src/proto/wraith.tunnel.rs`.

### Key Files

- `src/main.rs` — CLI entry point with clap, delegates to `run_agent`, `run_relay`, `run_socat`
- `src/agent/client.rs` — Core agent: dual-connection management, Yamux polling, command dispatch
- `src/relay/server.rs` — Relay server: accepts agents, handles tunnel open/close, TUN packet forwarding
- `src/tunnel/framing.rs` — `FramedReader`/`FramedWriter` for length-prefixed frames
- `src/tunnel/channel.rs` — Channel ID constants and helpers
- `src/tunnel/multiplex.rs` — `encode_frame` for Yamux stream framing

### Configuration

`src/wraith/config.rs` — `Config` struct loaded from embedded config or defaults. CLI args override via main.rs.

### TUN Device (relay mode)

Uses `tokio_tun` for TUN I/O. The relay opens a TUN interface (`wraith0` by default), assigns IP (`10.8.0.1/24`), and optionally adds routes. TUN fd is not `Send`, so TUN I/O runs in the main relay loop — tunnel packets are forwarded via an mpsc channel.

### Encryption

Supports chacha20poly1305 and aes-gcm (configurable at startup). See `src/tunnel/chacha20.rs`.

### Python Package (PyWraith)

`PyWraith/` — Python package for commanding the Wraith agent. Installed via `pip install pywraith`. Not part of the Rust build.