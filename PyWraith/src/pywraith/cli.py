#!/usr/bin/env python3
"""Interactive CLI for pywraith - controlling wraith tunnel tool."""

import sys
import cmd
import argparse
from typing import Optional

from .client import WraithClient


class WraithCLI(cmd.Cmd):
    """Interactive CLI for wraith."""

    intro = """
    ╔═══════════════════════════════════════════════════════════╗
    ║              WRAITH - Tunnel Control Interface              ║
    ╚═══════════════════════════════════════════════════════════╝

    Type 'help' for available commands.
    """
    prompt = "wraith> "

    def __init__(self, host: str = "127.0.0.1", port: int = 4444):
        cmd.Cmd.__init__(self)
        self.host = host
        self.port = port
        self.client: Optional[WraithClient] = None
        self.connected = False

    def do_connect(self, arg):
        """connect [host] [port] - Connect to wraith server."""
        args = arg.split()
        host = args[0] if len(args) > 0 else self.host
        port = int(args[1]) if len(args) > 1 else self.port

        self.client = WraithClient(host, port)
        if self.client.connect():
            self.connected = True
            self.host = host
            self.port = port
            self.prompt = f"wraith [{host}:{port}]> "
            print(f"Connected to {host}:{port}")
        else:
            print("Failed to connect")

    def do_disconnect(self, arg):
        """Disconnect from wraith server."""
        if self.client:
            self.client.disconnect()
            self.connected = False
            self.prompt = "wraith> "
            print("Disconnected")

    def do_create_relay(self, arg):
        """create_relay <listen_host> <listen_port> <forward_host> <forward_port>"""
        if not self.connected:
            print("Not connected. Use 'connect' first.")
            return

        args = arg.split()
        if len(args) != 4:
            print("Usage: create_relay <listen_host> <listen_port> <forward_host> <forward_port>")
            return

        listen_host, listen_port, forward_host, forward_port = args
        success, result = self.client.create_relay(
            listen_host, int(listen_port), forward_host, int(forward_port)
        )

        if success:
            print(f"Relay created: {result.get('output', 'unknown')}")
        else:
            print(f"Failed: {result.get('error', 'unknown error')}")

    def do_delete_relay(self, arg):
        """delete_relay <relay_id> - Delete a relay by ID."""
        if not self.connected:
            print("Not connected. Use 'connect' first.")
            return

        if not arg.strip():
            print("Usage: delete_relay <relay_id>")
            return

        success, result = self.client.delete_relay(arg)
        if success:
            print(f"Relay deleted: {arg}")
        else:
            print(f"Failed: {result.get('error', 'unknown error')}")

    def do_list_relays(self, arg):
        """list_relays - List all active relays."""
        if not self.connected:
            print("Not connected. Use 'connect' first.")
            return

        success, result = self.client.list_relays()
        if success:
            output = result.get('output', '')
            print(f"Relays:\n{output}")
        else:
            print(f"Failed: {result.get('error', 'unknown error')}")

    def do_exit(self, arg):
        """Exit the CLI."""
        if self.client:
            self.client.disconnect()
        print("Goodbye!")
        sys.exit(0)

    def do_quit(self, arg):
        """Exit the CLI."""
        return self.do_exit(arg)

    def do_EOF(self, arg):
        """Handle Ctrl-D."""
        print()
        self.do_exit(arg)


def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(description="Wraith Tunnel Control CLI")
    parser.add_argument("--host", default="127.0.0.1", help="Wraith server host")
    parser.add_argument("--port", type=int, default=4444, help="Wraith server port")
    args = parser.parse_args()

    cli = WraithCLI(args.host, args.port)
    cli.cmdloop()


if __name__ == "__main__":
    main()