"""Tests for PyWraith protocol module."""

import pytest
from PyWraith.protocol import WraithProtocol


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
