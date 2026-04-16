"""
WraithSession — Manages connected peer sessions.

Each session is a peer-to-peer connection to another wraith node.
Tracks tunnels, relays, and peer info per session.
"""

from dataclasses import dataclass, field
from datetime import datetime
from typing import Optional, Dict, Any


@dataclass
class PeerInfo:
    """Info exchanged during peer handshake."""
    node_id: str = ""
    version: str = "0.2.0"
    tun_capable: bool = True
    tun_ip: str = ""
    supports_relay: bool = True
    supports_nexus: bool = True
    options: Dict[str, str] = field(default_factory=dict)


@dataclass
class WraithSession:
    """Represents a connected peer session."""
    session_id: str
    addr: str = ""  # "host:port" of peer
    connected_at: datetime = field(default_factory=datetime.now)
    last_active: datetime = field(default_factory=datetime.now)
    status: str = "connected"  # "connected" | "handshake" | "active" | "closed"
    peer_info: Optional[PeerInfo] = None

    # Per-session state
    tunnels: Dict[str, Dict[str, Any]] = field(default_factory=dict)
    relays: Dict[str, Dict[str, Any]] = field(default_factory=dict)

    def update_activity(self):
        self.last_active = datetime.now()

    def add_tunnel(self, tunnel_id: str, info: Dict[str, Any]):
        self.tunnels[tunnel_id] = info
        self.update_activity()

    def remove_tunnel(self, tunnel_id: str):
        self.tunnels.pop(tunnel_id, None)

    def add_relay(self, relay_id: str, info: Dict[str, Any]):
        self.relays[relay_id] = info
        self.update_activity()

    def remove_relay(self, relay_id: str):
        self.relays.pop(relay_id, None)

    def __str__(self):
        return f"WraithSession({self.session_id} {self.addr} {self.status})"


class SessionManager:
    """Manages connected peer sessions."""

    def __init__(self):
        self.sessions: Dict[str, WraithSession] = {}
        self._session_counter = 0

    def create_session(self, addr: str) -> WraithSession:
        """Create a new pending session."""
        self._session_counter += 1
        session_id = f"peer_{self._session_counter}"
        session = WraithSession(session_id=session_id, addr=addr)
        self.sessions[session_id] = session
        return session

    def get_session(self, session_id: str) -> Optional[WraithSession]:
        return self.sessions.get(session_id)

    def remove_session(self, session_id: str) -> bool:
        if session_id in self.sessions:
            del self.sessions[session_id]
            return True
        return False

    def list_sessions(self) -> list:
        """List all sessions as formatted strings."""
        if not self.sessions:
            return ["No active sessions"]
        lines = ["Active Sessions:", "-" * 60]
        for sid, s in self.sessions.items():
            lines.append(f"[{sid}] {s.addr} status={s.status} tunnels={len(s.tunnels)} relays={len(s.relays)}")
        return lines
