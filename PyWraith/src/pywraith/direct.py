import asyncio
import json
import os
import uuid
from asyncio.subprocess import Process
from dataclasses import dataclass, field
from typing import Callable, Optional, Awaitable

@dataclass
class DirectConfig:
    binary_path: str = "./wraith"
    tun_name: Optional[str] = None
    tunnel_ip: str = "10.8.0.1/24"
    env: dict = field(default_factory=dict)

class WraithProcess:
    """Manages wraith direct subprocess via JSONL stdin/stdout.

    This class spawns the wraith binary in direct mode and communicates
    with it via newline-delimited JSON messages.
    """

    def __init__(self, config: DirectConfig):
        self.config = config
        self._process: Optional[Process] = None
        self._stdout_task: Optional[asyncio.Task] = None
        self._pending: dict[str, asyncio.Future] = {}
        self._event_handlers: list[Callable[[dict], Awaitable[None]]] = []
        self._running = False

    async def start(self):
        """Start the wraith binary as a subprocess."""
        env = {**os.environ, **self.config.env}

        args = [
            self.config.binary_path, "direct",
            "--tunnel-ip", self.config.tunnel_ip,
        ]
        if self.config.tun_name:
            args.extend(["--tun-name", self.config.tun_name])

        self._process = await asyncio.create_subprocess_exec(
            *args,
            env=env,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        self._running = True
        self._stdout_task = asyncio.create_task(self._read_loop())

    async def _read_loop(self):
        """Read JSONL responses from stdout, route to handlers."""
        assert self._process is not None
        assert self._process.stdout is not None

        while self._running:
            try:
                line = await asyncio.wait_for(
                    self._process.stdout.readline(),
                    timeout=60.0
                )
            except asyncio.TimeoutError:
                continue

            if not line:
                # EOF — process exited
                stderr = await self._process.stderr.read() if self._process.stderr else b""
                if stderr:
                    print(f"wraith stderr: {stderr.decode()}", file=__import__('sys').stderr)
                break

            line = line.decode().strip()
            if not line:
                continue

            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                print(f"Invalid JSON from wraith: {line}", file=__import__('sys').stderr)
                continue

            msg_id = msg.get("id")
            if msg_id and msg_id in self._pending:
                self._pending.pop(msg_id).set_result(msg)
            else:
                for handler in self._event_handlers:
                    await handler(msg)

    async def send_command(self, action: str, params: dict) -> dict:
        """Send a command and wait for the response.

        Args:
            action: The action name (e.g., "node_info", "tunnel_open")
            params: Dict of parameters for the action

        Returns:
            The response dict from wraith
        """
        if not self._process or not self._process.stdin:
            raise RuntimeError("Process not started")

        msg_id = str(uuid.uuid4())
        msg = {
            "type": "command",
            "id": msg_id,
            "action": action,
            "params": params,
        }

        data = json.dumps(msg).encode() + b"\n"
        self._process.stdin.write(data)
        await self._process.stdin.drain()

        future: asyncio.Future = asyncio.get_event_loop().create_future()
        self._pending[msg_id] = future

        return await future

    def on_event(self, handler: Callable[[dict], Awaitable[None]]):
        """Register an async handler for event messages (no id)."""
        self._event_handlers.append(handler)

    # ── High-level API ──────────────────────────────────────────

    async def get_node_info(self) -> dict:
        """Get node information."""
        result = await self.send_command("node_info", {})
        return result.get("result", {})

    async def tunnel_open(self, tunnel_id: str, tun_ip: str, netmask: int = 24) -> dict:
        """Open a tunnel."""
        result = await self.send_command("tunnel_open", {
            "tunnel_id": tunnel_id,
            "tun_ip": tun_ip,
            "netmask": netmask,
        })
        return result.get("result", {})

    async def tunnel_close(self, tunnel_id: str) -> dict:
        """Close a tunnel."""
        result = await self.send_command("tunnel_close", {
            "tunnel_id": tunnel_id,
        })
        return result.get("result", {})

    async def relay_open(self, relay_id: str, mode: str, local: str, remote: str) -> dict:
        """Open a relay."""
        result = await self.send_command("relay_open", {
            "relay_id": relay_id,
            "mode": mode,
            "local": local,
            "remote": remote,
        })
        return result.get("result", {})

    async def relay_close(self, relay_id: str) -> dict:
        """Close a relay."""
        result = await self.send_command("relay_close", {
            "relay_id": relay_id,
        })
        return result.get("result", {})

    async def listen(self, addr: str) -> dict:
        """Start listening for peer connections."""
        result = await self.send_command("listen", {"addr": addr})
        return result.get("result", {})

    async def connect(self, addr: str) -> dict:
        """Connect to a peer."""
        result = await self.send_command("connect", {"addr": addr})
        return result.get("result", {})

    async def status(self) -> dict:
        """Get status (connections, tunnels, relays)."""
        result = await self.send_command("status", {})
        return result.get("result", {})

    async def stop(self):
        """Stop the wraith process gracefully."""
        self._running = False
        if self._process:
            self._process.terminate()
            try:
                await asyncio.wait_for(self._process.wait(), timeout=5.0)
            except asyncio.TimeoutError:
                self._process.kill()
                await self._process.wait()
        if self._stdout_task:
            self._stdout_task.cancel()
            try:
                await self._stdout_task
            except asyncio.CancelledError:
                pass

    async def __aenter__(self) -> "WraithProcess":
        await self.start()
        return self

    async def __aexit__(self, *args):
        await self.stop()