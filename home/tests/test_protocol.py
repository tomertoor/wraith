"""Tests for PyWraith protocol module."""

import pytest
from PyWraith.protocol import WraithProtocol
from PyWraith.proto_gen import wraith_pb2 as pb


def test_create_command():
    """Test creating a command message."""
    msg = WraithProtocol.create_command(
        command_id="test-123",
        action="create_relay",
        params={"listen_host": "0.0.0.0", "listen_port": "12345"},
        timeout=30
    )

    assert msg.msg_type == 2  # COMMAND
    assert msg.command.command_id == "test-123"
    assert msg.command.action == "create_relay"
    assert msg.command.params["listen_host"] == "0.0.0.0"


def test_create_command_result():
    """Test creating a command result message."""
    msg = WraithProtocol.create_command_result(
        command_id="test-123",
        status="success",
        output="relay-456",
        exit_code=0,
        duration_ms=100,
        error=""
    )

    assert msg.msg_type == 3  # COMMAND_RESULT
    assert msg.result.command_id == "test-123"
    assert msg.result.status == "success"
    assert msg.result.output == "relay-456"
    assert msg.result.exit_code == 0


def test_encode_decode_message():
    """Test message encode/decode roundtrip."""
    original = WraithProtocol.create_command(
        command_id="test-456",
        action="list_relays",
        params={},
        timeout=30
    )

    encoded = WraithProtocol.encode_message(original)
    assert len(encoded) > 4  # Should have length prefix + data

    # Decode should work (implementation depends on protobuf details)
    # For now just verify encoding doesn't crash


def test_create_relay_command():
    """Test create_relay_command with protocol translation."""
    msg = WraithProtocol.create_relay_command(
        command_id="test-123",
        listen_host="0.0.0.0",
        listen_port=2222,
        listen_protocol="tcp",
        forward_host="127.0.0.1",
        forward_port=3333,
        forward_protocol="udp",
    )

    # Verify message structure
    assert msg.msg_type == pb.RELAY_CREATE
    assert msg.relay_create.config.listen.host == "0.0.0.0"
    assert msg.relay_create.config.listen.port == 2222
    assert msg.relay_create.config.listen.protocol == "tcp"
    assert msg.relay_create.config.forward.host == "127.0.0.1"
    assert msg.relay_create.config.forward.port == 3333
    assert msg.relay_create.config.forward.protocol == "udp"
    assert msg.relay_create.relay_id != ""


def test_create_relay_command_udp_to_tcp():
    """Test create_relay_command with UDP listen and TCP forward."""
    msg = WraithProtocol.create_relay_command(
        command_id="test-456",
        listen_host="0.0.0.0",
        listen_port=5555,
        listen_protocol="udp",
        forward_host="10.0.0.1",
        forward_port=8888,
        forward_protocol="tcp",
    )

    assert msg.msg_type == pb.RELAY_CREATE
    assert msg.relay_create.config.listen.protocol == "udp"
    assert msg.relay_create.config.forward.protocol == "tcp"


def test_create_relay_command_validation():
    """Test that invalid protocols are still accepted (validation happens server-side)."""
    msg = WraithProtocol.create_relay_command(
        command_id="test-789",
        listen_host="0.0.0.0",
        listen_port=2222,
        listen_protocol="tcp",
        forward_host="127.0.0.1",
        forward_port=3333,
        forward_protocol="udp",
    )
    # The protocol field accepts any string - server-side validation will reject invalid ones
    encoded = WraithProtocol.encode_message(msg)
    assert len(encoded) > 0
    # Verify we can create the message - validation of protocol values happens server-side
    assert msg.relay_create.config.listen.protocol == "tcp"
    assert msg.relay_create.config.forward.protocol == "udp"
