"""
Nexus API client for Wraith tunnel lifecycle management.

Provides HTTP client methods for creating, listing, stopping, and monitoring
tunnels and relays via the Nexus REST API.

The Nexus API endpoints:
    POST   /api/sessions/{id}/tunnels        Start tunnel on session
    GET    /api/sessions/{id}/tunnels        List tunnels
    DELETE /api/sessions/{id}/tunnels/{tid}  Stop tunnel
    GET    /api/tunnels/{tid}/stats          Get tunnel stats
    PATCH  /api/tunnels/{tid}                Update tunnel

Usage:
    client = NexusClient("http://localhost:8080", api_key="...")
    tunnels = client.list_tunnels("session-123")
    client.start_tunnel("session-123", tunnel_id="t1", mode="reverse", ...)
"""

import logging
from typing import Any, Dict, List, Optional

import requests

logger = logging.getLogger("pywraith.nexus")


class NexusClient:
    """
    HTTP client for Nexus tunnel management API.

    All methods are synchronous (requests.Session). For async, wrap with
    asyncio.to_thread() or use an async HTTP client (httpx, aiohttp).

    Args:
        base_url: Nexus server base URL (e.g., "http://localhost:8080")
        api_key: Optional API key for authentication
        timeout: Request timeout in seconds (default: 30)
    """

    def __init__(
        self,
        base_url: str,
        api_key: Optional[str] = None,
        timeout: int = 30,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self._session = requests.Session()
        if api_key:
            self._session.headers["Authorization"] = f"Bearer {api_key}"
        self._session.headers["Content-Type"] = "application/json"

    def _url(self, path: str) -> str:
        return f"{self.base_url}{path}"

    # ─── Tunnel Endpoints ──────────────────────────────────────────────────────

    def start_tunnel(
        self,
        session_id: str,
        tunnel_id: str,
        mode: str = "reverse",
        relay_host: str = "",
        relay_port: int = 0,
        tunnel_ip: str = "",
        encryption: str = "chacha20",
        agent_tun_ip: str = "",
        routes: str = "",
        netmask: str = "255.255.255.0",
    ) -> Dict[str, Any]:
        """
        Start a tunnel on a session.

        POST /api/sessions/{session_id}/tunnels

        Args:
            session_id: Session to start tunnel on
            tunnel_id: Unique tunnel identifier
            mode: "reverse" (agent->relay, default) or "forward" (relay->agent)
            relay_host: Relay server hostname/IP
            relay_port: Relay server port
            tunnel_ip: TUN interface IP (optional, auto-assigned if empty)
            encryption: "chacha20" (default), "aes256", or "tls"
            agent_tun_ip: Agent-side TUN IP (if agent has root)
            routes: Comma-separated routes (e.g., "0.0.0.0/0")
            netmask: Tunnel netmask (default: "255.255.255.0")

        Returns:
            API response dict with tunnel info
        """
        payload = {
            "tunnel_id": tunnel_id,
            "mode": mode,
            "relay_host": relay_host,
            "relay_port": relay_port,
            "tunnel_ip": tunnel_ip,
            "encryption": encryption,
            "agent_tun_ip": agent_tun_ip,
            "routes": routes,
            "netmask": netmask,
        }
        logger.info(f"Starting tunnel {tunnel_id} on session {session_id}")
        resp = self._session.post(
            self._url(f"/api/sessions/{session_id}/tunnels"),
            json=payload,
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    def list_tunnels(self, session_id: str) -> List[Dict[str, Any]]:
        """
        List all tunnels on a session.

        GET /api/sessions/{session_id}/tunnels

        Args:
            session_id: Session to list tunnels for

        Returns:
            List of tunnel info dicts
        """
        resp = self._session.get(
            self._url(f"/api/sessions/{session_id}/tunnels"),
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    def stop_tunnel(self, session_id: str, tunnel_id: str) -> None:
        """
        Stop a tunnel.

        DELETE /api/sessions/{session_id}/tunnels/{tunnel_id}

        Args:
            session_id: Session ID
            tunnel_id: Tunnel to stop
        """
        logger.info(f"Stopping tunnel {tunnel_id} on session {session_id}")
        resp = self._session.delete(
            self._url(f"/api/sessions/{session_id}/tunnels/{tunnel_id}"),
            timeout=self.timeout,
        )
        resp.raise_for_status()

    def get_tunnel_stats(self, tunnel_id: str) -> Dict[str, Any]:
        """
        Get statistics for a tunnel.

        GET /api/tunnels/{tunnel_id}/stats

        Returns:
            Dict with bytes_in, bytes_out, packets_in, packets_out, duration_seconds
        """
        resp = self._session.get(
            self._url(f"/api/tunnels/{tunnel_id}/stats"),
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    def update_tunnel(self, tunnel_id: str, **kwargs: Any) -> Dict[str, Any]:
        """
        Update tunnel configuration.

        PATCH /api/tunnels/{tunnel_id}

        Args:
            tunnel_id: Tunnel to update
            **kwargs: Fields to update (e.g., priority=5)

        Returns:
            Updated tunnel info
        """
        resp = self._session.patch(
            self._url(f"/api/tunnels/{tunnel_id}"),
            json=kwargs,
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    # ─── Relay Endpoints ───────────────────────────────────────────────────────

    def start_relay(
        self,
        session_id: str,
        relay_id: str,
        mode: str,
        local_addr: str = "",
        remote_addr: str = "",
        file_path: str = "",
        command: str = "",
        timeout: int = 300,
        keepalive: bool = True,
    ) -> Dict[str, Any]:
        """
        Start a relay on a session.

        POST /api/sessions/{session_id}/relays

        Args:
            session_id: Session to start relay on
            relay_id: Unique relay identifier
            mode: "tcp" | "udp" | "file" | "exec" | "listener" | "forward"
            local_addr: Agent listen address (modes: tcp, udp)
            remote_addr: Target address (modes: tcp, udp)
            file_path: Local file to send (mode=file)
            command: Command to pipe (mode=exec)
            timeout: Connection timeout in seconds
            keepalive: Enable TCP keepalive

        Returns:
            API response dict with relay info
        """
        payload = {
            "relay_id": relay_id,
            "mode": mode,
            "local_addr": local_addr,
            "remote_addr": remote_addr,
            "file_path": file_path,
            "command": command,
            "timeout": timeout,
            "keepalive": keepalive,
        }
        logger.info(f"Starting relay {relay_id} ({mode}) on session {session_id}")
        resp = self._session.post(
            self._url(f"/api/sessions/{session_id}/relays"),
            json=payload,
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    def list_relays(self, session_id: str) -> List[Dict[str, Any]]:
        """
        List all relays on a session.

        GET /api/sessions/{session_id}/relays

        Args:
            session_id: Session to list relays for

        Returns:
            List of relay info dicts
        """
        resp = self._session.get(
            self._url(f"/api/sessions/{session_id}/relays"),
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    def stop_relay(self, session_id: str, relay_id: str) -> None:
        """
        Stop a relay.

        DELETE /api/sessions/{session_id}/relays/{relay_id}

        Args:
            session_id: Session ID
            relay_id: Relay to stop
        """
        logger.info(f"Stopping relay {relay_id} on session {session_id}")
        resp = self._session.delete(
            self._url(f"/api/sessions/{session_id}/relays/{relay_id}"),
            timeout=self.timeout,
        )
        resp.raise_for_status()

    def get_relay_stats(self, relay_id: str) -> Dict[str, Any]:
        """
        Get statistics for a relay.

        GET /api/relays/{relay_id}/stats

        Returns:
            Dict with bytes_in, bytes_out, connections
        """
        resp = self._session.get(
            self._url(f"/api/relays/{relay_id}/stats"),
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    # ─── Session Management ───────────────────────────────────────────────────

    def get_session(self, session_id: str) -> Dict[str, Any]:
        """
        Get session info.

        GET /api/sessions/{session_id}
        """
        resp = self._session.get(
            self._url(f"/api/sessions/{session_id}"),
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    def list_sessions(self, active_only: bool = False) -> List[Dict[str, Any]]:
        """
        List sessions.

        GET /api/sessions
        """
        params = {"active": 1} if active_only else {}
        resp = self._session.get(
            self._url("/api/sessions"),
            params=params,
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    # ─── Listener Management ──────────────────────────────────────────────────

    def create_listener(
        self,
        shell_type: str = "wraith_tcp",
        bind_host: str = "0.0.0.0",
        bind_port: int = 4445,
    ) -> Dict[str, Any]:
        """
        Create a listener for incoming wraith agents.

        POST /api/listeners
        """
        payload = {
            "shell_type": shell_type,
            "bind_host": bind_host,
            "bind_port": bind_port,
        }
        logger.info(f"Creating wraith_tcp listener on {bind_host}:{bind_port}")
        resp = self._session.post(
            self._url("/api/listeners"),
            json=payload,
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    def list_listeners(self) -> List[Dict[str, Any]]:
        """List active listeners."""
        resp = self._session.get(
            self._url("/api/listeners"),
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()

    def close(self) -> None:
        """Close the underlying HTTP session."""
        self._session.close()
