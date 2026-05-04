# Wraith Protocol Validation Report

**Date:** 2026-05-01  
**Validated by:** rust-engineer (Wraith Protocol Expert)  
**Status:** ISSUES FOUND - Protocol Mismatch

---

## Executive Summary

The Wraith protocol has a **critical mismatch** between the Nexus implementation and the Wraith Rust dispatcher. The Nexus shell uses dedicated `RELAY_CREATE`, `RELAY_DELETE`, and `RELAY_LIST` payload message types, but the Wraith Rust dispatcher **only handles `Command.action` strings** and ignores the dedicated message types.

---

## Protocol Summary

### Message Framing
- ✅ **Compatible with Shelly**: `[4 bytes: length (big-endian u32)][N bytes: protobuf]`
- Confirmed in `wraith/src/connection/tcp.rs` and `PyWraith/protocol.py`

### Message Types (from proto/wraith.proto)

| Type | Value | Direction | Description |
|------|-------|-----------|-------------|
| REGISTRATION | 0 | Wraith→Nexus | Initial agent registration |
| HEARTBEAT | 1 | Both | Periodic status |
| COMMAND | 2 | Nexus→Wraith | Execute action with params |
| COMMAND_RESULT | 3 | Wraith→Nexus | Response with status/output |
| RELAY_CREATE | 4 | Both | Create relay (dedicated payload) |
| RELAY_DELETE | 5 | Both | Delete relay |
| RELAY_LIST | 6 | Both | Request relay list |
| RELAY_LIST_RESPONSE | 7 | Wraith→Nexus | Response with relay data |
| WRAITH_REGISTRATION | 8 | Wraith→Wraith | Peer registration |
| PEER_UPDATE | 9 | Wraith→Nexus | Peer connect/disconnect |
| PEER_LIST | 10 | Both | Request peer list |
| PEER_LIST_RESPONSE | 11 | Wraith→Nexus | Response with peers |

---

## Critical Issues Found

### Issue #1: Wraith Rust Dispatcher Only Handles Command.action (HIGH SEVERITY)

**Location:** `wraith/src/wraith/dispatcher.rs` lines 106-143

**Problem:** The `MessageDispatcher::dispatch()` method only handles `MessageType::Command` (value 2). Dedicated relay message types (RELAY_CREATE=4, RELAY_DELETE=5, RELAY_LIST=6, PEER_LIST=10) are defined in the protobuf but **never routed**.

```rust
pub async fn dispatch(&self, msg: WraithMessage, state: Arc<Mutex<WraithState>>) -> Option<WraithMessage> {
    let msg_type = msg.msg_type;

    if msg_type == MessageType::Command as i32 {
        // Only handles Command - relay payload types NOT handled!
        if let Some(crate::proto::wraith::wraith_message::Payload::Command(cmd)) = &msg.payload {
            // ... routes by cmd.action
        }
    }
    // MISSING: Handling for RelayCreate, RelayDelete, RelayList, PeerList
    None
}
```

**Impact:** Commands sent using dedicated payload types will be silently ignored.

---

### Issue #2: Nexus WraithTCPShell Uses Wrong Message Types (HIGH SEVERITY)

**Location:** `nexus/shells/wraith_tcp/comms.py`

**Problem:** The Nexus implementation sends relay commands using dedicated message types instead of Command.action strings:

| Method | Uses | Wraith Expects |
|--------|------|----------------|
| `_send_create_relay()` | `MessageType.RELAY_CREATE` payload | `Command.action="create_relay"` with params |
| `_send_delete_relay()` | `MessageType.RELAY_DELETE` payload | `Command.action="delete_relay"` with params |
| `_send_relay_list()` | `MessageType.RELAY_LIST` payload | `Command.action="list_relays"` with empty params |
| `_send_peer_list()` | `MessageType.PEER_LIST` payload | Not in dispatcher at all |

**Code Evidence** (comms.py lines 568-608):
```python
async def _send_create_relay(self, command: CommandMessage) -> bool:
    msg = WraithProtocol.create_relay_create(...)  # Uses RELAY_CREATE payload type!
    # This will NOT be handled by Wraith's dispatcher
```

**Expected Implementation** (should use Command.action):
```python
async def _send_create_relay(self, command: CommandMessage) -> bool:
    msg = WraithProtocol.create_command(
        command_id=command.command_id,
        action="create_relay",
        params={"listen_host": "...", "listen_port": "...", ...},
        timeout=...
    )
```

---

### Issue #3: set_id Parameter Name Mismatch (LOW SEVERITY)

**Location:** `nexus/shells/wraith_tcp/comms.py` line 614

**Problem:** Nexus sends `new_id` as the parameter name, but Wraith expects `wraith_id`.

```python
# Nexus sends (line 614):
str_params = {"wraith_id": new_id}  # correct parameter name

# But in _send_generic_command (line 689-691):
str_params = {k: str(v) if v is not None else "" for k, v in command.params.items()}
# This uses the original key from command.params
```

The `set_id` handler expects `wraith_id` (from `agent.rs` line 19):
```rust
let new_id = cmd.params.get("wraith_id").cloned().unwrap_or_default();
```

**Impact:** Medium - The parameter is correctly named in the CommandSpec definition.

---

## Command Parameter Mapping

### create_relay
| Param | Required | Default | Wraith Handler |
|-------|----------|---------|----------------|
| listen_host | Yes | - | `cmd.params.get("listen_host")` |
| listen_port | Yes | - | parsed as u16 |
| listen_proto | No | "tcp" | `cmd.params.get("listen_protocol")` |
| forward_host | Yes | - | `cmd.params.get("forward_host")` |
| forward_port | Yes | - | parsed as u16 |
| forward_proto | No | "tcp" | `cmd.params.get("forward_protocol")` |
| target_wraith_id | No | "" | Routes to peer if set |

### delete_relay
| Param | Required | Wraith Handler |
|-------|----------|----------------|
| relay_id | Yes | `cmd.params.get("relay_id")` |

### list_relays
| Param | Required | Wraith Handler |
|-------|----------|----------------|
| (none) | - | Returns JSON array |

### set_id
| Param | Required | Wraith Handler |
|-------|----------|----------------|
| wraith_id | Yes | `cmd.params.get("wraith_id")` |

### list_peers
| Param | Required | Wraith Handler |
|-------|----------|----------------|
| (none) | - | Returns JSON with wraith_id and peers array |

### wraith_listen
| Param | Required | Default | Wraith Handler |
|-------|----------|---------|----------------|
| host | No | "0.0.0.0" | `cmd.params.get("host")` |
| port | No | 4445 | parsed as u16 |

### wraith_connect
| Param | Required | Default | Wraith Handler |
|-------|----------|---------|----------------|
| host | Yes | - | `cmd.params.get("host")` |
| port | No | 4445 | parsed as u16 |

---

## Action Strings Handled by Wraith Dispatcher

✅ `create_relay` - handled via `RelayCommands::handle_create_relay()`  
✅ `delete_relay` - handled via `RelayCommands::handle_delete_relay()`  
✅ `list_relays` - handled via `RelayCommands::handle_list_relays()`  
✅ `set_id` - handled via `AgentCommands::handle_set_id()`  
✅ `list_peers` - handled via `AgentCommands::handle_list_peers()`  
✅ `wraith_listen` - handled via `AgentCommands::handle_wraith_listen()`  
✅ `wraith_connect` - handled via `AgentCommands::handle_wraith_connect()`  

❌ `RELAY_CREATE` payload type - NOT handled  
❌ `RELAY_DELETE` payload type - NOT handled  
❌ `RELAY_LIST` payload type - NOT handled  
❌ `PEER_LIST` payload type - NOT handled  
❌ `WRAITH_REGISTRATION` - NOT handled  
❌ `PEER_UPDATE` - NOT handled  

---

## Recommended Fixes

### Option A: Fix Nexus Implementation (Recommended for MVP)

Update `nexus/shells/wraith_tcp/comms.py` to use `Command.action` instead of dedicated payload types:

```python
async def _send_create_relay(self, command: CommandMessage) -> bool:
    """Send create_relay as Command.action instead of RELAY_CREATE payload."""
    params = command.params or {}
    str_params = {
        "listen_host": params.get("listen_host", "0.0.0.0"),
        "listen_port": str(params.get("listen_port", 0) or 0),
        "listen_protocol": params.get("listen_proto", "tcp"),
        "forward_host": params.get("forward_host", ""),
        "forward_port": str(params.get("forward_port", 0) or 0),
        "forward_protocol": params.get("forward_proto", "tcp"),
    }
    if params.get("target_wraith_id"):
        str_params["target_wraith_id"] = params["target_wraith_id"]

    msg = WraithProtocol.create_command(
        command_id=command.command_id,
        action="create_relay",
        params=str_params,
        timeout=int(command.timeout) if command.timeout else 30,
    )
    framed = WraithProtocol.encode_message(msg)
    return await self.send(framed)
```

Apply same pattern to `_send_delete_relay()`, `_send_relay_list()`, and `_send_peer_list()`.

### Option B: Fix Wraith Dispatcher (For Full Protocol Support)

Add handling in `wraith/src/wraith/dispatcher.rs` for dedicated relay messages:

```rust
pub async fn dispatch(&self, msg: WraithMessage, state: Arc<Mutex<WraithState>>) -> Option<WraithMessage> {
    let msg_type = msg.msg_type;

    // Handle Command messages (action-based)
    if msg_type == MessageType::Command as i32 {
        // ... existing logic
    }

    // Handle RELAY_CREATE (payload type)
    if msg_type == MessageType::RelayCreate as i32 {
        if let Some(wraith_message::Payload::RelayCreate(rc)) = &msg.payload {
            // Convert to Command.action and route
            let mut params = std::collections::HashMap::new();
            if let Some(listen) = &rc.config.listen {
                params.insert("listen_host".to_string(), listen.host.clone());
                params.insert("listen_port".to_string(), listen.port.to_string());
                params.insert("listen_protocol".to_string(), listen.protocol.clone());
            }
            if let Some(forward) = &rc.config.forward {
                params.insert("forward_host".to_string(), forward.host.clone());
                params.insert("forward_port".to_string(), forward.port.to_string());
                params.insert("forward_protocol".to_string(), forward.protocol.clone());
            }
            // Create Command and dispatch
            // ... route via relay_commands
        }
    }

    // Handle RELAY_DELETE, RELAY_LIST, PEER_LIST similarly
    // ...
}
```

---

## Files Reviewed

| File | Purpose | Status |
|------|---------|--------|
| `wraith/proto/wraith.proto` | Protobuf definitions | ✅ OK |
| `wraith/src/wraith/wraith.rs` | Core Wraith struct | ✅ OK |
| `wraith/src/wraith/dispatcher.rs` | Message dispatcher | ❌ Missing payload handlers |
| `wraith/src/commands/relay.rs` | Relay command handlers | ✅ OK |
| `wraith/src/commands/agent.rs` | Agent command handlers | ✅ OK |
| `wraith/src/message/codec.rs` | Message encoding | ✅ OK |
| `wraith/src/connection/tcp.rs` | TCP framing | ✅ OK |
| `nexus/shells/wraith_tcp/comms.py` | Nexus Wraith shell | ❌ Uses wrong message types |
| `nexus/shells/shelly_tcp/comms.py` | Reference implementation | ✅ Correct pattern |

---

## Validation Checklist

- [x] Protocol framing matches Shelly (4-byte big-endian + protobuf)
- [x] Message types defined correctly in proto
- [x] Command.action strings mapped correctly
- [x] Registration parsing → session creation (Nexus: ✅, Wraith: ✅)
- [x] CommandResult handling → task output (Nexus: ✅, Wraith: ✅)
- [ ] Relay commands using Command.action (Nexus: ❌, Wraith: ✅)
- [ ] Peer network messages handled (Nexus: partial, Wraith: partial)
- [ ] target_wraith_id routing supported (Wraith: ✅, Nexus: not tested)

---

## Next Steps

1. **Fix Nexus implementation** to use `Command.action` for relay operations (Option A recommended)
2. **Test the fix** by running integration tests
3. **Consider Option B** for full protocol support (dedicated relay messages)
4. **Document** the corrected behavior in SPEC.md

---

*End of Report*