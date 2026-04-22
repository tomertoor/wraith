# PyWraith

Python client for wraith pentesting tunnel tool.

## Installation

```bash
./setup.sh
```

## Usage

```bash
pywraith --host 127.0.0.1 --port 4444
```

## Commands

- `create_relay <listen_host> <listen_port> <forward_host> <forward_port>` - Create a relay
- `delete_relay <relay_id>` - Delete a relay
- `list_relays` - List all relays