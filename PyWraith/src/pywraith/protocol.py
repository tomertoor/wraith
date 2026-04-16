"""
Wraith Protocol - Tunnel and relay message codec.

Channel assignment:
  Channel 0: Tunnel data (raw IP packets) - no protobuf wrapper
  Channel 1: Control messages (AgentMessage wrapper for registration, commands, results, heartbeats)
  Channel 3: Relay data (raw length-prefixed bytes)
  Channel 5: Relay control (RelayOpen, RelayOpenAck, RelayClose, RelayStats, RelayList)

Each message on channels 1 and 5 is encoded as (matches Rust wraith/src/tunnel/multiplex.rs):
  [4-byte BE channel][4-byte BE length][protobuf payload]

Channel 0 and 3 are raw byte streams, framed at the transport layer with
4-byte BE length prefixes.
"""

import struct
import time
from dataclasses import dataclass, field, asdict
from enum import IntEnum
from typing import Any, Dict, List, Optional, Tuple, Union

from .proto_gen import tunnel_pb2


class MsgType(IntEnum):
    """Message type IDs, one per unique message."""

    # Tunnel control (Channel 1)
    TUNNEL_OPEN = 0x01
    TUNNEL_OPEN_ACK = 0x02
    TUNNEL_CLOSE = 0x03
    TUNNEL_STATS = 0x04
    TUNNEL_STATS_REQUEST = 0x05
    KEEPALIVE = 0x06
    KEEPALIVE_ACK = 0x07

    # Relay control (Channel 5)
    RELAY_OPEN = 0x10
    RELAY_OPEN_ACK = 0x11
    RELAY_CLOSE = 0x12
    RELAY_STATS = 0x13
    RELAY_STATS_REQUEST = 0x14
    RELAY_LIST = 0x15
    RELAY_LIST_RESPONSE = 0x16


class Channel(IntEnum):
    """Multiplexed channel IDs."""

    TUNNEL_DATA = 0  # Raw IP packets
    TUNNEL_CONTROL = 1  # TunnelOpen, Keepalive, etc.
    # Channel 2 reserved
    RELAY_DATA = 3  # Raw byte stream
    # Channel 4 reserved
    RELAY_CONTROL = 5  # RelayOpen, etc.


# ─── Agent / Nexus Control Messages ─────────────────────────────────────────


@dataclass
class WraithRegistration:
    """Agent registration — sent on first connect to Nexus."""

    hostname: str = ""
    username: str = ""
    os: str = ""  # "linux", "windows", "macos", "bsd"
    os_version: str = ""
    arch: str = ""  # "x64", "x86", "arm", etc.
    domain: str = ""
    ip_address: str = ""
    pid: int = 0
    tun_capable: bool = False  # agent has root (real TUN support)
    userspace_tun: bool = False  # agent using custom userspace stack
    supports_tls: bool = False
    supports_aes256: bool = False

    def to_protobuf(self) -> tunnel_pb2.WraithRegistration:
        msg = tunnel_pb2.WraithRegistration()
        msg.hostname = self.hostname
        msg.username = self.username
        msg.os = self.os
        msg.os_version = self.os_version
        msg.arch = self.arch
        msg.domain = self.domain
        msg.ip_address = self.ip_address
        msg.pid = self.pid
        msg.tun_capable = self.tun_capable
        msg.userspace_tun = self.userspace_tun
        msg.supports_tls = self.supports_tls
        msg.supports_aes256 = self.supports_aes256
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_protobuf(cls, msg: tunnel_pb2.WraithRegistration) -> "WraithRegistration":
        return cls(
            hostname=msg.hostname,
            username=msg.username,
            os=msg.os,
            os_version=msg.os_version,
            arch=msg.arch,
            domain=msg.domain,
            ip_address=msg.ip_address,
            pid=msg.pid,
            tun_capable=msg.tun_capable,
            userspace_tun=msg.userspace_tun,
            supports_tls=msg.supports_tls,
            supports_aes256=msg.supports_aes256,
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "WraithRegistration":
        return cls(
            hostname=d.get("hostname", ""),
            username=d.get("username", ""),
            os=d.get("os", ""),
            os_version=d.get("os_version", ""),
            arch=d.get("arch", ""),
            domain=d.get("domain", ""),
            ip_address=d.get("ip_address", ""),
            pid=int(d.get("pid", 0)),
            tun_capable=bool(d.get("tun_capable", False)),
            userspace_tun=bool(d.get("userspace_tun", False)),
            supports_tls=bool(d.get("supports_tls", False)),
            supports_aes256=bool(d.get("supports_aes256", False)),
        )


@dataclass
class NexusCommand:
    """Command from Nexus to agent — sent via channel 1."""

    command_id: str = ""
    action: str = ""  # "execute", "upload", "download", "tunnel_start", "relay_start", etc.
    params: Dict[str, str] = field(default_factory=dict)
    timeout: int = 0

    def to_protobuf(self) -> tunnel_pb2.NexusCommand:
        msg = tunnel_pb2.NexusCommand()
        msg.command_id = self.command_id
        msg.action = self.action
        msg.timeout = self.timeout
        for k, v in self.params.items():
            msg.params[k] = v
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return {"command_id": self.command_id, "action": self.action, "params": self.params, "timeout": self.timeout}

    @classmethod
    def from_protobuf(cls, msg: tunnel_pb2.NexusCommand) -> "NexusCommand":
        return cls(
            command_id=msg.command_id,
            action=msg.action,
            params=dict(msg.params),
            timeout=msg.timeout,
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "NexusCommand":
        return cls(
            command_id=d.get("command_id", ""),
            action=d.get("action", ""),
            params=dict(d.get("params", {})),
            timeout=int(d.get("timeout", 0)),
        )


@dataclass
class WraithCommandResult:
    """Result of a command execution — agent responds to Nexus."""

    command_id: str = ""
    status: str = "success"  # "success" | "error" | "timeout"
    stdout: str = ""
    stderr: str = ""
    exit_code: int = 0
    error: str = ""
    duration_ms: int = 0

    def to_protobuf(self) -> tunnel_pb2.WraithCommandResult:
        msg = tunnel_pb2.WraithCommandResult()
        msg.command_id = self.command_id
        msg.status = self.status
        msg.stdout = self.stdout
        msg.stderr = self.stderr
        msg.exit_code = self.exit_code
        msg.error = self.error
        msg.duration_ms = self.duration_ms
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_protobuf(cls, msg: tunnel_pb2.WraithCommandResult) -> "WraithCommandResult":
        return cls(
            command_id=msg.command_id,
            status=msg.status,
            stdout=msg.stdout,
            stderr=msg.stderr,
            exit_code=msg.exit_code,
            error=msg.error,
            duration_ms=msg.duration_ms,
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "WraithCommandResult":
        return cls(
            command_id=d.get("command_id", ""),
            status=d.get("status", "success"),
            stdout=d.get("stdout", ""),
            stderr=d.get("stderr", ""),
            exit_code=int(d.get("exit_code", 0)),
            error=d.get("error", ""),
            duration_ms=int(d.get("duration_ms", 0)),
        )


@dataclass
class WraithHeartbeat:
    """Periodic heartbeat from agent to Nexus."""

    commands_executed: int = 0
    status: str = "idle"  # "idle", "running", "error"
    timestamp: int = 0

    def to_protobuf(self) -> tunnel_pb2.WraithHeartbeat:
        msg = tunnel_pb2.WraithHeartbeat()
        msg.commands_executed = self.commands_executed
        msg.status = self.status
        msg.timestamp = self.timestamp or int(time.time())
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_protobuf(cls, msg: tunnel_pb2.WraithHeartbeat) -> "WraithHeartbeat":
        return cls(
            commands_executed=msg.commands_executed,
            status=msg.status,
            timestamp=msg.timestamp,
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "WraithHeartbeat":
        return cls(
            commands_executed=int(d.get("commands_executed", 0)),
            status=d.get("status", "idle"),
            timestamp=int(d.get("timestamp", 0)),
        )


@dataclass
class WraithFileTransfer:
    """File upload/download data."""

    action: str = ""  # "upload" | "download"
    remote_path: str = ""
    offset: int = 0
    total_size: int = 0
    data: bytes = b""

    def to_protobuf(self) -> tunnel_pb2.WraithFileTransfer:
        msg = tunnel_pb2.WraithFileTransfer()
        msg.action = self.action
        msg.remote_path = self.remote_path
        msg.offset = self.offset
        msg.total_size = self.total_size
        if self.data:
            msg.data = self.data
        return msg

    def to_dict(self) -> Dict[str, Any]:
        d = {
            "action": self.action,
            "remote_path": self.remote_path,
            "offset": self.offset,
            "total_size": self.total_size,
        }
        if self.data:
            d["data"] = self.data.hex()
        return d

    @classmethod
    def from_protobuf(cls, msg: tunnel_pb2.WraithFileTransfer) -> "WraithFileTransfer":
        return cls(
            action=msg.action,
            remote_path=msg.remote_path,
            offset=msg.offset,
            total_size=msg.total_size,
            data=msg.data if msg.data else b"",
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "WraithFileTransfer":
        data_hex = d.get("data", "")
        return cls(
            action=d.get("action", ""),
            remote_path=d.get("remote_path", ""),
            offset=int(d.get("offset", 0)),
            total_size=int(d.get("total_size", 0)),
            data=bytes.fromhex(data_hex) if data_hex else b"",
        )


@dataclass
class AgentMessage:
    """
    Oneof wrapper for all messages flowing between agent and Nexus on channel 1.

    Attributes:
        msg_type: String identifier ('registration', 'command', 'heartbeat',
                  'result', 'file_transfer', 'tunnel_control', 'relay_control')
        payload: Typed dataclass instance holding the message data
    """

    msg_type: str = ""
    payload: Any = None

    def to_protobuf(self) -> tunnel_pb2.AgentMessage:
        msg = tunnel_pb2.AgentMessage()
        if isinstance(self.payload, WraithRegistration):
            msg.registration.CopyFrom(self.payload.to_protobuf())
        elif isinstance(self.payload, NexusCommand):
            msg.command.CopyFrom(self.payload.to_protobuf())
        elif isinstance(self.payload, WraithCommandResult):
            msg.result.CopyFrom(self.payload.to_protobuf())
        elif isinstance(self.payload, WraithHeartbeat):
            msg.heartbeat.CopyFrom(self.payload.to_protobuf())
        elif isinstance(self.payload, WraithFileTransfer):
            msg.file_transfer.CopyFrom(self.payload.to_protobuf())
        elif isinstance(self.payload, TunnelControl):
            msg.tunnel_control.CopyFrom(self.payload.to_protobuf())
        elif isinstance(self.payload, RelayControl):
            msg.relay_control.CopyFrom(self.payload.to_protobuf())
        else:
            raise ValueError(f"Unknown payload type for AgentMessage: {type(self.payload)}")
        return msg

    @classmethod
    def from_protobuf(cls, msg: tunnel_pb2.AgentMessage) -> "AgentMessage":
        which = msg.WhichOneof("msg")
        if which == "registration":
            return cls(msg_type="registration", payload=WraithRegistration.from_protobuf(msg.registration))
        elif which == "command":
            return cls(msg_type="command", payload=NexusCommand.from_protobuf(msg.command))
        elif which == "result":
            return cls(msg_type="result", payload=WraithCommandResult.from_protobuf(msg.result))
        elif which == "heartbeat":
            return cls(msg_type="heartbeat", payload=WraithHeartbeat.from_protobuf(msg.heartbeat))
        elif which == "file_transfer":
            return cls(msg_type="file_transfer", payload=WraithFileTransfer.from_protobuf(msg.file_transfer))
        elif which == "tunnel_control":
            return cls(msg_type="tunnel_control", payload=TunnelControl.from_protobuf(msg.tunnel_control))
        elif which == "relay_control":
            return cls(msg_type="relay_control", payload=RelayControl.from_protobuf(msg.relay_control))
        else:
            raise ValueError(f"Unknown oneof field in AgentMessage: {which}")


# ─── Tunnel / Relay Control Wrappers ────────────────────────────────────────


# TunnelControl and RelayControl are oneof wrappers. They hold a single typed
# sub-message. We model them as dataclasses with a `type` field and a `data`
# dict for the sub-message, matching the protobuf oneof semantics.

# msg_type string → dataclass mapping for tunnel control messages
_TUNNEL_CONTROL_TYPES: Dict[str, Any] = {}


@dataclass
class TunnelControl:
    """
    Oneof wrapper for tunnel control messages on channel 1.

    Exactly one sub-message is set. Supported sub-types:
      TunnelOpen, TunnelOpenAck, TunnelClose, TunnelStats,
      TunnelStatsRequest, Keepalive, KeepaliveAck
    """

    type: str = ""  # "open", "open_ack", "close", "stats", "stats_request", "keepalive", "keepalive_ack"
    data: Dict[str, Any] = field(default_factory=dict)

    def to_protobuf(self) -> tunnel_pb2.TunnelControl:
        msg = tunnel_pb2.TunnelControl()
        if self.type == "open":
            msg.open.CopyFrom(TunnelOpen.from_dict(self.data).to_protobuf())
        elif self.type == "open_ack":
            msg.open_ack.CopyFrom(TunnelOpenAck.from_dict(self.data).to_protobuf())
        elif self.type == "close":
            msg.close.CopyFrom(TunnelClose.from_dict(self.data).to_protobuf())
        elif self.type == "stats":
            msg.stats.CopyFrom(TunnelStats.from_dict(self.data).to_protobuf())
        elif self.type == "stats_request":
            msg.stats_request.CopyFrom(TunnelStatsRequest.from_dict(self.data).to_protobuf())
        elif self.type == "keepalive":
            msg.keepalive.CopyFrom(Keepalive.from_dict(self.data).to_protobuf())
        elif self.type == "keepalive_ack":
            msg.keepalive_ack.CopyFrom(KeepaliveAck.from_dict(self.data).to_protobuf())
        return msg

    @classmethod
    def from_protobuf(cls, msg: tunnel_pb2.TunnelControl) -> "TunnelControl":
        which = msg.WhichOneof("msg")
        type_map = {
            "open": ("open", TunnelOpen.from_protobuf(msg.open)),
            "open_ack": ("open_ack", TunnelOpenAck.from_protobuf(msg.open_ack)),
            "close": ("close", TunnelClose.from_protobuf(msg.close)),
            "stats": ("stats", TunnelStats.from_protobuf(msg.stats)),
            "stats_request": ("stats_request", TunnelStatsRequest.from_protobuf(msg.stats_request)),
            "keepalive": ("keepalive", Keepalive.from_protobuf(msg.keepalive)),
            "keepalive_ack": ("keepalive_ack", KeepaliveAck.from_protobuf(msg.keepalive_ack)),
        }
        if which in type_map:
            t, payload = type_map[which]
            return cls(type=t, data=payload.to_dict())
        raise ValueError(f"Unknown tunnel_control oneof: {which}")


@dataclass
class RelayControl:
    """
    Oneof wrapper for relay control messages on channel 5.

    Exactly one sub-message is set. Supported sub-types:
      RelayOpen, RelayOpenAck, RelayClose, RelayStats,
      RelayStatsRequest, RelayList, RelayListResponse
    """

    type: str = ""  # "open", "open_ack", "close", "stats", "stats_request", "list", "list_response"
    data: Dict[str, Any] = field(default_factory=dict)

    def to_protobuf(self) -> tunnel_pb2.RelayControl:
        msg = tunnel_pb2.RelayControl()
        if self.type == "open":
            msg.open.CopyFrom(RelayOpen.from_dict(self.data).to_protobuf())
        elif self.type == "open_ack":
            msg.open_ack.CopyFrom(RelayOpenAck.from_dict(self.data).to_protobuf())
        elif self.type == "close":
            msg.close.CopyFrom(RelayClose.from_dict(self.data).to_protobuf())
        elif self.type == "stats":
            msg.stats.CopyFrom(RelayStats.from_dict(self.data).to_protobuf())
        elif self.type == "stats_request":
            msg.stats_request.CopyFrom(RelayStatsRequest.from_dict(self.data).to_protobuf())
        elif self.type == "list":
            msg.list.CopyFrom(RelayList().to_protobuf())
        elif self.type == "list_response":
            msg.list_response.CopyFrom(RelayListResponse.from_dict(self.data).to_protobuf())
        return msg

    @classmethod
    def from_protobuf(cls, msg: tunnel_pb2.RelayControl) -> "RelayControl":
        which = msg.WhichOneof("msg")
        type_map = {
            "open": ("open", RelayOpen.from_protobuf(msg.open)),
            "open_ack": ("open_ack", RelayOpenAck.from_protobuf(msg.open_ack)),
            "close": ("close", RelayClose.from_protobuf(msg.close)),
            "stats": ("stats", RelayStats.from_protobuf(msg.stats)),
            "stats_request": ("stats_request", RelayStatsRequest.from_protobuf(msg.stats_request)),
            "list": ("list", RelayList()),
            "list_response": ("list_response", RelayListResponse.from_protobuf(msg.list_response)),
        }
        if which in type_map:
            t, payload = type_map[which]
            return cls(type=t, data=payload.to_dict())
        raise ValueError(f"Unknown relay_control oneof: {which}")


# ─── Tunnel Messages ─────────────────────────────────────────────────────────

# NOTE: These dataclasses intentionally mirror the protobuf definitions from
# proto/tunnel.proto. Fields that are not applicable to Python are omitted
# (e.g., bytes/nonce). The nonce field for encryption is handled separately.


@dataclass
class TunnelOpen:
    """Request to open a VPN tunnel."""

    tunnel_id: str = ""
    relay_tun_ip: str = ""
    agent_tun_ip: str = ""
    tunnel_netmask: str = ""
    routes: str = ""  # Comma-separated route destinations
    encryption: str = "chacha20"  # chacha20 | aes256 | tls
    nonce: bytes = b""  # Optional encryption nonce

    def to_protobuf(self) -> "tunnel_pb2.TunnelOpen":
        """Convert to protobuf message for wire encoding."""
        msg = tunnel_pb2.TunnelOpen()
        msg.tunnel_id = self.tunnel_id
        msg.relay_tun_ip = self.relay_tun_ip
        msg.agent_tun_ip = self.agent_tun_ip
        msg.tunnel_netmask = self.tunnel_netmask
        msg.routes = self.routes
        msg.encryption = self.encryption
        if self.nonce:
            msg.nonce = self.nonce
        return msg

    def to_dict(self) -> Dict[str, Any]:
        d = {
            "tunnel_id": self.tunnel_id,
            "relay_tun_ip": self.relay_tun_ip,
            "agent_tun_ip": self.agent_tun_ip,
            "tunnel_netmask": self.tunnel_netmask,
            "routes": self.routes,
            "encryption": self.encryption,
        }
        if self.nonce:
            d["nonce"] = self.nonce.hex()
        return d

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.TunnelOpen") -> "TunnelOpen":
        """Construct from a protobuf message (received from wire)."""
        return cls(
            tunnel_id=msg.tunnel_id,
            relay_tun_ip=msg.relay_tun_ip,
            agent_tun_ip=msg.agent_tun_ip,
            tunnel_netmask=msg.tunnel_netmask,
            routes=msg.routes,
            encryption=msg.encryption,
            nonce=msg.nonce if msg.nonce else b"",
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "TunnelOpen":
        nonce = d.get("nonce", "")
        return cls(
            tunnel_id=d.get("tunnel_id", ""),
            relay_tun_ip=d.get("relay_tun_ip", ""),
            agent_tun_ip=d.get("agent_tun_ip", ""),
            tunnel_netmask=d.get("tunnel_netmask", ""),
            routes=d.get("routes", ""),
            encryption=d.get("encryption", "chacha20"),
            nonce=bytes.fromhex(nonce) if nonce else b"",
        )


@dataclass
class TunnelOpenAck:
    """Response to TunnelOpen."""

    tunnel_id: str = ""
    success: bool = False
    error_msg: str = ""
    assigned_ip: str = ""  # Agent's assigned IP if different from requested

    def to_protobuf(self) -> "tunnel_pb2.TunnelOpenAck":
        msg = tunnel_pb2.TunnelOpenAck()
        msg.tunnel_id = self.tunnel_id
        msg.success = self.success
        msg.error_msg = self.error_msg
        msg.assigned_ip = self.assigned_ip
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return {"tunnel_id": self.tunnel_id, "success": self.success, "error_msg": self.error_msg, "assigned_ip": self.assigned_ip}

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.TunnelOpenAck") -> "TunnelOpenAck":
        return cls(
            tunnel_id=msg.tunnel_id,
            success=msg.success,
            error_msg=msg.error_msg,
            assigned_ip=msg.assigned_ip,
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "TunnelOpenAck":
        return cls(
            tunnel_id=d.get("tunnel_id", ""),
            success=d.get("success", False),
            error_msg=d.get("error_msg", ""),
            assigned_ip=d.get("assigned_ip", ""),
        )


@dataclass
class TunnelClose:
    """Request to close a tunnel."""

    tunnel_id: str = ""
    reason: str = ""  # user_request | agent_disconnect | error

    def to_protobuf(self) -> "tunnel_pb2.TunnelClose":
        msg = tunnel_pb2.TunnelClose()
        msg.tunnel_id = self.tunnel_id
        msg.reason = self.reason
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return {"tunnel_id": self.tunnel_id, "reason": self.reason}

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.TunnelClose") -> "TunnelClose":
        return cls(tunnel_id=msg.tunnel_id, reason=msg.reason)

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "TunnelClose":
        return cls(tunnel_id=d.get("tunnel_id", ""), reason=d.get("reason", ""))


@dataclass
class TunnelStats:
    """Tunnel statistics."""

    tunnel_id: str = ""
    bytes_in: int = 0
    bytes_out: int = 0
    packets_in: int = 0
    packets_out: int = 0

    def to_protobuf(self) -> "tunnel_pb2.TunnelStats":
        msg = tunnel_pb2.TunnelStats()
        msg.tunnel_id = self.tunnel_id
        msg.bytes_in = self.bytes_in
        msg.bytes_out = self.bytes_out
        msg.packets_in = self.packets_in
        msg.packets_out = self.packets_out
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.TunnelStats") -> "TunnelStats":
        return cls(
            tunnel_id=msg.tunnel_id,
            bytes_in=msg.bytes_in,
            bytes_out=msg.bytes_out,
            packets_in=msg.packets_in,
            packets_out=msg.packets_out,
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "TunnelStats":
        return cls(
            tunnel_id=d.get("tunnel_id", ""),
            bytes_in=int(d.get("bytes_in", 0)),
            bytes_out=int(d.get("bytes_out", 0)),
            packets_in=int(d.get("packets_in", 0)),
            packets_out=int(d.get("packets_out", 0)),
        )


@dataclass
class TunnelStatsRequest:
    """Request tunnel statistics."""

    tunnel_id: str = ""

    def to_protobuf(self) -> "tunnel_pb2.TunnelStatsRequest":
        msg = tunnel_pb2.TunnelStatsRequest()
        msg.tunnel_id = self.tunnel_id
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return {"tunnel_id": self.tunnel_id}

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.TunnelStatsRequest") -> "TunnelStatsRequest":
        return cls(tunnel_id=msg.tunnel_id)

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "TunnelStatsRequest":
        return cls(tunnel_id=d.get("tunnel_id", ""))


@dataclass
class Keepalive:
    """Keepalive probe."""

    tunnel_id: str = ""
    timestamp: int = 0

    def to_protobuf(self) -> "tunnel_pb2.Keepalive":
        msg = tunnel_pb2.Keepalive()
        msg.tunnel_id = self.tunnel_id
        msg.timestamp = self.timestamp
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.Keepalive") -> "Keepalive":
        return cls(tunnel_id=msg.tunnel_id, timestamp=msg.timestamp)

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "Keepalive":
        return cls(tunnel_id=d.get("tunnel_id", ""), timestamp=int(d.get("timestamp", 0)))


@dataclass
class KeepaliveAck:
    """Keepalive response."""

    tunnel_id: str = ""
    timestamp: int = 0

    def to_protobuf(self) -> "tunnel_pb2.KeepaliveAck":
        msg = tunnel_pb2.KeepaliveAck()
        msg.tunnel_id = self.tunnel_id
        msg.timestamp = self.timestamp
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.KeepaliveAck") -> "KeepaliveAck":
        return cls(tunnel_id=msg.tunnel_id, timestamp=msg.timestamp)

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "KeepaliveAck":
        return cls(tunnel_id=d.get("tunnel_id", ""), timestamp=int(d.get("timestamp", 0)))


# ─── Relay Messages ───────────────────────────────────────────────────────────

RelayMode = str  # "tcp" | "udp" | "file" | "exec" | "listener" | "forward"


@dataclass
class RelayOpen:
    """Request to open a relay (socat-style)."""

    relay_id: str = ""
    mode: str = "tcp"  # tcp | udp | file | exec | listener | forward
    local_addr: str = ""  # Agent listen address or operator local address
    remote_addr: str = ""  # Target address or agent relay address
    file_path: str = ""  # For file mode
    command: str = ""  # For exec mode
    timeout: int = 300  # Connection timeout in seconds
    keepalive: bool = True

    def to_protobuf(self) -> "tunnel_pb2.RelayOpen":
        msg = tunnel_pb2.RelayOpen()
        msg.relay_id = self.relay_id
        msg.mode = self.mode
        msg.local_addr = self.local_addr
        msg.remote_addr = self.remote_addr
        msg.file_path = self.file_path
        msg.command = self.command
        msg.timeout = self.timeout
        msg.keepalive = self.keepalive
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.RelayOpen") -> "RelayOpen":
        return cls(
            relay_id=msg.relay_id,
            mode=msg.mode,
            local_addr=msg.local_addr,
            remote_addr=msg.remote_addr,
            file_path=msg.file_path,
            command=msg.command,
            timeout=msg.timeout,
            keepalive=msg.keepalive,
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "RelayOpen":
        return cls(
            relay_id=d.get("relay_id", ""),
            mode=d.get("mode", "tcp"),
            local_addr=d.get("local_addr", ""),
            remote_addr=d.get("remote_addr", ""),
            file_path=d.get("file_path", ""),
            command=d.get("command", ""),
            timeout=int(d.get("timeout", 300)),
            keepalive=d.get("keepalive", True),
        )


@dataclass
class RelayOpenAck:
    """Response to RelayOpen."""

    relay_id: str = ""
    success: bool = False
    error_msg: str = ""
    bound_addr: str = ""  # Actual bound address

    def to_protobuf(self) -> "tunnel_pb2.RelayOpenAck":
        msg = tunnel_pb2.RelayOpenAck()
        msg.relay_id = self.relay_id
        msg.success = self.success
        msg.error_msg = self.error_msg
        msg.bound_addr = self.bound_addr
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.RelayOpenAck") -> "RelayOpenAck":
        return cls(
            relay_id=msg.relay_id,
            success=msg.success,
            error_msg=msg.error_msg,
            bound_addr=msg.bound_addr,
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "RelayOpenAck":
        return cls(
            relay_id=d.get("relay_id", ""),
            success=d.get("success", False),
            error_msg=d.get("error_msg", ""),
            bound_addr=d.get("bound_addr", ""),
        )


@dataclass
class RelayClose:
    """Request to close a relay."""

    relay_id: str = ""
    reason: str = ""

    def to_protobuf(self) -> "tunnel_pb2.RelayClose":
        msg = tunnel_pb2.RelayClose()
        msg.relay_id = self.relay_id
        msg.reason = self.reason
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.RelayClose") -> "RelayClose":
        return cls(relay_id=msg.relay_id, reason=msg.reason)

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "RelayClose":
        return cls(relay_id=d.get("relay_id", ""), reason=d.get("reason", ""))


@dataclass
class RelayStats:
    """Relay statistics."""

    relay_id: str = ""
    bytes_in: int = 0
    bytes_out: int = 0
    connections: int = 0

    def to_protobuf(self) -> "tunnel_pb2.RelayStats":
        msg = tunnel_pb2.RelayStats()
        msg.relay_id = self.relay_id
        msg.bytes_in = self.bytes_in
        msg.bytes_out = self.bytes_out
        msg.connections = self.connections
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.RelayStats") -> "RelayStats":
        return cls(
            relay_id=msg.relay_id,
            bytes_in=msg.bytes_in,
            bytes_out=msg.bytes_out,
            connections=msg.connections,
        )

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "RelayStats":
        return cls(
            relay_id=d.get("relay_id", ""),
            bytes_in=int(d.get("bytes_in", 0)),
            bytes_out=int(d.get("bytes_out", 0)),
            connections=int(d.get("connections", 0)),
        )


@dataclass
class RelayStatsRequest:
    """Request relay statistics."""

    relay_id: str = ""

    def to_protobuf(self) -> "tunnel_pb2.RelayStatsRequest":
        msg = tunnel_pb2.RelayStatsRequest()
        msg.relay_id = self.relay_id
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return {"relay_id": self.relay_id}

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.RelayStatsRequest") -> "RelayStatsRequest":
        return cls(relay_id=msg.relay_id)

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "RelayStatsRequest":
        return cls(relay_id=d.get("relay_id", ""))


@dataclass
class RelayList:
    """Request list of active relays."""

    def to_protobuf(self) -> "tunnel_pb2.RelayList":
        return tunnel_pb2.RelayList()

    def to_dict(self) -> Dict[str, Any]:
        return {}

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.RelayList") -> "RelayList":
        return cls()

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "RelayList":
        return cls()


@dataclass
class RelayListResponse:
    """Response containing list of relays."""

    @dataclass
    class RelayInfo:
        relay_id: str = ""
        mode: str = ""
        local_addr: str = ""
        remote_addr: str = ""
        status: str = ""
        connections: int = 0

        def to_dict(self) -> Dict[str, Any]:
            return asdict(self)

        @classmethod
        def from_dict(cls, d: Dict[str, Any]) -> "RelayListResponse.RelayInfo":
            return cls(
                relay_id=d.get("relay_id", ""),
                mode=d.get("mode", ""),
                local_addr=d.get("local_addr", ""),
                remote_addr=d.get("remote_addr", ""),
                status=d.get("status", ""),
                connections=int(d.get("connections", 0)),
            )

    relays: List["RelayListResponse.RelayInfo"] = field(default_factory=list)

    def to_protobuf(self) -> "tunnel_pb2.RelayListResponse":
        msg = tunnel_pb2.RelayListResponse()
        for r in self.relays:
            info = msg.relays.add()
            info.relay_id = r.relay_id
            info.mode = r.mode
            info.local_addr = r.local_addr
            info.remote_addr = r.remote_addr
            info.status = r.status
            info.connections = r.connections
        return msg

    def to_dict(self) -> Dict[str, Any]:
        return {"relays": [r.to_dict() for r in self.relays]}

    @classmethod
    def from_protobuf(cls, msg: "tunnel_pb2.RelayListResponse") -> "RelayListResponse":
        relays = []
        for r in msg.relays:
            relays.append(cls.RelayInfo(
                relay_id=r.relay_id,
                mode=r.mode,
                local_addr=r.local_addr,
                remote_addr=r.remote_addr,
                status=r.status,
                connections=r.connections,
            ))
        return cls(relays=relays)

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "RelayListResponse":
        relays = [cls.RelayInfo.from_dict(r) for r in d.get("relays", [])]
        return cls(relays=relays)


# ─── Protocol Codec ───────────────────────────────────────────────────────────


class WraithProtocol:
    """
    Encode/decode Wraith tunnel and relay messages.

    Framing format (matches Rust wraith/src/tunnel/multiplex.rs):
      [4-byte BE channel][4-byte BE length][protobuf payload]

    Channels 1 and 5 carry protobuf-encoded messages.
    Channels 0 and 3 carry raw tunnel/relay data.

    Usage:
        msg = WraithProtocol.create_tunnel_open("t1", "10.8.0.1", "10.8.0.2", "255.255.255.0")
        framed = WraithProtocol.encode(msg, channel=Channel.TUNNEL_CONTROL)
        decoded = WraithProtocol.decode(framed)  # -> (channel, msg, payload_dict)
    """

    # Type → dataclass mapping
    _TYPE_TO_CLS: Dict[int, Any] = {
        MsgType.TUNNEL_OPEN: TunnelOpen,
        MsgType.TUNNEL_OPEN_ACK: TunnelOpenAck,
        MsgType.TUNNEL_CLOSE: TunnelClose,
        MsgType.TUNNEL_STATS: TunnelStats,
        MsgType.TUNNEL_STATS_REQUEST: TunnelStatsRequest,
        MsgType.KEEPALIVE: Keepalive,
        MsgType.KEEPALIVE_ACK: KeepaliveAck,
        MsgType.RELAY_OPEN: RelayOpen,
        MsgType.RELAY_OPEN_ACK: RelayOpenAck,
        MsgType.RELAY_CLOSE: RelayClose,
        MsgType.RELAY_STATS: RelayStats,
        MsgType.RELAY_STATS_REQUEST: RelayStatsRequest,
        MsgType.RELAY_LIST: RelayList,
        MsgType.RELAY_LIST_RESPONSE: RelayListResponse,
    }

    _CLS_TO_TYPE: Dict[Any, int] = {v: k for k, v in _TYPE_TO_CLS.items()}

    # Protobuf class → msg_type enum mapping
    _PB_TO_MSG_TYPE: Dict[Any, int] = {
        tunnel_pb2.TunnelOpen: MsgType.TUNNEL_OPEN,
        tunnel_pb2.TunnelOpenAck: MsgType.TUNNEL_OPEN_ACK,
        tunnel_pb2.TunnelClose: MsgType.TUNNEL_CLOSE,
        tunnel_pb2.TunnelStats: MsgType.TUNNEL_STATS,
        tunnel_pb2.TunnelStatsRequest: MsgType.TUNNEL_STATS_REQUEST,
        tunnel_pb2.Keepalive: MsgType.KEEPALIVE,
        tunnel_pb2.KeepaliveAck: MsgType.KEEPALIVE_ACK,
        tunnel_pb2.RelayOpen: MsgType.RELAY_OPEN,
        tunnel_pb2.RelayOpenAck: MsgType.RELAY_OPEN_ACK,
        tunnel_pb2.RelayClose: MsgType.RELAY_CLOSE,
        tunnel_pb2.RelayStats: MsgType.RELAY_STATS,
        tunnel_pb2.RelayStatsRequest: MsgType.RELAY_STATS_REQUEST,
        tunnel_pb2.RelayList: MsgType.RELAY_LIST,
        tunnel_pb2.RelayListResponse: MsgType.RELAY_LIST_RESPONSE,
    }

    @classmethod
    def encode(cls, msg: Any, channel: int = Channel.TUNNEL_CONTROL) -> bytes:
        """
        Encode a message with length-prefixed framing.

        Format (matches Rust): [4-byte BE channel][4-byte BE length][protobuf payload]

        Args:
            msg: A dataclass message instance (TunnelOpen, RelayOpen, etc.)
            channel: Channel number (Channel.TUNNEL_CONTROL or Channel.RELAY_CONTROL)

        Returns:
            Fully framed bytes ready to send over the wire.
        """
        msg_type = cls._CLS_TO_TYPE.get(type(msg))
        if msg_type is None:
            raise ValueError(f"Unknown message type: {type(msg)}")

        payload = msg.to_protobuf().SerializeToString()
        frame = struct.pack(">II", channel, len(payload)) + payload
        return frame

    @classmethod
    def decode(cls, framed: bytes) -> Tuple[int, int, Dict[str, Any]]:
        """
        Decode a framed message.

        Args:
            framed: Bytes in format [4-byte BE channel][4-byte BE length][protobuf payload]

        Returns:
            Tuple of (channel, msg_type, payload_dict)
        """
        if len(framed) < 8:
            raise ValueError(f"Frame too short: {len(framed)} bytes")

        channel, payload_len = struct.unpack(">II", framed[:8])
        if payload_len > len(framed) - 8:
            raise ValueError(f"Incomplete frame: expected {payload_len}, got {len(framed) - 8}")

        payload = framed[8 : 8 + payload_len]

        # Parse protobuf to determine msg_type and extract fields
        for pb_cls, mtype in cls._PB_TO_MSG_TYPE.items():
            try:
                pb_msg = pb_cls()
                pb_msg.ParseFromString(payload)
                # Check that the message parsed correctly (ByteSize matches or it's a valid empty message)
                # For empty messages (e.g. RelayList), ByteSize()=0 is valid
                if pb_msg.ByteSize() <= len(payload):
                    return channel, int(mtype), cls._TYPE_TO_CLS[int(mtype)].from_protobuf(pb_msg).to_dict()
            except Exception:
                continue

        raise ValueError(f"Could not decode protobuf payload as any known message type")

    @classmethod
    def decode_message(cls, channel: int, msg_type: int, payload: Dict[str, Any]) -> Any:
        """
        Deserialize a decoded message into its typed dataclass.

        Args:
            channel: Channel number
            msg_type: Message type ID
            payload: JSON-decoded payload dict

        Returns:
            Typed dataclass instance
        """
        cls_name = cls._TYPE_TO_CLS.get(msg_type)
        if cls_name is None:
            raise ValueError(f"Unknown msg_type: {msg_type}")
        return cls_name.from_dict(payload)

    @classmethod
    def encode_data(cls, data: bytes, channel: int) -> bytes:
        """
        Encode raw data on channel 0 (tunnel) or 3 (relay).

        Format (matches Rust): [4-byte BE channel][4-byte BE length][raw data...]

        Args:
            data: Raw byte payload
            channel: Channel.TUNNEL_DATA (0) or Channel.RELAY_DATA (3)

        Returns:
            Framed bytes
        """
        return struct.pack(">II", channel, len(data)) + data

    @classmethod
    def decode_data(cls, framed: bytes) -> Tuple[int, bytes]:
        """
        Decode raw data on channel 0 or 3.

        Args:
            framed: [4-byte BE channel][4-byte BE length][raw data...]

        Returns:
            Tuple of (channel, raw_data)
        """
        if len(framed) < 8:
            raise ValueError(f"Data frame too short: {len(framed)} bytes")

        channel, payload_len = struct.unpack(">II", framed[:8])
        if payload_len > len(framed) - 8:
            raise ValueError(f"Incomplete data frame: expected {payload_len}, got {len(framed) - 8}")

        return channel, framed[8 : 8 + payload_len]

    # ─── Factory methods ───────────────────────────────────────────────────────

    @classmethod
    def create_tunnel_open(
        cls,
        tunnel_id: str,
        relay_tun_ip: str,
        agent_tun_ip: str,
        tunnel_netmask: str = "255.255.255.0",
        routes: str = "",
        encryption: str = "chacha20",
        nonce: bytes = b"",
    ) -> TunnelOpen:
        """Create a TunnelOpen message."""
        return TunnelOpen(
            tunnel_id=tunnel_id,
            relay_tun_ip=relay_tun_ip,
            agent_tun_ip=agent_tun_ip,
            tunnel_netmask=tunnel_netmask,
            routes=routes,
            encryption=encryption,
            nonce=nonce,
        )

    @classmethod
    def create_tunnel_open_ack(
        cls,
        tunnel_id: str,
        success: bool,
        error_msg: str = "",
        assigned_ip: str = "",
    ) -> TunnelOpenAck:
        """Create a TunnelOpenAck message."""
        return TunnelOpenAck(tunnel_id=tunnel_id, success=success, error_msg=error_msg, assigned_ip=assigned_ip)

    @classmethod
    def create_tunnel_close(cls, tunnel_id: str, reason: str = "user_request") -> TunnelClose:
        """Create a TunnelClose message."""
        return TunnelClose(tunnel_id=tunnel_id, reason=reason)

    @classmethod
    def create_tunnel_stats(
        cls,
        tunnel_id: str,
        bytes_in: int = 0,
        bytes_out: int = 0,
        packets_in: int = 0,
        packets_out: int = 0,
    ) -> TunnelStats:
        """Create a TunnelStats message."""
        return TunnelStats(
            tunnel_id=tunnel_id,
            bytes_in=bytes_in,
            bytes_out=bytes_out,
            packets_in=packets_in,
            packets_out=packets_out,
        )

    @classmethod
    def create_tunnel_stats_request(cls, tunnel_id: str) -> TunnelStatsRequest:
        """Create a TunnelStatsRequest message."""
        return TunnelStatsRequest(tunnel_id=tunnel_id)

    @classmethod
    def create_keepalive(cls, tunnel_id: str, timestamp: int = 0) -> Keepalive:
        """Create a Keepalive message."""
        return Keepalive(tunnel_id=tunnel_id, timestamp=timestamp or int(time.time()))

    @classmethod
    def create_keepalive_ack(cls, tunnel_id: str, timestamp: int) -> KeepaliveAck:
        """Create a KeepaliveAck message."""
        return KeepaliveAck(tunnel_id=tunnel_id, timestamp=timestamp)

    @classmethod
    def create_relay_open(
        cls,
        relay_id: str,
        mode: str,
        local_addr: str = "",
        remote_addr: str = "",
        file_path: str = "",
        command: str = "",
        timeout: int = 300,
        keepalive: bool = True,
    ) -> RelayOpen:
        """Create a RelayOpen message."""
        return RelayOpen(
            relay_id=relay_id,
            mode=mode,
            local_addr=local_addr,
            remote_addr=remote_addr,
            file_path=file_path,
            command=command,
            timeout=timeout,
            keepalive=keepalive,
        )

    @classmethod
    def create_relay_open_ack(
        cls,
        relay_id: str,
        success: bool,
        error_msg: str = "",
        bound_addr: str = "",
    ) -> RelayOpenAck:
        """Create a RelayOpenAck message."""
        return RelayOpenAck(relay_id=relay_id, success=success, error_msg=error_msg, bound_addr=bound_addr)

    @classmethod
    def create_relay_close(cls, relay_id: str, reason: str = "user_request") -> RelayClose:
        """Create a RelayClose message."""
        return RelayClose(relay_id=relay_id, reason=reason)

    @classmethod
    def create_relay_stats(
        cls,
        relay_id: str,
        bytes_in: int = 0,
        bytes_out: int = 0,
        connections: int = 0,
    ) -> RelayStats:
        """Create a RelayStats message."""
        return RelayStats(relay_id=relay_id, bytes_in=bytes_in, bytes_out=bytes_out, connections=connections)

    @classmethod
    def create_relay_stats_request(cls, relay_id: str) -> RelayStatsRequest:
        """Create a RelayStatsRequest message."""
        return RelayStatsRequest(relay_id=relay_id)

    @classmethod
    def create_relay_list(cls) -> RelayList:
        """Create a RelayList message."""
        return RelayList()

    @classmethod
    def create_relay_list_response(cls, relays: List[Dict[str, Any]]) -> RelayListResponse:
        """Create a RelayListResponse message from a list of relay dicts."""
        return RelayListResponse(relays=[RelayListResponse.RelayInfo.from_dict(r) for r in relays])

    # ── Parsing (for receiving messages) ────────────────────────────────────

    @staticmethod
    def parse_tunnel_control(data: bytes) -> tunnel_pb2.TunnelControl:
        """Parse raw bytes into a TunnelControl protobuf message."""
        msg = tunnel_pb2.TunnelControl()
        msg.ParseFromString(data)
        return msg

    @staticmethod
    def parse_relay_control(data: bytes) -> tunnel_pb2.RelayControl:
        """Parse raw bytes into a RelayControl protobuf message."""
        msg = tunnel_pb2.RelayControl()
        msg.ParseFromString(data)
        return msg


# ─── AgentMessage Framing API ─────────────────────────────────────────────────


def encode_message(msg: Union[AgentMessage, Any], channel_id: int) -> bytes:
    """
    Encode a message with length-prefixed wire framing.

    Format (matches Rust wraith/src/tunnel/multiplex.rs):
        [4-byte BE channel_id][4-byte BE length][protobuf payload]

    For AgentMessage, the payload is the serialized AgentMessage protobuf.
    For other typed dataclasses (TunnelOpen, etc.), they are wrapped in
    their respective AgentMessage oneof before serialization.

    Args:
        msg: An AgentMessage instance, or any typed dataclass with a
             to_protobuf() method.
        channel_id: Wire channel number (e.g. Channel.TUNNEL_CONTROL=1)

    Returns:
        Framed bytes ready to send over the wire.
    """
    if isinstance(msg, AgentMessage):
        payload = msg.to_protobuf().SerializeToString()
    else:
        # Wrap in AgentMessage
        am = _wrap_in_agent_message(msg)
        payload = am.to_protobuf().SerializeToString()
    return struct.pack(">II", channel_id, len(payload)) + payload


def decode_message(data: bytes) -> Tuple[int, AgentMessage]:
    """
    Decode a framed message from the wire.

    Format (matches Rust): [4-byte BE channel_id][4-byte BE length][protobuf payload]

    Args:
        data: Raw bytes from the wire.

    Returns:
        Tuple of (channel_id, AgentMessage) where AgentMessage has a
        .msg_type string attribute and a .payload attribute containing
        the typed dataclass instance.
    """
    if len(data) < 8:
        raise ValueError(f"Framed message too short: {len(data)} bytes (need >= 8)")

    channel_id, payload_len = struct.unpack(">II", data[:8])
    if payload_len > len(data) - 8:
        raise ValueError(f"Incomplete frame: expected {payload_len} bytes, got {len(data) - 8}")

    payload = data[8 : 8 + payload_len]
    pb_msg = tunnel_pb2.AgentMessage()
    pb_msg.ParseFromString(payload)
    return channel_id, AgentMessage.from_protobuf(pb_msg)


def _wrap_in_agent_message(msg: Any) -> AgentMessage:
    """Wrap a typed dataclass in an AgentMessage oneof wrapper."""
    type_map = {
        WraithRegistration: "registration",
        NexusCommand: "command",
        WraithCommandResult: "result",
        WraithHeartbeat: "heartbeat",
        WraithFileTransfer: "file_transfer",
        TunnelControl: "tunnel_control",
        RelayControl: "relay_control",
    }
    for cls, msg_type in type_map.items():
        if isinstance(msg, cls):
            return AgentMessage(msg_type=msg_type, payload=msg)
    # Also handle tunnel/relay dataclasses by wrapping in control wrappers
    tunnel_type_map = {
        TunnelOpen: "open",
        TunnelOpenAck: "open_ack",
        TunnelClose: "close",
        TunnelStats: "stats",
        TunnelStatsRequest: "stats_request",
        Keepalive: "keepalive",
        KeepaliveAck: "keepalive_ack",
    }
    relay_type_map = {
        RelayOpen: "open",
        RelayOpenAck: "open_ack",
        RelayClose: "close",
        RelayStats: "stats",
        RelayStatsRequest: "stats_request",
        RelayList: "list",
        RelayListResponse: "list_response",
    }
    if isinstance(msg, tuple(tunnel_type_map.keys())):
        sub_type = tunnel_type_map[type(msg)]
        return AgentMessage(
            msg_type="tunnel_control",
            payload=TunnelControl(type=sub_type, data=msg.to_dict()),
        )
    if isinstance(msg, tuple(relay_type_map.keys())):
        sub_type = relay_type_map[type(msg)]
        return AgentMessage(
            msg_type="relay_control",
            payload=RelayControl(type=sub_type, data=msg.to_dict()),
        )
    raise ValueError(f"Unknown message type for AgentMessage wrapping: {type(msg)}")


# ─── Message Factory Functions ────────────────────────────────────────────────


def create_registration(
    hostname: str,
    username: str,
    os: str,
    os_version: str,
    arch: str,
    domain: str,
    ip_address: str,
    pid: int,
    tun_capable: bool,
    userspace_tun: bool,
    supports_tls: bool,
    supports_aes256: bool,
) -> AgentMessage:
    """
    Create an AgentMessage wrapping a WraithRegistration payload.

    This message is sent by the agent on first connect to the Nexus,
    before Yamux stream negotiation.

    Args:
        hostname: Machine hostname
        username: Logged-in username
        os: OS name ("linux", "windows", "macos", "bsd")
        os_version: OS version string
        arch: CPU architecture ("x64", "x86", "arm", etc.)
        domain: Windows domain or host name
        ip_address: Primary IP address
        pid: Process ID of the agent
        tun_capable: True if the agent has root privileges (real TUN support)
        userspace_tun: True if using a custom userspace networking stack
        supports_tls: True if the agent supports TLS connections
        supports_aes256: True if the agent supports AES-256 encryption

    Returns:
        AgentMessage with msg_type='registration'
    """
    reg = WraithRegistration(
        hostname=hostname,
        username=username,
        os=os,
        os_version=os_version,
        arch=arch,
        domain=domain,
        ip_address=ip_address,
        pid=pid,
        tun_capable=tun_capable,
        userspace_tun=userspace_tun,
        supports_tls=supports_tls,
        supports_aes256=supports_aes256,
    )
    return AgentMessage(msg_type="registration", payload=reg)


def create_command(
    command_id: str,
    action: str,
    params: Optional[Dict[str, str]] = None,
    timeout: int = 30,
) -> AgentMessage:
    """
    Create an AgentMessage wrapping a NexusCommand payload.

    This message is sent by the Nexus to the agent via channel 1.

    Args:
        command_id: Unique identifier for this command
        action: Action to perform ("execute", "upload", "download",
                "tunnel_start", "relay_start", etc.)
        params: Key-value parameters for the action
        timeout: Command timeout in seconds

    Returns:
        AgentMessage with msg_type='command'
    """
    cmd = NexusCommand(
        command_id=command_id,
        action=action,
        params=params or {},
        timeout=timeout,
    )
    return AgentMessage(msg_type="command", payload=cmd)


def create_heartbeat(
    commands_executed: int = 0,
    status: str = "idle",
    timestamp: Optional[int] = None,
) -> AgentMessage:
    """
    Create an AgentMessage wrapping a WraithHeartbeat payload.

    Sent periodically by the agent to the Nexus.

    Args:
        commands_executed: Number of commands executed since last heartbeat
        status: Agent status ("idle", "running", "error")
        timestamp: Unix timestamp (defaults to current time)

    Returns:
        AgentMessage with msg_type='heartbeat'
    """
    hb = WraithHeartbeat(
        commands_executed=commands_executed,
        status=status,
        timestamp=timestamp or int(time.time()),
    )
    return AgentMessage(msg_type="heartbeat", payload=hb)


def create_command_result(
    command_id: str,
    status: str = "success",
    stdout: str = "",
    stderr: str = "",
    exit_code: int = 0,
    error: str = "",
    duration_ms: int = 0,
) -> AgentMessage:
    """
    Create an AgentMessage wrapping a WraithCommandResult payload.

    Sent by the agent in response to a NexusCommand.

    Args:
        command_id: Command ID this result corresponds to
        status: Result status ("success", "error", "timeout")
        stdout: Standard output from command execution
        stderr: Standard error from command execution
        exit_code: Process exit code
        error: Error message (for status="error")
        duration_ms: Execution time in milliseconds

    Returns:
        AgentMessage with msg_type='result'
    """
    result = WraithCommandResult(
        command_id=command_id,
        status=status,
        stdout=stdout,
        stderr=stderr,
        exit_code=exit_code,
        error=error,
        duration_ms=duration_ms,
    )
    return AgentMessage(msg_type="result", payload=result)


def create_file_transfer(
    action: str,
    remote_path: str,
    offset: int = 0,
    total_size: int = 0,
    data: bytes = b"",
) -> AgentMessage:
    """
    Create an AgentMessage wrapping a WraithFileTransfer payload.

    Args:
        action: Transfer action ("upload" or "download")
        remote_path: Path on the remote (agent) side
        offset: Byte offset within the file
        total_size: Total file size in bytes
        data: Raw file chunk bytes

    Returns:
        AgentMessage with msg_type='file_transfer'
    """
    ft = WraithFileTransfer(
        action=action,
        remote_path=remote_path,
        offset=offset,
        total_size=total_size,
        data=data,
    )
    return AgentMessage(msg_type="file_transfer", payload=ft)
