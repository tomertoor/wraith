# Wraith Naming & Interface Simplification — Design

## Status: Approved

---

## Concept Definitions

Three distinct things, all clearly named:

### relay
A port-forwarding pipe (TCP or UDP). The `src/relay/` module (formerly `src/socat/`). Like `socat` — listens on a local port and bridges traffic to a target through the wraith tunnel. Proto messages: `RelayOpen`, `RelayControl`, etc. (wire format, unchanged).

- Channels 3 (FORWARD_DATA) and 5 (FORWARD_CONTROL)
- No TUN device — pure socket-to-socket bridging

### tunnel
An IP packet conduit via a TUN network interface. Raw IP packets flow through a virtual `tun` device. One tunnel = one VPN connection between agent and tunnel server.

- Channels 0 (TUNNEL_DATA) and 1 (TUNNEL_CONTROL)
- TUN device created and owned by the tunnel server side

### tserver (tunnel server)
The server binary that accepts agent connections, creates TUN interfaces, and routes tunnel IP packets. The `src/tserver/` module (formerly `src/relay/`).

---

## CLI Changes

### Before
```
wraith c2 <addr>        Connect to Nexus C2 server
wraith listen [addr]   Listen for peer connections
wraith connect <addr>  Connect to a peer
wraith direct           JSONL stdin/stdout (PyWraith)
```

### After
```
wraith agent <c2_addr>              Run as agent, connect to C2 (PyWraith)
wraith listen [addr]                 Listen for peer connections (unchanged)
wraith connect <addr>                Connect to a peer (unchanged)
```

- `c2` → `agent` (C2 is jargon)
- `direct` mode removed (replaced by agent → PyWraith TCP)
- `wraith relay` binary unchanged (it's already called "relay")

---

## Module Renames

### `src/socat/` → `src/relay/`
`socat` was just a name for port forwarding — it IS "relay". Just rename the directory and update imports.

Contents: `SocatRelay` struct, `Relay` struct, `run_tcp_relay`, `run_udp_relay`, `run_listener_relay` — these keep proto-compatible names since they use `relay_id` in wire format.

### `src/relay/` (tunnel server) → `src/tserver/`
The module that manages TUN tunnels should not be called "relay". Renamed to `tserver`.

Contents: `RelayServer` → `TunnelServer`, `RelayHandler` → `TunnelHandler`, `RelayConnection` → `TunnelConnection`.

---

## Channel ID Renames

| Current | Proposed | Value |
|---|---|---|
| `RELAY_DATA` | `FORWARD_DATA` | 3 |
| `RELAY_CONTROL` | `FORWARD_CONTROL` | 5 |
| `TUNNEL_DATA` | `TUNNEL_DATA` (keep) | 0 |
| `TUNNEL_CONTROL` | `TUNNEL_CONTROL` (keep) | 1 |

---

## Config / Field Renames

| Current | Proposed | Reason |
|---|---|---|
| `relay_host` | `tunnel_host` | Host for tunnel server connection |
| `relay_port` | `tunnel_port` | Port for tunnel server connection |
| `Cli::C2` | `Cli::Agent` | C2 is jargon |

---

## Proto Wire Format — NO CHANGES

`proto/tunnel.proto` message names unchanged:
- `RelayOpen`, `RelayControl`, `RelayData` — wire format must be stable
- Internal Rust code uses local variable names to clarify context

---

## Files to Change

### `src/socat/` → `src/relay/`
- Rename directory `socat` → `relay`
- Flatten `relay.rs` into `mod.rs`
- `SocatRelay` stays (proto-compatible), struct `Relay` stays (proto-compatible)
- Function names (`run_tcp_relay`, etc.) stay (proto-compatible)

### `src/relay/` (tserver) → `src/tserver/`
- Rename directory
- `RelayServer` → `TunnelServer`
- `RelayHandler` → `TunnelHandler`
- `RelayConnection` → `TunnelConnection`
- `relay_id` field in proto structs stays (wire format)

### `src/tunnel/channel.rs`
- `RELAY_DATA` → `FORWARD_DATA`
- `RELAY_CONTROL` → `FORWARD_CONTROL`

### `src/peer/mod.rs`
- `RelayState` → `ForwardState`
- `relays` HashMap → `forwards`
- `open_relay` → `open_forward`
- `close_relay` → `close_forward`
- `list_relays` → `list_forwards`
- Update imports: `SocatRelay` → `PortForward` (alias or direct use)

### `src/agent/client.rs`
- `relay_conn` → `tunnel_conn`
- `relay_port` → `tunnel_port`
- `RelayStatsData` → `ForwardStats`
- `handle_relay_connection` → `handle_forward_connection`

### `src/agent/commands.rs`
- `socat` field → `relay` field (keep name since it IS relay)
- `start_relay` → `start_forward` (method name only)
- `stop_relay` → `stop_forward`
- `list_relays` → `list_forwards`

### `src/main.rs`
- `Cli::C2` → `Cli::Agent`
- `run_c2` → `run_agent`
- Remove `Cli::Direct` and `direct::run_direct`
- Remove `mod direct`
- Add `wraith forward` subcommand

### `src/wraith/config.rs` + `embedded_config.rs`
- `relay_host` → `tunnel_host`
- `relay_port` → `tunnel_port`

### `src/direct/` — REMOVE entirely

---

## Implementation Order

1. `src/socat/` → `src/relay/` (rename + flatten)
2. `src/relay/` → `src/tserver/` (rename + rename structs)
3. `src/tunnel/channel.rs` — RELAY_DATA → FORWARD_DATA, RELAY_CONTROL → FORWARD_CONTROL
4. `src/peer/mod.rs` — RelayState → ForwardState, method renames
5. `src/agent/client.rs` — field renames (relay_conn → tunnel_conn, etc.)
6. `src/agent/commands.rs` — method renames, update imports
7. `src/main.rs` — CLI renames, remove direct
8. `src/wraith/config.rs` + `embedded_config.rs` — field renames
9. Remove `src/direct/`
10. Update all cascading imports, verify build

---

## Success Criteria

1. `wraith agent localhost:9000` — immediately clear
2. `wraith relay` binary is clearly about port forwarding (relay)
3. `wraith tserver` binary is clearly about TUN tunnel management
4. No "socat" anywhere in the codebase
5. No "relay" used for the tunnel server module
6. FORWARD_DATA and FORWARD_CONTROL channels are clearly about port forwarding
