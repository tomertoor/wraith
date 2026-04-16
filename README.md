# Wraith

Modular reverse tunneling tool for penetration testing.

## Overview

Wraith enables **relay chains** through a network, allowing you to access services that are only reachable through intermediate hosts.

```
Attacker (A) ← B ← C (target network)
```

- **A**: Connected to C2 (Nexus), listens for incoming relay connections
- **B**: Connects to A's relay, can listen for downstream agents
- **C**: Connects to B's relay, can access target networks

## Installation

```bash
cargo build --release
```

## Usage

### Startup Modes

Wraith agents start with **only one connection** — either to a C2 server OR to an upstream relay:

```bash
# Connect to C2 server only
wraith agent --c2 nexus:8080

# Connect to an upstream relay only
wraith agent --tunnel A:8080
```

### Dynamic Relay Creation

Once running, relays are created dynamically via C2 commands:

```bash
# C2 sends AddRelay to A: listen on 8080, forward to target via chain
{ action: "add_relay", params: { listen_addr: "0.0.0.0:8080", connect_addr: "target:80", relay_id: "relay1" } }

# C2 sends AddRelay to B: listen on 9090 (for C to connect to)
{ action: "add_relay", params: { listen_addr: "0.0.0.0:9090", relay_id: "relay2" } }
```

### Example: A → B → C Chain

**1. Start A (connected to C2):**
```bash
wraith agent --c2 nexus:8080
```

**2. Start B (connected to A's relay):**
```bash
# A needs to be listening on its relay port first (default 4446)
wraith agent --tunnel A:4446
```

**3. C2 tells B to listen for C:**
```bash
# C2 → B: AddRelay { listen_addr: "0.0.0.0:9090" }
```

**4. Start C (connected to B's relay):**
```bash
wraith agent --tunnel B:9090
```

**5. C2 tells A to create relay to target:**
```bash
# C2 → A: AddRelay { listen_addr: "0.0.0.0:8080", connect_addr: "target:80" }
```

**6. Access target:**
```bash
curl http:// A:8080  # traffic flows through chain to target
```

## Architecture

### Connection Model

The agent maintains two independent connections:
1. **C2 connection** (TCP to Nexus): sends `WraithRegistration` on connect, then reads `NexusCommand` messages and returns `WraithCommandResult`. Uses 30s heartbeat interval.
2. **Relay connection** (TCP to wraith relay): Yamux-muxed with channels 0 (tunnel data), 3 (relay data), 5 (relay control).

### Wire Format

- **C2/Nexus**: `[4-byte u32 BE length][protobuf bytes]`
- **Relay Yamux streams**: `[4-byte channel_id][4-byte length][protobuf bytes]`

### Channel IDs

| Channel | Name | Purpose |
|---------|------|---------|
| 0 | TUNNEL_DATA | IP packet forwarding (TUN mode) |
| 1 | TUNNEL_CONTROL | Tunnel open/close/keepalive |
| 3 | RELAY_DATA | TCP relay data (port forwarding) |
| 5 | RELAY_CONTROL | Relay open/close commands |

## Modes

- `wraith agent` — Agent mode: connects to C2 and/or relay
- `wraith relay` — Relay server mode (legacy TUN-based)
- `wraith socat` — Socat-style relay modes (tcp, udp, listener)

## Building

```bash
cargo build --release        # Build
cargo test                   # Run tests
```
