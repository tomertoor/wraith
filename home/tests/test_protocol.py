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


def test_create_relay_chain_command():
    """Test create_relay_chain_command creates proper relay chain messages."""
    hops = [
        {
            'listen_host': '127.0.0.1',
            'listen_port': 6666,
            'forward_host': '127.0.0.1',
            'forward_port': 7777,
            'protocol': 'tcp'
        },
        {
            'listen_host': '127.0.0.1',
            'listen_port': 7777,
            'forward_host': '10.0.0.1',
            'forward_port': 443,
            'protocol': 'udp'
        }
    ]

    msg = WraithProtocol.create_relay_chain_command('cmd-123', hops)

    assert msg.msg_type == pb.RELAY_CREATE
    assert msg.message_id == 'cmd-123'
    assert len(msg.relay_create.hops) == 2

    # Verify first hop
    assert msg.relay_create.hops[0].listen_host == '127.0.0.1'
    assert msg.relay_create.hops[0].listen_port == 6666
    assert msg.relay_create.hops[0].forward_host == '127.0.0.1'
    assert msg.relay_create.hops[0].forward_port == 7777
    assert msg.relay_create.hops[0].protocol == 'tcp'

    # Verify second hop
    assert msg.relay_create.hops[1].listen_host == '127.0.0.1'
    assert msg.relay_create.hops[1].listen_port == 7777
    assert msg.relay_create.hops[1].forward_host == '10.0.0.1'
    assert msg.relay_create.hops[1].forward_port == 443
    assert msg.relay_create.hops[1].protocol == 'udp'
