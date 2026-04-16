#!/usr/bin/env python3
"""
PyWraith CLI — Interactive command interface for Wraith tunnel orchestrator.

Usage:
    wraith                  # Interactive shell
    wraith start 0.0.0.0:4445  # Start listener (blocking)
    wraith status           # Show status
    wraith tunnel open ...
    wraith relay open ...

WraithServer listens for wraith binaries to connect to it via `wraith connect <addr>`.
Once connected, use tunnel/relay commands to manage the peer.
"""

import asyncio
import cmd
import threading
import sys
import argparse
from typing import Optional

from .server import WraithServer


class PyWraithCLI(cmd.Cmd):
    """Interactive CLI for PyWraith."""

    intro = """
    ╔═══════════════════════════════════════════════════════════╗
    ║              PyWraith — Interactive Shell                 ║
    ╚═══════════════════════════════════════════════════════════╝

    Use 'start <addr>' to listen for wraith peer connections.
    Then use 'tunnel open', 'relay open', etc.

    Type 'help' for available commands.
    """
    prompt = "pywraith> "

    def __init__(self):
        cmd.Cmd.__init__(self)
        self.server: Optional[WraithServer] = None
        self._server_thread: Optional[threading.Thread] = None
        self._server_ready = asyncio.Event()

    # ── Server lifecycle ──────────────────────────────────────────────────

    def do_start(self, arg):
        """start [addr] — Start listening for wraith peer connections (blocks terminal)."""
        addr = arg.strip() or "0.0.0.0:4445"
        if ":" not in addr:
            addr = f"{addr}:4445"

        host, port_s = addr.rsplit(":", 1)
        port = int(port_s)

        if self.server and self.server._running:
            print(f"Server already running on {self.server.host}:{self.server.port}")
            return

        self.server = WraithServer(host=host, port=port)

        # Run in a background thread so cmdloop stays responsive
        def run_server():
            asyncio.run(self._serve_background())

        self._server_thread = threading.Thread(target=run_server, daemon=True)
        self._server_thread.start()

        # Wait for server to be ready
        import time
        for _ in range(50):  # 5 second timeout
            time.sleep(0.1)
            if self.server._running:
                break

        if self.server._running:
            print(f"WraithServer listening on {host}:{port}")
            print("Waiting for wraith peers to connect...")
            print("  From wraith binary: wraith connect <this_ip>:{port}")
        else:
            print("Failed to start server.")

    async def _serve_background(self) -> None:
        """Run the server in background thread."""
        try:
            await self.server.start()
            # Block forever until stopped
            while self.server._running:
                await asyncio.sleep(1)
        except Exception as e:
            print(f"Server error: {e}")
        finally:
            self.server._running = False

    def do_stop(self, arg):
        """stop — Stop the WraithServer."""
        if not self.server or not self.server._running:
            print("Server not running.")
            return

        async def run():
            await self.server.stop()

        asyncio.run(run())
        print("Server stopped.")

    # ── Helpers ──────────────────────────────────────────────────────────

    def _check_server(self) -> bool:
        if not self.server or not self.server._running:
            print("Server not running. Use 'start <addr>' first.")
            return False
        if not self.server.peers:
            print("No peers connected. Run 'wraith connect <addr>' on the wraith binary.")
            return False
        return True

    def parseline(self, line: str):
        """Override parseline to handle both 'relay open' and 'relay_open' syntax.

        cmd.Cmd splits on the first space, so 'relay open args' looks for do_relay.
        We also check for 'relay_open' (underscore) so both forms work.
        """
        command, arg, line = super().parseline(line)
        if command is None:
            return None, None, line

        # If no handler found, try underscore variant: "relay open" -> "relay_open"
        if not hasattr(self, f"do_{command}"):
            words = arg.split() if arg else []
            if words:
                combined = f"{command}_{words[0]}"
                if hasattr(self, f"do_{combined}"):
                    rest = " ".join(words[1:]) if len(words) > 1 else ""
                    return combined, rest, line

        return command, arg, line

    def onecmd(self, line: str):
        """Override onecmd to use underscore-style method lookup."""
        command, arg, _ = self.parseline(line)
        if command and hasattr(self, f"do_{command}"):
            return getattr(self, f"do_{command}")(arg)
        return super().onecmd(line)

    def _run_async(self, coro):
        """Run a coroutine and print result."""
        try:
            return asyncio.run(coro)
        except Exception as e:
            print(f"Error: {e}")
            return None

    # ── Tunnel commands ───────────────────────────────────────────────────

    def do_tunnel_open(self, arg):
        """tunnel open <tunnel_id> <tun_ip[/netmask]> — Open a tunnel on connected peer."""
        args = arg.split()
        if len(args) < 2:
            print("Usage: tunnel open <tunnel_id> <tun_ip[/netmask]>")
            return
        if not self._check_server():
            return

        tunnel_id, tun_ip = args[0], args[1]
        netmask = 24
        if "/" in tun_ip:
            tun_ip, nm = tun_ip.split("/")
            netmask = int(nm)

        result = self._run_async(
            self.server.tunnel_open(tunnel_id, tun_ip, netmask)
        )
        if result:
            print(f"Tunnel open: {result}")

    def do_tunnel_close(self, arg):
        """tunnel close <tunnel_id> — Close a tunnel."""
        if not arg.strip():
            print("Usage: tunnel close <tunnel_id>")
            return
        if not self._check_server():
            return

        result = self._run_async(
            self.server.tunnel_close(arg.strip())
        )
        if result:
            print(f"Tunnel close: {result}")

    def do_tunnel_list(self, arg):
        """tunnel list — List active tunnels."""
        if not self.server or not self.server._running:
            print("Server not running.")
            return
        tunnels = list(self.server.tunnels.values())
        if not tunnels:
            print("No active tunnels.")
            return
        for t in tunnels:
            print(f"  [{t.status}] {t.tunnel_id} {t.tun_ip}/{t.netmask}")

    # ── Relay commands ────────────────────────────────────────────────────

    def do_relay_open(self, arg):
        """relay open <relay_id> <mode> <local_addr> [remote_addr] — Open a relay."""
        args = arg.split()
        if len(args) < 3:
            print("Usage: relay open <relay_id> <mode> <local_addr> [remote_addr]")
            return
        if not self._check_server():
            return

        relay_id, mode, local = args[0], args[1], args[2]
        remote = args[3] if len(args) > 3 else ""

        result = self._run_async(
            self.server.relay_open(relay_id, mode, local, remote)
        )
        if result:
            print(f"Relay open: {result}")

    def do_relay_close(self, arg):
        """relay close <relay_id> — Close a relay."""
        if not arg.strip():
            print("Usage: relay close <relay_id>")
            return
        if not self._check_server():
            return

        result = self._run_async(
            self.server.relay_close(arg.strip())
        )
        if result:
            print(f"Relay close: {result}")

    def do_relay_list(self, arg):
        """relay list — List active relays."""
        if not self.server or not self.server._running:
            print("Server not running.")
            return
        relays = list(self.server.relays.values())
        if not relays:
            print("No active relays.")
            return
        for r in relays:
            print(f"  [{r.status}] {r.relay_id} {r.mode} {r.local_addr} -> {r.remote_addr}")

    # ── Status ─────────────────────────────────────────────────────────────

    def do_status(self, arg):
        """status — Show server status and connected peers."""
        if not self.server or not self.server._running:
            print("Server not running.")
            return

        result = self._run_async(self.server.status())
        if result:
            print(f"Node: {result['node_id']} ({result['version']})")
            print(f"Peers: {len(result['peers'])}")
            for p in result["peers"]:
                print(f"  - {p['node_id']} (v{p['version']}) tunnels={p['tunnels']} relays={p['relays']}")
            print(f"Tunnels: {len(result['tunnels'])}")
            print(f"Relays: {len(result['relays'])}")

    def do_peers(self, arg):
        """peers — List connected wraith peers."""
        if not self.server or not self.server._running:
            print("Server not running.")
            return
        if not self.server.peers:
            print("No peers connected.")
            return
        for node_id, peer in self.server.peers.items():
            print(f"  {node_id} (v{peer.version}) tunnels={len(peer.tunnels)} relays={len(peer.relays)}")

    # ── Exit ───────────────────────────────────────────────────────────────

    def do_exit(self, arg):
        """Exit."""
        if self.server and self.server._running:
            self.do_stop("")
        print("Goodbye!")
        sys.exit(0)

    def do_quit(self, arg):
        """Exit."""
        return self.do_exit(arg)

    def do_EOF(self, arg):
        """Handle Ctrl-D."""
        print()
        self.do_exit(arg)


def main():
    parser = argparse.ArgumentParser(description="PyWraith CLI")
    parser.add_argument("--host", default="0.0.0.0", help="Listen host")
    parser.add_argument("--port", type=int, default=4445, help="Listen port")
    parser.add_argument("command", nargs="*", help="Command to run")
    args = parser.parse_args()

    cli = PyWraithCLI()

    if args.command:
        cmd_str = " ".join(args.command)
        cmd0 = args.command[0]

        if cmd0 == "start":
            addr = f"{args.host}:{args.port}"
            if len(args.command) > 1:
                addr = args.command[1]
            cli.do_start(addr)
        elif cmd0 == "stop":
            cli.do_stop("")
        elif cmd0 == "status":
            cli.do_status("")
        elif cmd0 == "peers":
            cli.do_peers("")
        elif cmd0 == "tunnel":
            if len(args.command) < 2:
                print("Usage: wraith tunnel open/close/list ...")
                sys.exit(1)
            sub = args.command[1]
            if sub == "open":
                cli.do_tunnel_open(" ".join(args.command[2:]))
            elif sub == "close":
                cli.do_tunnel_close(args.command[2] if len(args.command) > 2 else "")
            elif sub == "list":
                cli.do_tunnel_list("")
        elif cmd0 == "relay":
            if len(args.command) < 2:
                print("Usage: wraith relay open/close/list ...")
                sys.exit(1)
            sub = args.command[1]
            if sub == "open":
                cli.do_relay_open(" ".join(args.command[2:]))
            elif sub == "close":
                cli.do_relay_close(args.command[2] if len(args.command) > 2 else "")
            elif sub == "list":
                cli.do_relay_list("")
        else:
            print(f"Unknown command: {cmd0}")
            sys.exit(1)
    else:
        cli.cmdloop()


if __name__ == "__main__":
    main()
