# Wraith Protocol Fix for Nexus Implementation

## Problem Summary

The Nexus `wraith_tcp/comms.py` implementation sends relay commands using dedicated protobuf payload types (`RELAY_CREATE`, `RELAY_DELETE`, `RELAY_LIST`, `PEER_LIST`), but the Wraith Rust dispatcher **only handles `Command.action` strings**. This causes all relay operations to fail silently.

## Root Cause

From `wraith/src/wraith/dispatcher.rs` lines 106-128:

```rust
pub async fn dispatch(&self, msg: WraithMessage, state: Arc<Mutex<WraithState>>) -> Option<WraithMessage> {
    let msg_type = msg.msg_type;

    if msg_type == MessageType::Command as i32 {
        // ONLY handles Command - ignores RELAY_CREATE/Delete/List payloads
        if let Some(wraith_message::Payload::Command(cmd)) = &msg.payload {
            // Routes by cmd.action: create_relay, delete_relay, list_relays, etc.
        }
    }
    // No handling for RELAY_CREATE(4), RELAY_DELETE(5), RELAY_LIST(6), PEER_LIST(10)
    None
}
```

The `MessageType` enum defines relay payload types, but the dispatcher never routes them.

## Correct Approach

Per SPEC.md section 1.1, relay commands should use `COMMAND` with action strings:

```python
# CORRECT: Use Command.action with params
msg = WraithProtocol.create_command(
    command_id="...",
    action="create_relay",  # Action string, not message type
    params={
        "listen_host": "0.0.0.0",
        "listen_port": "9001",
        "listen_protocol": "tcp",
        "forward_host": "192.168.1.1",
        "forward_port": "22",
        "forward_protocol": "tcp",
    },
    timeout=30,
)
```

## Current Incorrect Implementation

`nexus/shells/wraith_tcp/comms.py` lines 568-608 use dedicated payload types:

```python
# INCORRECT - Wraith dispatcher ignores this
async def _send_create_relay(self, command: CommandMessage) -> bool:
    msg = WraithProtocol.create_relay_create(...)  # Uses RELAY_CREATE payload
```

## Fix Required

Replace all `_send_*` methods to use `Command.action` instead of dedicated payload types:

### `_send_create_relay` Fix

```python
async def _send_create_relay(self, command: CommandMessage) -> bool:
    """Send create_relay as Command.action (not RELAY_CREATE payload)."""
    params = command.params or {}

    # Build params dict (Wraith expects string values in map<string,string>)
    str_params = {
        "listen_host": str(params.get("listen_host", "0.0.0.0")),
        "listen_port": str(params.get("listen_port", 0) or 0),
        "listen_protocol": str(params.get("listen_proto", "tcp")),
        "forward_host": str(params.get("forward_host", "")),
        "forward_port": str(params.get("forward_port", 0) or 0),
        "forward_protocol": str(params.get("forward_proto", "tcp")),
    }

    # Handle optional target_wraith_id for remote relay creation
    if params.get("target_wraith_id"):
        str_params["target_wraith_id"] = str(params["target_wraith_id"])

    # Handle multi-hop format
    i = 0
    while True:
        hop_prefix = f"hop_{i}_"
        if f"{hop_prefix}listen_host" in params:
            str_params[f"{hop_prefix}listen_host"] = str(params[f"{hop_prefix}listen_host"])
            str_params[f"{hop_prefix}listen_port"] = str(params[f"{hop_prefix}listen_port"] or 0)
            str_params[f"{hop_prefix}listen_protocol"] = str(params.get(f"{hop_prefix}listen_proto", "tcp"))
            str_params[f"{hop_prefix}forward_host"] = str(params[f"{hop_prefix}forward_host"])
            str_params[f"{hop_prefix}forward_port"] = str(params[f"{hop_prefix}forward_port"] or 0)
            str_params[f"{hop_prefix}forward_protocol"] = str(params.get(f"{hop_prefix}forward_proto", "tcp"))
            i += 1
        else:
            break

    msg = WraithProtocol.create_command(
        command_id=command.command_id,
        action="create_relay",
        params=str_params,
        timeout=int(command.timeout) if command.timeout else 30,
        target_wraith_id=str(params.get("target_wraith_id", "")),
    )

    framed = WraithProtocol.encode_message(msg)
    return await self.send(framed)
```

### `_send_delete_relay` Fix

```python
async def _send_delete_relay(self, command: CommandMessage) -> bool:
    """Send delete_relay as Command.action (not RELAY_DELETE payload)."""
    params = command.params or {}

    str_params = {
        "relay_id": str(params.get("relay_id", "")),
    }

    msg = WraithProtocol.create_command(
        command_id=command.command_id,
        action="delete_relay",
        params=str_params,
        timeout=int(command.timeout) if command.timeout else 30,
        target_wraith_id=str(params.get("target_wraith_id", "")),
    )

    framed = WraithProtocol.encode_message(msg)
    return await self.send(framed)
```

### `_send_relay_list` Fix

```python
async def _send_relay_list(self, command: CommandMessage) -> bool:
    """Send list_relays as Command.action (not RELAY_LIST payload)."""
    params = command.params or {}

    # list_relays takes no params, but target_wraith_id can be set
    msg = WraithProtocol.create_command(
        command_id=command.command_id,
        action="list_relays",
        params={},
        timeout=int(command.timeout) if command.timeout else 30,
        target_wraith_id=str(params.get("target_wraith_id", "")),
    )

    framed = WraithProtocol.encode_message(msg)
    return await self.send(framed)
```

### `_send_peer_list` Fix

```python
async def _send_peer_list(self, command: CommandMessage) -> bool:
    """Send list_peers as Command.action (not PEER_LIST payload)."""
    params = command.params or {}

    msg = WraithProtocol.create_command(
        command_id=command.command_id,
        action="list_peers",
        params={},
        timeout=int(command.timeout) if command.timeout else 30,
        target_wraith_id=str(params.get("target_wraith_id", "")),
    )

    framed = WraithProtocol.encode_message(msg)
    return await self.send(framed)
```

## Keep These Methods (Already Correct)

The following methods already use `Command.action` correctly:
- ✅ `_send_set_id` - uses `create_command(action="set_id", params={"wraith_id": new_id})`
- ✅ `_send_wraith_listen` - uses `create_command(action="wraith_listen", ...)`
- ✅ `_send_wraith_connect` - uses `create_command(action="wraith_connect", ...)`
- ✅ `_send_generic_command` - uses `create_command(action=..., params=...)`

## Why The Dedicated Payload Types Exist

The `RELAY_CREATE`, `RELAY_DELETE`, `RELAY_LIST` payload types are defined in the protobuf schema but **not implemented in the dispatcher**. They appear to be:
1. Reserved for future expansion
2. Potentially used by other clients (PyWraith's `create_relay_command()` uses them)
3. A protocol design that was never fully implemented in the Rust side

**Note:** PyWraith's `protocol.py` has `create_relay_command()` that uses the dedicated payload type. However, the Wraith Rust agent only handles `Command.action` strings. This is an inconsistency in the protocol design, but for Nexus integration, we must follow the Wraith dispatcher's behavior.

## Verification

After applying the fix, test with:
```bash
# Start Wraith in listen mode
cargo run -- --c2-host 0.0.0.0 --c2-port 4445 --wraith-id test-wraith --listen

# Start Nexus and connect a Wraith agent
# Then issue create_relay command via API
curl -X POST http://localhost:8080/api/sessions/{id}/tasks \
  -H "Content-Type: application/json" \
  -d '{"action": "create_relay", "params": {"listen_port": 9001, "forward_host": "192.168.1.1", "forward_port": 22}}'
```

Expected: Wraith logs "Created relay with id: ..." and returns relay_id in response.