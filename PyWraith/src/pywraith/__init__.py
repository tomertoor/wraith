"""
PyWraith - Python commanding package for the Wraith reverse tunnel and relay tool.

Provides:
- Protocol codec for tunnel and relay messages
- CLI for tunnel and relay management
- Nexus API integration for tunnel lifecycle
- Binary config stamping utility

Example usage:
    from pywraith import WraithProtocol, WraithCLI

    # Protocol encode/decode
    msg = WraithProtocol.create_tunnel_open("t1", "10.8.0.1", "10.8.0.2", "255.255.255.0")
    framed = WraithProtocol.encode(msg)

    # CLI entry point
    WraithCLI.main()
"""

__version__ = "1.0.0"

from .protocol import (
    MsgType,
    Channel,
    WraithProtocol,
    WraithRegistration,
    NexusCommand,
    WraithCommandResult,
    WraithHeartbeat,
    WraithFileTransfer,
    AgentMessage,
    TunnelControl,
    RelayControl,
    TunnelOpen,
    TunnelOpenAck,
    TunnelClose,
    TunnelStats,
    TunnelStatsRequest,
    Keepalive,
    KeepaliveAck,
    RelayOpen,
    RelayOpenAck,
    RelayClose,
    RelayStats,
    RelayStatsRequest,
    RelayList,
    RelayListResponse,
    encode_message,
    decode_message,
    create_registration,
    create_command,
    create_heartbeat,
    create_command_result,
    create_file_transfer,
)
from .nexus_integration import NexusClient
from .stamper import stamp_binary, SHELLY_EMBED_V1
from .direct import WraithProcess, DirectConfig
from .tun import TunDevice, TunConfig
from .server import WraithServer
from .session import SessionManager, WraithSession

__all__ = [
    # Version
    "__version__",
    # Protocol
    "MsgType",
    "Channel",
    "WraithProtocol",
    # Agent/Nexus control messages
    "WraithRegistration",
    "NexusCommand",
    "WraithCommandResult",
    "WraithHeartbeat",
    "WraithFileTransfer",
    "AgentMessage",
    "TunnelControl",
    "RelayControl",
    # Tunnel messages
    "TunnelOpen",
    "TunnelOpenAck",
    "TunnelClose",
    "TunnelStats",
    "TunnelStatsRequest",
    "Keepalive",
    "KeepaliveAck",
    # Relay messages
    "RelayOpen",
    "RelayOpenAck",
    "RelayClose",
    "RelayStats",
    "RelayStatsRequest",
    "RelayList",
    "RelayListResponse",
    # Framing API
    "encode_message",
    "decode_message",
    # Factory functions
    "create_registration",
    "create_command",
    "create_heartbeat",
    "create_command_result",
    "create_file_transfer",
    # Nexus
    "NexusClient",
    # Stamper
    "stamp_binary",
    "SHELLY_EMBED_V1",
    # Direct mode subprocess control
    "WraithProcess",
    "DirectConfig",
    # TUN device management
    "TunDevice",
    "TunConfig",
    # Server (subprocess control)
    "WraithServer",
    # Session management
    "SessionManager",
    "WraithSession",
]
