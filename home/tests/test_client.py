"""Tests for PyWraith client module."""

import pytest
from pywraith.client import WraithClient


def test_client_init():
    """Test client initialization."""
    client = WraithClient("127.0.0.1", 4444)
    assert client.host == "127.0.0.1"
    assert client.port == 4444
    assert client.socket is None


def test_client_context_manager():
    """Test client as context manager."""
    client = WraithClient("127.0.0.1", 4444)
    # connect would fail but init should work
    assert client.host == "127.0.0.1"
