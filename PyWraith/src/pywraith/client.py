"""PyWraith TCP client for sending commands to wraith."""

import socket
import uuid
import time
from typing import Optional, Tuple, Dict, Any

from .protocol import WraithProtocol
from .proto_gen import wraith_pb2 as pb


class WraithClient:
    """TCP client for communicating with wraith."""

    def __init__(self, host: str, port: int):
        self.host = host
        self.port = port
        self.socket: Optional[socket.socket] = None

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

        command_id = str(uuid.uuid4())
        msg = WraithProtocol.create_command(command_id, action, params, timeout)
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
        forward_host: str,
        forward_port: int
    ) -> Tuple[bool, Dict[str, Any]]:
        """Create a relay."""
        params = {
            'listen_host': listen_host,
            'listen_port': str(listen_port),
            'forward_host': forward_host,
            'forward_port': str(forward_port),
        }
        return self.send_command('create_relay', params)

    def delete_relay(self, relay_id: str) -> Tuple[bool, Dict[str, Any]]:
        """Delete a relay by ID."""
        params = {'relay_id': relay_id}
        return self.send_command('delete_relay', params)

    def list_relays(self) -> Tuple[bool, Dict[str, Any]]:
        """List all relays."""
        return self.send_command('list_relays', {})

    def __enter__(self):
        self.connect()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.disconnect()