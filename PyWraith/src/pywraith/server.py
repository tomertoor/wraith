"""
WraithServer — TCP server that accepts wraith peer connections.

PyWraith listens on a port for wraith binaries to connect to.
When a wraith connects, it performs the Yamux handshake and then
exchanges tunnel/relay commands over protobuf-encoded Yamux streams.

Usage:
    server = WraithServer("0.0.0.0", 4445)
    await server.start()
    # wraith connect 0.0.0.0:4445 from another machine
    result = await server.tunnel_open("t1", "10.8.0.2/24")
"""

import asyncio
import struct
import logging
import uuid
from dataclasses import dataclass, field
from typing import Callable, Optional, Any

logger = logging.getLogger(__name__)


# Yamux channel IDs (must match src/tunnel/channel.rs)
CHANNEL_TUNNEL_DATA = 0
CHANNEL_TUNNEL_CONTROL = 1
CHANNEL_RELAY_DATA = 3
CHANNEL_RELAY_CONTROL = 5


@dataclass
class TunnelState:
    """Active tunnel state."""
    tunnel_id: str = ""
    tun_ip: str = ""
    netmask: int = 24
    bytes_in: int = 0
    bytes_out: int = 0
    status: str = "opening"


@dataclass
class RelayState:
    """Active relay state."""
    relay_id: str = ""
    mode: str = ""
    local_addr: str = ""
    remote_addr: str = ""
    bound_addr: str = ""
    status: str = "opening"


@dataclass
class PeerConnection:
    """A connected wraith peer over Yamux."""
    reader: asyncio.StreamReader
    writer: asyncio.StreamWriter
    node_id: str = ""
    version: str = "0.2.0"
    tunnels: dict[str, TunnelState] = field(default_factory=dict)
    relays: dict[str, RelayState] = field(default_factory=dict)
    yamux_ready: bool = False


class WraithServer:
    """TCP server that accepts wraith peer connections and manages tunnels/relays."""

    def __init__(self, host: str = "0.0.0.0", port: int = 4445):
        self.host = host
        self.port = port
        self._server: Optional[asyncio.Server] = None
        self._running = False

        # Connected peers (Yamux connections)
        self.peers: dict[str, PeerConnection] = {}  # node_id -> PeerConnection

        # Local state
        self.tunnels: dict[str, TunnelState] = {}
        self.relays: dict[str, RelayState] = {}

        # Node info
        self.node_id = str(uuid.uuid4())
        self.version = "0.2.0"

        # Event callbacks
        self._event_handlers: list[Callable[[str, dict], None]] = []

    # ── Lifecycle ────────────────────────────────────────────────────────────

    async def start(self) -> None:
        """Start listening for peer connections."""
        self._server = await asyncio.start_server(
            self._handle_client,
            self.host,
            self.port,
        )
        self._running = True
        logger.info(f"WraithServer listening on {self.host}:{self.port}")

    async def stop(self) -> None:
        """Stop the server and all peer connections."""
        self._running = False
        if self._server:
            self._server.close()
            await self._server.wait_closed()
        for peer in list(self.peers.values()):
            peer.writer.close()
        self.peers.clear()
        logger.info("WraithServer stopped")

    # ── Connection handling ────────────────────────────────────────────────────

    async def _handle_client(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        """Handle an incoming wraith peer connection."""
        addr = writer.get_extra_info("peername")
        logger.info(f"Accepted connection from {addr}")

        try:
            # Read WRAITH2 protocol prefix
            prefix = await reader.readexactly(8)
            if prefix != b"WRAITH2\n":
                logger.warning(f"Unknown protocol prefix from {addr}: {prefix!r}")
                writer.close()
                return

            # Yamux server-side handshake
            # For now, treat as a simple framed connection
            # Full Yamux would require yamux library — use basic framing
            await self._run_peer_loop(reader, writer, addr)
        except Exception as e:
            logger.error(f"Peer connection error from {addr}: {e}")
        finally:
            writer.close()

    async def _run_peer_loop(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        addr: tuple,
    ) -> None:
        """Run the peer message loop after protocol handshake."""
        peer = PeerConnection(reader=reader, writer=writer)
        self.peers[self.node_id] = peer  # temp until handshake

        while self._running:
            try:
                # Read 8-byte frame header: [channel_id(4)][length(4)]
                header = await reader.readexactly(8)
                channel_id, length = struct.unpack(">II", header)

                if length > 10 * 1024 * 1024:  # 10MB limit
                    logger.warning(f"Oversized frame from {addr}: {length} bytes")
                    break

                payload = await reader.readexactly(length)

                await self._handle_frame(peer, channel_id, payload)
            except asyncio.IncompleteReadError:
                break
            except Exception as e:
                logger.error(f"Frame read error from {addr}: {e}")
                break

        # Cleanup
        self.peers.pop(peer.node_id, None)
        logger.info(f"Peer disconnected: {addr}")

    async def _handle_frame(
        self, peer: PeerConnection, channel_id: int, payload: bytes
    ) -> None:
        """Handle a received Yamux frame."""
        from .protocol import WraithProtocol

        if channel_id == CHANNEL_TUNNEL_CONTROL:
            msg = WraithProtocol.parse_tunnel_control(payload)
            await self._handle_tunnel_control(peer, msg)
        elif channel_id == CHANNEL_RELAY_CONTROL:
            msg = WraithProtocol.parse_relay_control(payload)
            await self._handle_relay_control(peer, msg)
        elif channel_id == CHANNEL_TUNNEL_DATA:
            # Raw tunnel data — forward to TUN device
            await self._handle_tunnel_data(peer, payload)
        else:
            logger.debug(f"Unknown channel {channel_id} from {peer.node_id}")

    async def _handle_tunnel_control(self, peer: PeerConnection, msg) -> None:
        """Handle a tunnel control message from peer."""
        msg_type = msg.WhichOneof("msg") if hasattr(msg, "WhichOneof") else str(msg)

        if hasattr(msg, "open_ack") and msg.open_ack.HasField("tunnel_id"):
            tunnel_id = msg.open_ack.tunnel_id
            success = msg.open_ack.success
            if tunnel_id in peer.tunnels:
                peer.tunnels[tunnel_id].status = "open" if success else "failed"
            if tunnel_id in self.tunnels:
                self.tunnels[tunnel_id].status = "open" if success else "failed"
            logger.info(f"Tunnel {tunnel_id} open_ack: success={success}")

    async def _handle_relay_control(self, peer: PeerConnection, msg) -> None:
        """Handle a relay control message from peer."""
        if hasattr(msg, "open_ack") and msg.open_ack.HasField("relay_id"):
            relay_id = msg.open_ack.relay_id
            success = msg.open_ack.success
            if relay_id in peer.relays:
                peer.relays[relay_id].status = "open" if success else "failed"
            logger.info(f"Relay {relay_id} open_ack: success={success}")

    async def _handle_tunnel_data(self, peer: PeerConnection, payload: bytes) -> None:
        """Handle raw tunnel data from peer — forward to TUN device."""
        # TODO: forward to TUN device
        logger.debug(f"Tunnel data from {peer.node_id}: {len(payload)} bytes")

    # ── Send commands to peers ───────────────────────────────────────────────

    async def _send_frame(
        self, peer: PeerConnection, channel_id: int, payload: bytes
    ) -> None:
        """Send a framed message to a peer."""
        header = struct.pack(">II", channel_id, len(payload))
        peer.writer.write(header + payload)
        await peer.writer.drain()

    async def tunnel_open(
        self, tunnel_id: str, tun_ip: str, netmask: int = 24
    ) -> dict:
        """Open a tunnel on all connected peers (or specific peer)."""
        if not self.peers:
            return {"success": False, "error": "no peers connected"}

        # Use first connected peer for now
        peer = list(self.peers.values())[0]

        # Parse tun_ip "10.8.0.2/24" -> relay_tun_ip, agent_tun_ip
        # The peer is the "relay" side; we are the "agent" side
        tun_ip_str = tun_ip if "/" not in tun_ip else tun_ip.split("/")[0]

        from .protocol import WraithProtocol
        msg = WraithProtocol.create_tunnel_open(
            tunnel_id=tunnel_id,
            relay_tun_ip="10.8.0.1",  # peer's TUN IP
            agent_tun_ip=tun_ip_str,   # our assigned IP
            netmask=f"{netmask}",
        )
        payload = msg.SerializeToString()
        await self._send_frame(peer, CHANNEL_TUNNEL_CONTROL, payload)

        # Track locally
        self.tunnels[tunnel_id] = TunnelState(
            tunnel_id=tunnel_id, tun_ip=tun_ip_str, netmask=netmask, status="opening"
        )
        peer.tunnels[tunnel_id] = TunnelState(
            tunnel_id=tunnel_id, tun_ip=tun_ip_str, netmask=netmask, status="opening"
        )

        return {"success": True, "tunnel_id": tunnel_id}

    async def tunnel_close(self, tunnel_id: str, reason: str = "user_request") -> dict:
        """Close a tunnel."""
        if not self.peers:
            return {"success": False, "error": "no peers connected"}

        peer = list(self.peers.values())[0]

        from .protocol import WraithProtocol
        msg = WraithProtocol.create_tunnel_close(tunnel_id, reason)
        payload = msg.SerializeToString()
        await self._send_frame(peer, CHANNEL_TUNNEL_CONTROL, payload)

        self.tunnels.pop(tunnel_id, None)
        peer.tunnels.pop(tunnel_id, None)
        return {"success": True}

    async def relay_open(
        self, relay_id: str, mode: str, local: str, remote: str = ""
    ) -> dict:
        """Open a relay on connected peer."""
        if not self.peers:
            return {"success": False, "error": "no peers connected"}

        peer = list(self.peers.values())[0]

        from .protocol import WraithProtocol
        msg = WraithProtocol.create_relay_open(
            relay_id=relay_id,
            mode=mode,
            local_addr=local,
            remote_addr=remote,
        )
        payload = msg.SerializeToString()
        await self._send_frame(peer, CHANNEL_RELAY_CONTROL, payload)

        self.relays[relay_id] = RelayState(
            relay_id=relay_id, mode=mode, local_addr=local, remote_addr=remote, status="opening"
        )
        peer.relays[relay_id] = RelayState(
            relay_id=relay_id, mode=mode, local_addr=local, remote_addr=remote, status="opening"
        )
        return {"success": True, "relay_id": relay_id}

    async def relay_close(self, relay_id: str, reason: str = "user_request") -> dict:
        """Close a relay."""
        if not self.peers:
            return {"success": False, "error": "no peers connected"}

        peer = list(self.peers.values())[0]

        from .protocol import WraithProtocol
        msg = WraithProtocol.create_relay_close(relay_id, reason)
        payload = msg.SerializeToString()
        await self._send_frame(peer, CHANNEL_RELAY_CONTROL, payload)

        self.relays.pop(relay_id, None)
        peer.relays.pop(relay_id, None)
        return {"success": True}

    async def status(self) -> dict:
        """Get status of all tunnels, relays, and peers."""
        return {
            "node_id": self.node_id,
            "version": self.version,
            "peers": [
                {
                    "node_id": p.node_id,
                    "version": p.version,
                    "tunnels": len(p.tunnels),
                    "relays": len(p.relays),
                }
                for p in self.peers.values()
            ],
            "tunnels": [t.__dict__ for t in self.tunnels.values()],
            "relays": [r.__dict__ for r in self.relays.values()],
        }

    # ── Event handling ──────────────────────────────────────────────────────

    def on_event(self, handler: Callable[[str, dict], None]) -> None:
        """Register an event handler."""
        self._event_handlers.append(handler)

    # ── Context manager ─────────────────────────────────────────────────────

    async def __aenter__(self) -> "WraithServer":
        await self.start()
        return self

    async def __aexit__(self, *args) -> None:
        await self.stop()
