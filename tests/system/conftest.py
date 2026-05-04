"""Pytest fixtures for wraith system integration tests.

Provides:
- wraith_process: spawns a wraith agent process
- wraith_client: provides a WraithClient connected to the spawned wraith
"""

import os
import socket
import subprocess
import sys
import time
import pytest

# Ensure PyWraith is importable - it lives under home/
_PYWRAITH_ROOT = os.path.join(os.path.dirname(__file__), "..", "..", "home")
if _PYWRAITH_ROOT not in sys.path:
    sys.path.append(_PYWRAITH_ROOT)


# Find free port
def get_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


# Path to wraith binary (built via cargo build)
WRAITH_BINARY = os.environ.get(
    "WRAITH_BINARY",
    os.path.join(os.path.dirname(__file__), "..", "..", "target", "debug", "wraith"),
)


@pytest.fixture(scope="session")
def wraith_binary_path():
    """Return path to wraith binary, building if needed."""
    binary = WRAITH_BINARY
    if not os.path.exists(binary):
        pytest.skip(f"Wraith binary not found at {binary}. Run `cargo build` first.")
    return binary


@pytest.fixture(scope="session")
def wraith_id(wraith_port):
    """Return the wraith_id used for the test wraith process."""
    return f"test-wraith-{wraith_port}"


@pytest.fixture(scope="session")
def wraith_port(wraith_binary_path):
    """Allocate a free port for the wraith session."""
    return get_free_port()


@pytest.fixture(scope="session")
def wraith_process(wraith_binary_path, wraith_id, wraith_port):
    """Spawn a wraith agent process in listen mode and clean up on teardown.

    This runs once per test session.
    """
    proc = subprocess.Popen(
        [
            wraith_binary_path,
            "--c2-host", "127.0.0.1",
            "-p", str(wraith_port),
            "--wraith-id", wraith_id,
            "--listen",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    # Wait for wraith to be ready (up to 10s)
    start = time.time()
    while time.time() - start < 10:
        try:
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.settimeout(0.5)
            result = sock.connect_ex(("127.0.0.1", wraith_port))
            sock.close()
            if result == 0:
                break
        except OSError:
            pass
        time.sleep(0.2)
    else:
        proc.terminate()
        stdout, stderr = proc.communicate(timeout=5)
        raise RuntimeError(
            f"Wraith process failed to start within 10s.\n"
            f"stdout: {stdout.decode(errors='replace')}\n"
            f"stderr: {stderr.decode(errors='replace')}"
        )

    yield proc

    # Teardown: kill wraith
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


@pytest.fixture
def wraith_client(wraith_process, wraith_id, wraith_port):
    """Provide a WraithClient connected to the test wraith process.

    Each test gets a fresh connection via context manager.
    """
    from PyWraith.client import WraithClient

    client = WraithClient("127.0.0.1", wraith_port)
    client.set_target(wraith_id)
    # Each test uses its own connection
    yield client
    # Cleanup handled by test via context manager, but ensure disconnected
    client.disconnect()


# --- Peer wraith fixtures (second wraith for tunnel tests) ---

@pytest.fixture(scope="function")
def wraith_peer_port(wraith_binary_path):
    """Allocate a free port for the peer listener (function-scoped per test)."""
    return get_free_port()


@pytest.fixture(scope="function")
def wraith_peer_id(wraith_peer_port):
    """Return the wraith_id used for the peer wraith."""
    return f"test-peer-{wraith_peer_port}"


@pytest.fixture(scope="function")
def wraith_peer_process(wraith_binary_path, wraith_peer_id, wraith_peer_port):
    """Spawn a wraith that connects to main wraith's peer listener via --agent-connect.

    This is function-scoped so the process starts AFTER wraith_listen has been sent
    to the main wraith (which sets up the peer listener).
    """
    proc = subprocess.Popen(
        [
            wraith_binary_path,
            "--wraith-id", wraith_peer_id,
            "--agent-connect", f"127.0.0.1:{wraith_peer_port}",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    yield proc

    # Teardown: kill peer wraith
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
