#!/usr/bin/env python3
"""Interactive CLI for pywraith - controlling wraith tunnel tool."""

import sys
import argparse
import logging
from typing import Optional

from IPython.terminal.interactiveshell import TerminalInteractiveShell

from PyWraith.client import WraithClient
from PyWraith.server import WraithServer


logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')


class WraithCLI:
    """Interactive CLI for wraith with IPython backend."""

    def __init__(self, mode: str = "connect", host: str = "127.0.0.1", port: int = 4444, listen_port: int = 4445):
        self.mode = mode
        self.host = host
        self.port = port
        self.listen_port = listen_port
        self.client: Optional[WraithClient] = None
        self.server: Optional[WraithServer] = None
        self.connected = False
        self._shell: Optional[TerminalInteractiveShell] = None

    def _update_prompt(self):
        """Update shell prompt if running."""
        pass  # Prompt updates removed - IPython 8+ doesn't support prompt_manager

    def _get_prompt(self) -> str:
        """Generate prompt based on current mode."""
        if self.mode == "listen":
            return f"wraith [listen:{self.listen_port}]> "
        elif self.connected:
            return f"wraith [{self.host}:{self.port}]> "
        return "wraith> "

    def run_shell(self):
        """Run the IPython shell."""
        self._shell = TerminalInteractiveShell.instance(
            prompt_in1=self._get_prompt(),
            prompt_out='',
            banner1='''
    ╔═══════════════════════════════════════════════════════════╗
    ║              WRAITH - Tunnel Control Interface              ║
    ╚═══════════════════════════════════════════════════════════╝

    Type %connect, %listen, %create_relay, etc. for commands.
    Use %help for full command list.
    ''',
        )

        # Expose CLI and client/server in user namespace
        self._shell.user_ns.update({
            'cli': self,
            'client': self.client,
            'server': self.server,
        })

        # Register magic commands after shell is created
        magic_methods = {
            'connect': self.do_connect,
            'disconnect': self.do_disconnect,
            'create_relay': self.do_create_relay,
            'delete_relay': self.do_delete_relay,
            'list_relays': self.do_list_relays,
            'listen': self.do_listen,
            'stop_listening': self.do_stop_listening,
            'agents': self.do_agents,
            'status': self.do_status,
            'help': self.do_help,
            'exit': self.do_exit,
        }

        for name, method in magic_methods.items():
            self._shell.register_magic_function(method, 'line', name)

        self._shell.mainloop()

    # ---- Command handlers ----

    def do_listen(self, arg: str = ""):
        """%listen [port] - Start listening for wraith agents."""
        if self.mode == "listen":
            print("Already in listen mode")
            return

        args = arg.split()
        port = int(args[0]) if len(args) > 0 else self.listen_port

        self.server = WraithServer(port=port)
        if self.server.start():
            self.mode = "listen"
            self.listen_port = port
            self._update_prompt()
            print(f"Listening on port {port} for wraith agents")
        else:
            print(f"Failed to start listening on port {port}")

    def do_stop_listening(self, arg: str = ""):
        """%stop_listening - Stop listening for agents."""
        if self.mode != "listen" or not self.server:
            print("Not in listen mode")
            return

        self.server.stop()
        self.server = None
        self.mode = "connect"
        self._update_prompt()
        print("Stopped listening")

    def do_agents(self, arg: str = ""):
        """%agents - List connected agents (listen mode only)."""
        if self.mode != "listen" or not self.server:
            print("Not in listen mode")
            return

        agents = self.server.list_agents()
        if not agents:
            print("No agents connected")
            return

        print("Connected agents:")
        for agent in agents:
            print(f"  {agent['agent_id'][:8]}... - {agent['username']}@{agent['hostname']} ({agent['ip_address']})")

    def do_connect(self, arg: str = ""):
        """%connect [host] [port] - Connect to wraith server."""
        args = arg.split()
        host = args[0] if len(args) > 0 else self.host
        port = int(args[1]) if len(args) > 1 else self.port

        self.client = WraithClient(host, port)
        if self.client.connect():
            self.connected = True
            self.host = host
            self.port = port
            self._update_prompt()
            print(f"Connected to {host}:{port}")
        else:
            print("Failed to connect")

    def do_disconnect(self, arg: str = ""):
        """%disconnect - Disconnect from wraith server or stop listening."""
        if self.mode == "listen" and self.server:
            self.server.stop()
            self.server = None
            self.mode = "connect"
            self._update_prompt()
            print("Stopped listening")
        elif self.client:
            self.client.disconnect()
            self.connected = False
            self._update_prompt()
            print("Disconnected")

    def do_create_relay(self, arg: str = ""):
        """%create_relay -l <protocol> -L <host> <port> -f <protocol> -F <host> <port> - Create protocol-relaying relay"""
        if not self.connected:
            print("Not connected. Use 'connect' first.")
            return

        # Parse: -l tcp -L 0.0.0.0 2222 -f udp -F 127.0.0.1 3333
        parts = arg.split()
        if len(parts) != 10:
            print("Usage: create_relay -l <protocol> -L <host> <port> -f <protocol> -F <host> <port>")
            print("Example: create_relay -l tcp -L 0.0.0.0 2222 -f udp -F 127.0.0.1 3333")
            return

        # Parse flags
        try:
            listen_protocol = parts[1]  # after -l
            listen_host = parts[3]
            listen_port = int(parts[4])
            forward_protocol = parts[6]  # after -f
            forward_host = parts[8]
            forward_port = int(parts[9])
        except (IndexError, ValueError) as e:
            print(f"Parse error: {e}")
            print("Usage: create_relay -l <protocol> -L <host> <port> -f <protocol> -F <host> <port>")
            return

        # Validate protocols
        if listen_protocol not in ('tcp', 'udp'):
            print(f"Invalid listen protocol: {listen_protocol} (must be tcp or udp)")
            return
        if forward_protocol not in ('tcp', 'udp'):
            print(f"Invalid forward protocol: {forward_protocol} (must be tcp or udp)")
            return

        # Validate ports
        if not (0 <= listen_port <= 65535):
            print(f"Listen port {listen_port} out of range (0-65535)")
            return
        if not (0 <= forward_port <= 65535):
            print(f"Forward port {forward_port} out of range (0-65535)")
            return

        success, result = self.client.create_relay(
            listen_host, listen_port, listen_protocol,
            forward_host, forward_port, forward_protocol
        )

        if success:
            print(f"Relay created: {result.get('output', 'unknown')}")
        else:
            print(f"Failed: {result.get('error', 'unknown error')}")

    def do_delete_relay(self, arg: str = ""):
        """%delete_relay <relay_id> - Delete a relay by ID."""
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

    def do_list_relays(self, arg: str = ""):
        """%list_relays - List all active relays."""
        if not self.connected:
            print("Not connected. Use 'connect' first.")
            return

        success, result = self.client.list_relays()
        if success:
            output = result.get('output', '')
            print(f"Relays:\n{output}")
        else:
            print(f"Failed: {result.get('error', 'unknown error')}")

    def do_status(self, arg: str = ""):
        """%status - Show current connection status."""
        if self.mode == "listen":
            print(f"Mode: listen (port {self.listen_port})")
            print(f"Server: {'running' if self.server else 'stopped'}")
        else:
            print(f"Mode: connect")
            print(f"Connected: {self.connected}")
            if self.connected:
                print(f"Host: {self.host}:{self.port}")

    def do_help(self, arg: str = ""):
        """%help [command] - Show help for commands."""
        commands = {
            'connect': self.do_connect,
            'disconnect': self.do_disconnect,
            'create_relay': self.do_create_relay,
            'delete_relay': self.do_delete_relay,
            'list_relays': self.do_list_relays,
            'listen': self.do_listen,
            'stop_listening': self.do_stop_listening,
            'agents': self.do_agents,
            'status': self.do_status,
            'exit': self.do_exit,
            'help': self.do_help,
        }

        if arg:
            cmd = commands.get(arg)
            if cmd:
                print(cmd.__doc__)
            else:
                print(f"Unknown command: {arg}")
            return

        print("Available commands:")
        print("-" * 50)
        for name, method in commands.items():
            if method.__doc__:
                doc = method.__doc__.split('\n')[0].strip()
                print(f"  {name:<20} {doc}")
        print("-" * 50)
        print("Tip: Use plain Python expressions for interactive access:")
        print("  client.create_relay('0.0.0.0', 2222, 'tcp', '127.0.0.1', 3333, 'udp')")
        print("  cli.do_connect('127.0.0.1', 4444)")

    def do_exit(self, arg: str = ""):
        """%exit - Exit the CLI."""
        if self.client:
            self.client.disconnect()
        print("Goodbye!")
        sys.exit(0)

    # Aliases
    def do_quit(self, arg: str = ""):
        """%quit - Exit the CLI."""
        self.do_exit(arg)


def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(description="Wraith Tunnel Control CLI")
    parser.add_argument("--host", default="127.0.0.1", help="Wraith server host")
    parser.add_argument("--port", type=int, default=4444, help="Wraith server port")
    parser.add_argument("--listen", action="store_true", help="Listen mode (receive connections from wraith)")
    parser.add_argument("--listen-port", type=int, default=4445, help="Port to listen on for wraith agents")
    args = parser.parse_args()

    cli = WraithCLI(
        mode="listen" if args.listen else "connect",
        host=args.host,
        port=args.port,
        listen_port=args.listen_port
    )

    if args.listen:
        cli.do_listen(str(args.listen_port))

    cli.run_shell()


if __name__ == "__main__":
    main()
