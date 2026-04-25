"""PyWraith TCP client for sending commands to wraith."""

import socket
import uuid
import time
from typing import Optional, Tuple, Dict, Any

from PyWraith.protocol import WraithProtocol
from PyWraith.proto_gen import wraith_pb2 as pb


class WraithClient:
    """TCP client for communicating with wraith."""

    def __init__(self, host: str, port: int):
        self.host = host
        self.port = port
        self.socket: Optional[socket.socket] = None
        self._target: str = ""

    def set_target(self, wraith_id: str):
        """Set the global routing target."""
        self._target = wraith_id

    def get_target(self) -> str:
        """Get current target."""
        return self._target

    def clear_target(self):
        """Clear the routing target."""
        self._target = ""

    def connect(self) -> bool:
        """Connect to wraith server."""
        try:
            self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            self.socket.settimeout(5.0)
            self.socket.connect((self.host, self.port))
            return True
        except Exception as e:
            return False

    def disconnect(self):
        """Disconnect from wraith server."""
        if self.socket:
            self.socket.close()
            self.socket = None

    def send_command(
        self,
        action: str,
        params: Optional[Dict[str, str]] = None,
        timeout: int = 30
    ) -> Tuple[bool, Dict[str, Any]]:
        """
        Send a command to wraith and wait for response.
        Returns (success, result_dict).
        """
        if not self.socket:
            return False, {'error': 'Not connected'}

        if not self._target:
            return False, {'error': 'No target wraith set. Use set_target(<id>) first.'}

        command_id = str(uuid.uuid4())
        msg = WraithProtocol.create_command(command_id, action, params, timeout, target_wraith_id=self._target)
        data = WraithProtocol.encode_message(msg)

        try:
            self.socket.sendall(data)

            # Wait for response
            start = time.time()
            while time.time() - start < timeout:
                response_data = WraithProtocol.read_frame(self.socket)
                if response_data:
                    response = WraithProtocol.decode_message(response_data)
                    if response.msg_type == pb.COMMAND_RESULT:
                        return True, WraithProtocol.parse_command_result(response)
                time.sleep(0.1)

            return False, {'error': 'Timeout waiting for response'}
        except Exception as e:
            return False, {'error': str(e)}

    def create_relay(
        self,
        listen_host: str,
        listen_port: int,
        listen_protocol: str,
        forward_host: str,
        forward_port: int,
        forward_protocol: str,
    ) -> Tuple[bool, Dict[str, Any]]:
        """Create a relay with protocol translation support.

        Args:
            listen_host: Host to bind for listening
            listen_port: Port to bind for listening
            listen_protocol: "tcp" or "udp"
            forward_host: Host to forward connections to
            forward_port: Port to forward connections to
            forward_protocol: "tcp" or "udp"

        Returns:
            Tuple[bool, Dict[str, Any]]: (success, result)
        """
        params = {
            'listen_host': listen_host,
            'listen_port': str(listen_port),
            'listen_protocol': listen_protocol,
            'forward_host': forward_host,
            'forward_port': str(forward_port),
            'forward_protocol': forward_protocol,
        }
        return self.send_command('create_relay', params)

    def delete_relay(self, relay_id: str) -> Tuple[bool, Dict[str, Any]]:
        """Delete a relay by ID."""
        params = {'relay_id': relay_id}
        return self.send_command('delete_relay', params)

    def list_relays(self) -> Tuple[bool, Dict[str, Any]]:
        """List all relays."""
        return self.send_command('list_relays', {})

    def set_id(self, wraith_id: str) -> Tuple[bool, Dict[str, Any]]:
        """Set the wraith's ID."""
        params = {'wraith_id': wraith_id}
        return self.send_command('set_id', params)

    def list_peers(self) -> Tuple[bool, Dict[str, Any]]:
        """List direct peer wraiths."""
        return self.send_command('list_peers', {})

    def list_peers_recursive(self, timeout: int = 30) -> Tuple[bool, Dict[str, Any]]:
        """List all peers recursively by traversing the network.

        Returns hierarchical view of wraith network.
        """
        # First get our direct peers
        success, result = self.list_peers()
        if not success:
            return success, result

        network = {
            'self': result.get('wraith_id'),
            'peers': result.get('peers', []),
        }

        return True, network

    def wraith_listen(self, port: int = 4445) -> Tuple[bool, Dict[str, Any]]:
        """Start listening for peer wraith connections on a port.

        Args:
            port: Port to listen on for peer connections

        Returns:
            Tuple[bool, Dict[str, Any]]: (success, result)
        """
        params = {'port': str(port)}
        return self.send_command('wraith_listen', params)

    def wraith_connect(self, host: str, port: int = 4445) -> Tuple[bool, Dict[str, Any]]:
        """Connect to a peer wraith.

        Args:
            host: Host to connect to
            port: Port to connect to

        Returns:
            Tuple[bool, Dict[str, Any]]: (success, result)
        """
        params = {'host': host, 'port': str(port)}
        return self.send_command('wraith_connect', params)

    def __enter__(self):
        self.connect()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.disconnect()