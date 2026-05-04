"""System integration tests for wraith + pywraith.

Tests the full integration: pywraith client sending commands to wraith agent.
These tests require a running wraith instance which is provided by pytest fixtures.
"""

import json
import socket
import subprocess
import time

import pytest


def get_free_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class TestWraithConnection:
    """Test basic connection and registration."""

    def test_wraith_accepts_connection(self, wraith_client):
        """Wraith should accept a TCP connection."""
        client = wraith_client
        assert client.connect(), "Failed to connect to wraith"


class TestRelayCommands:
    """Test relay management commands via pywraith."""

    def test_create_tcp_relay_with_hop_format(self, wraith_client, wraith_process):
        """Should be able to create a TCP relay using hop format and get a relay_id back."""
        client = wraith_client
        client.connect()

        try:
            # Use hop_0_* params which is the format wraith expects
            success, result = client.create_relay(
                listen_host="127.0.0.1",
                listen_port=16001,
                listen_protocol="tcp",
                forward_host="127.0.0.1",
                forward_port=19999,
                forward_protocol="tcp",
            )

            assert success, f"Command failed: {result}"
            assert result["status"] == "success", f"Unexpected status: {result}"
            assert result["exit_code"] == 0
            assert result["output"], "Expected relay_id in output"

            # Clean up - delete the relay
            relay_id = result["output"]
            client.delete_relay(relay_id)
        finally:
            client.disconnect()

    def test_list_relays_empty(self, wraith_client, wraith_process):
        """list_relays should return empty list on fresh wraith."""
        client = wraith_client
        client.connect()

        try:
            success, result = client.list_relays()

            assert success, f"Command failed: {result}"
            assert result["status"] == "success"
            assert result["exit_code"] == 0
            # Output should be valid JSON (empty array)
            assert result["output"] == "[]"
        finally:
            client.disconnect()

    def test_list_relays_after_create(self, wraith_client, wraith_process):
        """list_relays should show created relays and the relay is cleaned up after."""
        client = wraith_client
        client.connect()

        try:
            # Create a relay first using hop format
            success, create_result = client.create_relay(
                listen_host="127.0.0.1",
                listen_port=16002,
                listen_protocol="tcp",
                forward_host="127.0.0.1",
                forward_port=19998,
                forward_protocol="tcp",
            )
            assert success, "create_relay failed"
            relay_id = create_result["output"]

            # List relays
            success, result = client.list_relays()
            assert success
            assert result["status"] == "success"
            # Should contain JSON array with our relay
            output = result["output"]
            assert relay_id in output, f"Expected relay {relay_id} in output: {output}"

            # Clean up
            client.delete_relay(relay_id)
        finally:
            client.disconnect()

    def test_delete_relay(self, wraith_client, wraith_process):
        """Should be able to delete a created relay."""
        client = wraith_client
        client.connect()

        try:
            # Create a relay with a valid (non-zero) port
            success, create_result = client.create_relay(
                listen_host="127.0.0.1",
                listen_port=16004,
                listen_protocol="tcp",
                forward_host="127.0.0.1",
                forward_port=19997,
                forward_protocol="tcp",
            )
            assert success, f"create_relay failed: {create_result}"
            relay_id = create_result["output"]

            # Delete it
            success, delete_result = client.delete_relay(relay_id)
            assert success, f"Delete command failed: {delete_result}"
            assert delete_result["status"] == "success", f"Delete returned non-success: {delete_result}"
        finally:
            client.disconnect()

    def test_delete_relay_not_found(self, wraith_client, wraith_process):
        """Deleting a non-existent relay should return not_found status."""
        client = wraith_client
        client.connect()

        try:
            success, result = client.delete_relay("non-existent-id-12345")
            assert success, f"Command failed: {result}"
            assert result["status"] == "not_found"
        finally:
            client.disconnect()


class TestAgentCommands:
    """Test agent commands via pywraith."""

    def test_set_id(self, wraith_client, wraith_process):
        """Should be able to set wraith_id at runtime."""
        client = wraith_client
        client.connect()

        try:
            success, result = client.set_id("test-wraith-123")

            assert success, f"Command failed: {result}"
            assert result["status"] == "success"
            assert result["output"] == "test-wraith-123"
        finally:
            client.disconnect()

    def test_list_peers_broadcasts_with_empty_peer_table(self, wraith_client, wraith_process):
        """list_peers on a standalone wraith broadcasts and returns broadcast status."""
        client = wraith_client
        client.connect()

        try:
            success, result = client.list_peers()

            assert success, f"Command failed: {result}"
            # With no direct peers, broadcasts to all peers and returns broadcast status
            assert result["status"] == "broadcast", f"Expected broadcast status but got: {result}"
        finally:
            client.disconnect()


class TestPeerTunnel:
    """Test wraith-to-wraith peer tunnel establishment and list_peers."""

    def test_tunnel_list_peers_after_connect(
        self,
        wraith_client,
        wraith_process,
        wraith_binary_path,
        wraith_peer_id,
        wraith_peer_port,
    ):
        """Establish a tunnel from wraith to wraith_peer, then list_peers shows the peer.

        Flow:
        1. Main wraith is running in C2 listen mode (session-scoped fixture)
        2. Send wraith_listen command to main wraith to start peer listener on wraith_peer_port
        3. Spawn secondary wraith with --agent-connect to connect to main wraith's peer port
        4. Poll list_peers on main wraith until peer appears
        """
        wraith_client.connect()

        # Step 1: Tell main wraith to start listening for peer connections
        success, result = wraith_client.wraith_listen(port=wraith_peer_port)
        assert success, f"wraith_listen failed: {result}"

        # Wait for peer listener to be ready before spawning peer
        time.sleep(2)

        # Step 2: Spawn secondary wraith that connects via agent-connect
        peer_proc = subprocess.Popen(
            [
                wraith_binary_path,
                "--wraith-id", wraith_peer_id,
                "--agent-connect", f"127.0.0.1:{wraith_peer_port}",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        try:
            # Poll list_peers until peer shows up (peer connection is async)
            peer_seen = False
            last_status = None
            for attempt in range(15):
                time.sleep(2)
                success, list_result = wraith_client.list_peers()
                last_status = list_result.get("status") if success else "connection_failed"
                if success and list_result["status"] == "success":
                    output = json.loads(list_result["output"])
                    peer_ids = [p["wraith_id"] for p in output.get("peers", [])]
                    if wraith_peer_id in peer_ids:
                        peer_seen = True
                        break
                # If broadcast or empty, peer not ready yet - retry

            assert peer_seen, (
                f"Expected peer {wraith_peer_id} not found after 15 retries. "
                f"Last status: {last_status}, final result: {list_result if 'list_result' in dir() else 'no result'}"
            )
        finally:
            peer_proc.terminate()
            try:
                peer_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                peer_proc.kill()
                peer_proc.wait()
            wraith_client.disconnect()
