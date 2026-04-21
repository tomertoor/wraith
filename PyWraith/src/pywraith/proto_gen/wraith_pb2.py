"""Generated protobuf code for wraith protocol."""
from typing import Optional, Dict, Any

# Message types
REGISTRATION = 0
HEARTBEAT = 1
COMMAND = 2
COMMAND_RESULT = 3
RELAY_CREATE = 4
RELAY_DELETE = 5
RELAY_LIST = 6
RELAY_LIST_RESPONSE = 7


class WraithMessage:
    """Stub for WraithMessage protobuf message."""
    def __init__(self, type: int = 0, payload: bytes = b''):
        self.type = type
        self.payload = payload

    def SerializeToString(self) -> bytes:
        return bytes([self.type]) + self.payload

    @classmethod
    def ParseFromString(cls, data: bytes):
        msg = cls()
        if data:
            msg.type = data[0]
            msg.payload = data[1:]
        return msg


class Registration:
    """Stub for Registration protobuf message."""
    def __init__(self, agent_id: str = "", hostname: str = ""):
        self.agent_id = agent_id
        self.hostname = hostname

    def SerializeToString(self) -> bytes:
        return f"{self.agent_id}|{self.hostname}".encode()

    @classmethod
    def ParseFromString(cls, data: bytes):
        parts = data.decode().split('|')
        return cls(agent_id=parts[0] if len(parts) > 0 else "",
                   hostname=parts[1] if len(parts) > 1 else "")


class Heartbeat:
    """Stub for Heartbeat protobuf message."""
    def __init__(self, agent_id: str = "", status: int = 0):
        self.agent_id = agent_id
        self.status = status

    def SerializeToString(self) -> bytes:
        return f"{self.agent_id}|{self.status}".encode()

    @classmethod
    def ParseFromString(cls, data: bytes):
        parts = data.decode().split('|')
        return cls(agent_id=parts[0] if len(parts) > 0 else "",
                   status=int(parts[1]) if len(parts) > 1 else 0)


class Command:
    """Stub for Command protobuf message."""
    def __init__(self, command_id: int = 0, command: str = "", args: str = ""):
        self.command_id = command_id
        self.command = command
        self.args = args

    def SerializeToString(self) -> bytes:
        return f"{self.command_id}|{self.command}|{self.args}".encode()

    @classmethod
    def ParseFromString(cls, data: bytes):
        parts = data.decode().split('|')
        return cls(
            command_id=int(parts[0]) if len(parts) > 0 else 0,
            command=parts[1] if len(parts) > 1 else "",
            args=parts[2] if len(parts) > 2 else ""
        )


class CommandResult:
    """Stub for CommandResult protobuf message."""
    def __init__(self, command_id: int = 0, result: str = "", error: str = ""):
        self.command_id = command_id
        self.result = result
        self.error = error

    def SerializeToString(self) -> bytes:
        return f"{self.command_id}|{self.result}|{self.error}".encode()

    @classmethod
    def ParseFromString(cls, data: bytes):
        parts = data.decode().split('|')
        return cls(
            command_id=int(parts[0]) if len(parts) > 0 else 0,
            result=parts[1] if len(parts) > 1 else "",
            error=parts[2] if len(parts) > 2 else ""
        )


class RelayCreate:
    """Stub for RelayCreate protobuf message."""
    def __init__(self, listen_host: str = "", listen_port: int = 0,
                 forward_host: str = "", forward_port: int = 0):
        self.listen_host = listen_host
        self.listen_port = listen_port
        self.forward_host = forward_host
        self.forward_port = forward_port

    def SerializeToString(self) -> bytes:
        return f"{self.listen_host}|{self.listen_port}|{self.forward_host}|{self.forward_port}".encode()

    @classmethod
    def ParseFromString(cls, data: bytes):
        parts = data.decode().split('|')
        return cls(
            listen_host=parts[0] if len(parts) > 0 else "",
            listen_port=int(parts[1]) if len(parts) > 1 else 0,
            forward_host=parts[2] if len(parts) > 2 else "",
            forward_port=int(parts[3]) if len(parts) > 3 else 0
        )


class RelayDelete:
    """Stub for RelayDelete protobuf message."""
    def __init__(self, relay_id: str = ""):
        self.relay_id = relay_id

    def SerializeToString(self) -> bytes:
        return self.relay_id.encode()

    @classmethod
    def ParseFromString(cls, data: bytes):
        return cls(relay_id=data.decode())


class RelayList:
    """Stub for RelayList protobuf message."""
    def __init__(self):
        pass

    def SerializeToString(self) -> bytes:
        return b""

    @classmethod
    def ParseFromString(cls, data: bytes):
        return cls()


class RelayListResponse:
    """Stub for RelayListResponse protobuf message."""
    def __init__(self, relays: list = None):
        self.relays = relays or []

    def SerializeToString(self) -> bytes:
        relay_strs = [f"{r.get('id', '')}|{r.get('listen_host', '')}|{r.get('listen_port', 0)}|{r.get('forward_host', '')}|{r.get('forward_port', 0)}" for r in self.relays]
        return ";".join(relay_strs).encode()

    @classmethod
    def ParseFromString(cls, data: bytes):
        if not data:
            return cls(relays=[])
        relays = []
        for relay_str in data.decode().split(';'):
            if relay_str:
                parts = relay_str.split('|')
                relays.append({
                    'id': parts[0] if len(parts) > 0 else "",
                    'listen_host': parts[1] if len(parts) > 1 else "",
                    'listen_port': int(parts[2]) if len(parts) > 2 else 0,
                    'forward_host': parts[3] if len(parts) > 3 else "",
                    'forward_port': int(parts[4]) if len(parts) > 4 else 0,
                })
        return cls(relays=relays)