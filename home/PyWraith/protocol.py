"""PyWraith protocol handling - protobuf encode/decode and message helpers."""

import struct
import time
import uuid
from typing import Optional, Dict, Any, List

from PyWraith.proto_gen import wraith_pb2 as pb


class WraithProtocol:
    """Pure protocol handling - no I/O, no session management."""

    @staticmethod
    def encode_message(msg: pb.WraithMessage) -> bytes:
        """Encode a protobuf message with 4-byte length prefix (big-endian)."""
        data = msg.SerializeToString()
        return struct.pack('>I', len(data)) + data

    @staticmethod
    def decode_message(data: bytes) -> pb.WraithMessage:
        """Decode a protobuf message from raw bytes."""
        msg = pb.WraithMessage()
        msg.ParseFromString(data)
        return msg

    @staticmethod
    def read_frame(sock) -> Optional[bytes]:
        """Read a length-prefixed frame from a blocking socket."""
        try:
            len_data = sock.recv(4)
            if not len_data or len(len_data) < 4:
                return None

            msg_len = struct.unpack('>I', len_data)[0]
            data = b''
            while len(data) < msg_len:
                chunk = sock.recv(msg_len - len(data))
                if not chunk:
                    return None
                data += chunk

            return data
        except Exception:
            return None

    @staticmethod
    def create_command(
        command_id: str,
        action: str,
        params: Optional[Dict[str, str]] = None,
        timeout: int = 30
    ) -> pb.WraithMessage:
        """Create a COMMAND message."""
        cmd = pb.Command(
            command_id=command_id,
            action=action,
            params=params or {},
            timeout=timeout
        )
        msg = pb.WraithMessage(
            msg_type=pb.COMMAND,
            message_id=command_id,
            timestamp=int(time.time() * 1000),
            command=cmd
        )
        return msg

    @staticmethod
    def create_command_result(
        command_id: str,
        status: str,
        output: str = '',
        exit_code: int = 0,
        duration_ms: int = 0,
        error: str = ''
    ) -> pb.WraithMessage:
        """Create a COMMAND_RESULT message."""
        result = pb.CommandResult(
            command_id=command_id,
            status=status,
            output=output,
            exit_code=exit_code,
            duration_ms=duration_ms,
            error=error
        )
        msg = pb.WraithMessage(
            msg_type=pb.COMMAND_RESULT,
            message_id=command_id,
            timestamp=int(time.time() * 1000),
            result=result
        )
        return msg

    @staticmethod
    def create_registration(
        hostname: str,
        username: str,
        os: str,
        ip_address: str
    ) -> pb.WraithMessage:
        """Create REGISTRATION message."""
        registration = pb.Registration(
            hostname=hostname,
            username=username,
            os=os,
            ip_address=ip_address
        )
        msg = pb.WraithMessage(
            msg_type=pb.REGISTRATION,
            message_id=str(uuid.uuid4()),
            timestamp=int(time.time() * 1000),
            registration=registration
        )
        return msg

    @staticmethod
    def create_relay_chain_command(
        command_id: str,
        hops: List[Dict],
        timeout: int = 30
    ) -> pb.WraithMessage:
        """Create a relay chain command with multiple hops.

        Args:
            command_id: Unique command identifier
            hops: List of dicts, each containing:
                - listen_host: str
                - listen_port: int
                - forward_host: str
                - forward_port: int
                - protocol: str ("tcp" or "udp")
            timeout: Command timeout in seconds

        Returns:
            pb.WraithMessage with RELAY_CREATE payload containing hops
        """
        # Validate required hop keys
        required_keys = {'listen_host', 'listen_port', 'forward_host', 'forward_port', 'protocol'}
        for i, hop in enumerate(hops):
            missing = required_keys - set(hop.keys())
            if missing:
                raise ValueError(f"Hop {i} missing required keys: {missing}")

        msg = pb.WraithMessage()
        msg.msg_type = pb.RELAY_CREATE
        msg.message_id = command_id
        msg.timestamp = int(time.time() * 1000)

        relay_create = pb.RelayCreate()
        relay_create.relay_id = str(uuid.uuid4())

        for hop in hops:
            h = pb.RelayHop()
            h.listen_host = hop['listen_host']
            h.listen_port = hop['listen_port']
            h.forward_host = hop['forward_host']
            h.forward_port = hop['forward_port']
            h.protocol = hop['protocol']
            relay_create.hops.append(h)

        msg.relay_create.CopyFrom(relay_create)
        return msg

    @staticmethod
    def parse_command_result(msg: pb.WraithMessage) -> Dict[str, Any]:
        """Extract command result fields as a dict."""
        result = msg.result
        return {
            'command_id': result.command_id,
            'status': result.status,
            'output': result.output,
            'exit_code': result.exit_code,
            'duration_ms': result.duration_ms,
            'error': result.error,
            'message_id': msg.message_id,
            'timestamp': msg.timestamp,
        }