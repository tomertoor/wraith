# PyWraith Agent Routing Design

## Date
2026-04-25

## Overview

Enable operator control of remote wraiths through an intermediate wraith chain, without connecting the remote wraith directly to C2. The operator connects to the nearest wraith (Wraith A) and can issue commands targeting any wraith in the chain (Wraith C) via `set_target`.

Topology: `CLI → Wraith A → Wraith B → Wraith C`

## CLI Interface

### Global Target Context

`set_target <wraith_id>` — Sets a persistent routing context on the CLI. All subsequent commands are routed to this target wraith until changed.

```
%set_target wraith-c         ← routing context: wraith-c
%list_peers                  ← executed on wraith-c
%list_relays                 ← executed on wraith-c
%set_target wraith-b         ← switch
%list_peers                  ← now executed on wraith-b
```

`set_target` must be set before any command. No default — if no target is set, commands error.

`%set_target` alone prints current target. `set_target none` clears it.

### Relay Creation

`create_relay` takes a `--target` flag specifying the wraith on the *forward* side of the relay (the other endpoint). This is separate from the global `set_target`.

```
%set_target wraith-b        ← commands execute on wraith-b
%create_relay -l tcp -L 0.0.0.0 2222 -f tcp -F 127.0.0.1 3333 --target wraith-c
```

Result: relay runs on **wraith-b** (the execute target), listening on wraith-b, forward connecting to **wraith-c** (the --target). If wraith-b isn't directly connected to wraith-c, the forward connection is routed through the peer chain.

### Command Syntax Summary

| Command | Behavior |
|---------|----------|
| `%set_target <id>` | Set global routing context |
| `%set_target` | Print current target |
| `%set_target none` | Clear target |
| `%list_peers` | Execute on target wraith |
| `%list_relays` | Execute on target wraith |
| `%create_relay ... --target <id>` | Create relay on execute target, forward connects to `--target` wraith |

## Protocol

### Message Envelope

Add `target_wraith_id` to the `WraithMessage` envelope in `proto/wraith.proto`:

```protobuf
message WraithMessage {
    MsgType msg_type = 1;
    string message_id = 2;
    int64 timestamp = 3;
    string target_wraith_id = 4;  // NEW: routing target
    Command command = 10;
    CommandResult result = 11;
    // ... other fields
}
```

When `target_wraith_id` is set:
- If matches local wraith_id → execute locally
- If in peer table → forward via Yamux stream 0 to that peer
- Otherwise → broadcast to all peers (with message_id dedup to prevent loops)

When empty → execute locally (existing behavior).

## Command Routing

1. CLI sends `WraithMessage` with `target_wraith_id=wraith-c`, action=`create_relay`, params include relay config
2. Wraith A receives on its C2 connection, dispatcher reads `target_wraith_id`
3. Wraith A checks: am I wraith-c? No. Is wraith-c in my peer table? No.
4. Wraith A broadcasts to all peers (Yamux stream 0 to each peer) with same `target_wraith_id`
5. Wraith B receives, same checks — not local, not direct peer, broadcasts further
6. Wraith C receives, matches local wraith_id → executes command
7. Wraith C sends response back along reverse path: C → B → A → CLI

**Loop prevention:** Rust tracks `seen_message_ids` in state. If a message_id is already processed, it is dropped. Each command gets a UUID from the CLI.

## Relay Data Channel

When creating a relay where `--target` wraith isn't directly connected to the execute target:

1. Relay creation command arrives at execute target (e.g., wraith-b) via the routing above
2. Wraith-b attempts to open forward connection to `--target` wraith (e.g., wraith-c)
3. If not directly connected, wraith-b uses the same broadcast/flood mechanism to locate wraith-c
4. Once wraith-c is found, a data path is established and relay-forward connection is made through the chain
5. Relay operates normally once connected

## Rust-side Changes

| File | Change |
|------|--------|
| `proto/wraith.proto` | Add `target_wraith_id` to `WraithMessage` |
| `src/message/codec.rs` | Parse/serialize `target_wraith_id` |
| `src/wraith/dispatcher.rs` | Check `target_wraith_id`, route to peer or broadcast |
| `src/wraith/state.rs` | `seen_message_ids` already exists, reuse for dedup |

## Python-side Changes

| File | Change |
|------|--------|
| `client.py` | Add `set_target(id)`, `_target` field, inject `target_wraith_id` in every command |
| `cli.py` | Add `%set_target` magic, add `--target` flag to `do_create_relay` |
| `proto_gen/wraith_pb2.py` | Regenerate after proto change |

## Error Handling

| Scenario | Behavior |
|----------|----------|
| `set_target` not set | Error: "No target wraith set. Use %set_target <id>" |
| Target wraith unreachable | Timeout after CLI timeout. Error propagated back through chain. |
| Target wraith not in network | CLI timeout, chain intermediate wraiths may hold connection open |
| Relay `--target` unreachable | Relay creation fails with error describing forward-side failure |
| message_id loop | Dropped by intermediate wraiths via `seen_message_ids` |
