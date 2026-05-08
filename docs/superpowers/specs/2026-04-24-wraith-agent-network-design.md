# Stage 2 Design: Wraith-to-Wraith Agent Network

## Overview

Enable Wraith agents to connect to each other forming a chain: `C2 → Wraith A → Wraith B`. C2 can send commands to any wraith in the chain, and relays can span multiple hops.

---

## 1. Network Topology & Connections

### Topology Model
- Each wraith maintains one connection to C2 (existing behavior)
- Each wraith can maintain zero or more connections to peer wraiths
- **Full mesh capable** — a wraith can connect to multiple peers
- Peers connect via a new `agent listen/connect` mechanism

### Peer Table
Each wraith maintains a peer table mapping wraith IDs to their Yamux connections:
```
{wraith_id: "B", connection: YamuxConnection}
```

### New CLI Modes
```
./wraith agent listen <host>:<port>   # Listen for peer wraith connections
./wraith agent connect <host>:<port> # Connect to a peer wraith
```

---

## 2. Wraith Identification

### ID Assignment
- **Auto-generated UUID** on startup (default temporary ID)
- **`set_id <new_id>`** command changes wraith's ID at runtime

### WraithRegistration Message
Sent by a wraith when connecting to a peer over Yamux Stream 0:
```protobuf
message WraithRegistration {
    string wraith_id = 1;
    string hostname = 2;
    string os = 3;
    int64 connected_at = 4;
}
```

### PeerUpdate Message
Broadcast to C2 when peers connect/disconnect:
```protobuf
message PeerUpdate {
    string wraith_id = 1;
    string hostname = 2;
    string action = 3;  // "connected" or "disconnected"
    int64 timestamp = 4;
}
```

---

## 3. Agent Connection Protocol

### Connection Flow
1. Wraith B opens a Yamux connection to Wraith A
2. B sends `WraithRegistration` on Stream 0 identifying itself
3. A adds B to its peer table: `{wraith_id: "B", connection: yamux_connection}`
4. A broadcasts `PeerUpdate` to C2 with action "connected"

### Disconnection
- When a peer disconnects, its entry is removed from the peer table
- A broadcasts `PeerUpdate` to C2 with action "disconnected"

---

## 4. Command Routing

### Command Format
Commands from C2 include `target_wraith_id` in params:
```protobuf
Command {
    string command_id = 1;
    string action = 2;
    map<string, string> params = 3;  // includes "target_wraith_id"
    int32 timeout = 4;
}
```

### Routing Algorithm
When a wraith receives a command:
1. **Local match**: If `target_wraith_id` matches local wraith_id, execute locally
2. **Direct peer**: If target is in peer table, forward on that peer's Yamux Stream 0
3. **Broadcast**: Otherwise broadcast to all peers except the source
   - Uses `message_id` tracking for loop prevention

### Loop Prevention
- Each command has a unique `message_id`
- Wraiths track seen `message_id`s and don't re-broadcast if already handled

---

## 5. Yamux Stream Management

### Stream Allocation
- **Stream 0**: Command/control traffic (bidirectional)
- **Stream 1+**: Relay data (one stream per active relay)

### Command Stream
- All commands (routed or direct) travel over Stream 0
- Stream 0 is always open for the lifetime of the peer connection

### Relay Streams
- Each active relay gets a dedicated Yamux stream
- Stream identified by `relay_id` in the relay message header
- Streams are opened/closed as relays are created/deleted

---

## 6. Relay Through Chain

### Create Relay Flow
When C2 sends `create_relay` to Wraith A with `target_wraith_id: B`:

1. A receives command, looks up B in peer table
2. A forwards command to B over Yamux Stream 0
3. B creates the actual relay listening socket on B's network
4. B opens a new Yamux stream back through A for relay data
5. A bridges: incoming connections on A:listen_port → Yamux stream → B → forward_host:forward_port

### Example Scenario
```
Client connects to A:4444
       |
       v
Wraith A (127.0.0.1:4444) <-- Yamux tunnel --> Wraith B (forwards to 8.8.8.8:8888)
                                                        |
                                                        v
                                                  Real target (8.8.8.8:8888)
```

### Command Format
```protobuf
Command {
    action: "create_relay"
    params: {
        "target_wraith_id": "B",
        "listen_host": "127.0.0.1",
        "listen_port": "4444",
        "forward_host": "8.8.8.8",
        "forward_port": "8888"
    }
}
```

### Return Path
Responses flow back on the same Yamux streams, reversed (B→A→C2).

---

## 7. List Peers Command

### Command
```
Command {
    action: "list_peers"
    params: {}
}
```

### Response
Returns direct neighbors only (recursive traversal handled by PyWraith client):
```protobuf
message PeerListResponse {
    string wraith_id = 1;
    repeated PeerInfo peers = 2;
}

message PeerInfo {
    string wraith_id = 1;
    string hostname = 2;
    bool connected = 3;
}
```

---

## 8. Full Command List

| Action | Description |
|--------|-------------|
| `set_id` | Set wraith's ID at runtime |
| `create_relay` | Create relay, optionally targeting remote wraith via `target_wraith_id` |
| `delete_relay` | Delete relay by ID |
| `list_relays` | List all active relays |
| `list_peers` | List direct neighbor wraiths |

---

## 9. Component Changes

### Rust (Wraith Agent)
- `src/connection/` — Add Yamux-based connection handler for peer connections
- `src/tunnel/` — New module for peer-to-peer tunnel management
- `src/commands/` — Add `set_id`, `list_peers` handlers
- `src/relay/` — Extend to support remote relay creation via peer

### Python (PyWraith)
- `client.py` — Add methods for peer management, recursive list_peers traversal
- `protocol.py` — Add `WraithRegistration`, `PeerUpdate`, `PeerListResponse` message types
- `cli.py` — Add `agent listen`, `agent connect` commands

### Protocol
- Add `WraithRegistration`, `PeerUpdate`, `PeerListResponse` messages to `wraith.proto`

---

## 10. Error Handling

- **Peer disconnection**: Remove from peer table, notify C2 via PeerUpdate
- **Routing failure**: If target wraith not reachable, return error to C2
- **Relay failure**: Return error in CommandResult with details
- **Yamux stream error**: Close stream, attempt reconnect if peer still connected
