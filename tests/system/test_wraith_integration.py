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


def _spawn_peer_wraith(wraith_binary_path, wraith_peer_id, wraith_peer_port):
    """Spawn a secondary wraith process that connects to the main wraith's peer listener.

    Returns the subprocess.Popen object. Caller is responsible for cleanup.
    """
    return subprocess.Popen(
        [
            wraith_binary_path,
            "--wraith-id", wraith_peer_id,
            "--agent-connect", f"127.0.0.1:{wraith_peer_port}",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def _cleanup_process(proc):
    """Terminate a subprocess gracefully, falling back to kill."""
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


def _wait_for_peer(client, expected_peer_id, max_attempts=15, interval=2):
    """Poll list_peers until the expected peer appears with 'success' status.

    Returns (success: bool, last_result: dict).
    """
    for _ in range(max_attempts):
        time.sleep(interval)
        success, list_result = client.list_peers()
        if success and list_result["status"] == "success":
            output = json.loads(list_result["output"])
            peer_ids = [p["wraith_id"] for p in output.get("peers", [])]
            if expected_peer_id in peer_ids:
                return True, list_result
    return False, list_result


class TestPeerCommandRouting:
    """Test command routing through wraith-to-wraith peer tunnels.

    Topology: PyWraith -> Wraith A (C2 listen + agent listen) -> Wraith B (agent connect to A)
    """

    def _establish_peer_tunnel(
        self, client, wraith_binary_path, wraith_peer_id, wraith_peer_port
    ):
        """Helper: tell A to listen for peers, then spawn B to connect.

        Returns the peer subprocess. Caller must clean up in a finally block.
        """
        # Start peer listener on main wraith A
        success, result = client.wraith_listen(port=wraith_peer_port)
        assert success, f"wraith_listen failed: {result}"

        # Wait for listener to be ready
        time.sleep(2)

        # Spawn B connecting to A's peer listener
        peer_proc = _spawn_peer_wraith(wraith_binary_path, wraith_peer_id, wraith_peer_port)

        # Wait until A sees B as a direct peer
        found, last_result = _wait_for_peer(client, wraith_peer_id)
        if not found:
            _cleanup_process(peer_proc)
            pytest.fail(
                f"Peer {wraith_peer_id} never appeared in A's peer table. "
                f"Last list_peers result: {last_result}"
            )

        return peer_proc

    def test_list_peers_returns_success_after_peer_connect(
        self,
        wraith_client,
        wraith_process,
        wraith_binary_path,
        wraith_peer_id,
        wraith_peer_port,
    ):
        """After A<->B tunnel is established, list_peers on A returns 'success' not 'broadcast'.

        This verifies the peer_table is populated correctly so direct routing works
        instead of falling through to broadcast.
        """
        wraith_client.connect()

        try:
            peer_proc = self._establish_peer_tunnel(
                wraith_client, wraith_binary_path, wraith_peer_id, wraith_peer_port
            )

            try:
                # Now list_peers on A should return success (not broadcast)
                success, result = wraith_client.list_peers()
                assert success, f"list_peers command failed: {result}"
                assert result["status"] == "success", (
                    f"Expected 'success' status but got '{result['status']}'. "
                    f"This indicates the peer_table is not populated correctly. "
                    f"Full result: {result}"
                )

                # Verify the peer appears in the output
                output = json.loads(result["output"])
                peer_ids = [p["wraith_id"] for p in output.get("peers", [])]
                assert wraith_peer_id in peer_ids, (
                    f"Expected peer {wraith_peer_id} in peer list, got: {peer_ids}"
                )
            finally:
                _cleanup_process(peer_proc)
        finally:
            wraith_client.disconnect()

    def test_command_routes_through_peer(
        self,
        wraith_client,
        wraith_process,
        wraith_binary_path,
        wraith_id,
        wraith_peer_id,
        wraith_peer_port,
    ):
        """Send a command targeted at B through A; verify direct routing to B succeeds.

        When the client sets target to B's wraith_id and sends list_peers, A should
        look up B in its peer_table and forward directly, returning B's response
        (not a broadcast acknowledgement).
        """
        wraith_client.connect()

        try:
            peer_proc = self._establish_peer_tunnel(
                wraith_client, wraith_binary_path, wraith_peer_id, wraith_peer_port
            )

            try:
                # Now target B directly through A
                wraith_client.set_target(wraith_peer_id)

                # Send list_peers targeted at B
                success, result = wraith_client.list_peers()
                assert success, f"list_peers routed to B failed: {result}"
                assert result["status"] == "success", (
                    f"Expected 'success' from direct routing to B, "
                    f"but got '{result['status']}'. "
                    f"This means A is not doing direct peer lookup and fell through to broadcast. "
                    f"Full result: {result}"
                )

                # B's peer list should contain A's wraith_id (its only neighbor)
                output = json.loads(result["output"])
                peer_ids = [p["wraith_id"] for p in output.get("peers", [])]
                assert wraith_id in peer_ids, (
                    f"Expected A's id '{wraith_id}' in B's peer list, got: {peer_ids}"
                )
            finally:
                _cleanup_process(peer_proc)
        finally:
            wraith_client.disconnect()

    def test_set_id_routes_to_remote_peer(
        self,
        wraith_client,
        wraith_process,
        wraith_binary_path,
        wraith_peer_id,
        wraith_peer_port,
    ):
        """Send set_id command to B through A; verify the command reaches B and executes.

        This proves that command routing through the peer tunnel correctly forwards
        the command to the remote wraith and returns its response.
        """
        wraith_client.connect()

        try:
            peer_proc = self._establish_peer_tunnel(
                wraith_client, wraith_binary_path, wraith_peer_id, wraith_peer_port
            )

            try:
                # Target B directly
                wraith_client.set_target(wraith_peer_id)

                # Send set_id to B
                new_id = "renamed-peer"
                success, result = wraith_client.set_id(new_id)
                assert success, f"set_id routed to B failed: {result}"
                assert result["status"] == "success", (
                    f"Expected 'success' from set_id routed to B, "
                    f"but got '{result['status']}'. "
                    f"Full result: {result}"
                )
                assert result["output"] == new_id, (
                    f"Expected output '{new_id}' but got '{result['output']}'"
                )
            finally:
                _cleanup_process(peer_proc)
        finally:
            wraith_client.disconnect()
