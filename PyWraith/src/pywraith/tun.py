import asyncio
from dataclasses import dataclass
from typing import Optional

@dataclass
class TunConfig:
    name: Optional[str] = None      # e.g. "wraith0" or None for auto
    ip: str = "10.8.0.1"           # TUN interface IP
    netmask: int = 24               # /24
    mtu: int = 1500

class TunDevice:
    """Manages TUN device lifecycle via `ip` command.

    This class creates a TUN device using the `ip tuntap` command,
    configures its IP address, and handles cleanup on destroy.
    """

    def __init__(self, config: TunConfig):
        self.config = config
        self.name: Optional[str] = None
        self._created = False

    async def __aenter__(self) -> "TunDevice":
        await self.create()
        return self

    async def __aexit__(self, *args):
        await self.destroy()

    async def _run(self, *args) -> tuple[int, bytes, bytes]:
        """Run an ip command, return (returncode, stdout, stderr)."""
        proc = await asyncio.create_subprocess_exec(
            "ip", *args,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate()
        return proc.returncode, stdout, stderr

    async def create(self) -> str:
        """Create and configure TUN device. Returns interface name."""
        name = self.config.name or "wraith%d"

        # Create tuntap device
        rc, _, stderr = await self._run(
            "tuntap", "add", "dev", name, "mode", "tun"
        )
        if rc not in (0, 255):  # 255 = already exists
            raise RuntimeError(f"Failed to create TUN device: {stderr.decode()}")

        self.name = name
        self._created = True

        # Set IP and bring up
        ip_addr = f"{self.config.ip}/{self.config.netmask}"
        rc, _, stderr = await self._run("addr", "add", ip_addr, "dev", name)
        if rc != 0:
            raise RuntimeError(f"Failed to set IP: {stderr.decode()}")

        rc, _, _ = await self._run("link", "set", "dev", name, "up")
        if rc != 0:
            raise RuntimeError(f"Failed to bring up TUN device")

        # Set MTU
        rc, _, _ = await self._run("link", "set", "dev", name, "mtu", str(self.config.mtu))

        return name

    async def destroy(self):
        """Destroy TUN device and remove IP."""
        if not self._created or not self.name:
            return

        name = self.name
        try:
            # Bring down
            await self._run("link", "set", "dev", name, "down")
            # Delete
            await self._run("tuntap", "del", "dev", name, "mode", "tun")
        except Exception:
            pass
        finally:
            self._created = False
            self.name = None

    async def add_route(self, destination: str):
        """Add a route through this TUN device."""
        if not self.name:
            raise RuntimeError("TUN device not created")
        rc, _, stderr = await self._run("route", "add", destination, "dev", self.name)
        if rc != 0:
            raise RuntimeError(f"Failed to add route: {stderr.decode()}")

    async def del_route(self, destination: str):
        """Delete a route through this TUN device."""
        if not self.name:
            raise RuntimeError("TUN device not created")
        rc, _, stderr = await self._run("route", "del", destination, "dev", self.name)
        if rc != 0:
            raise RuntimeError(f"Failed to delete route: {stderr.decode()}")