"""
Binary config stamper for embedding configuration into the Wraith binary.

Uses the WRAITH_EMBED_V1 sentinel pattern: the config is serialized as JSON
and appended to the binary after a sentinel marker. The Rust binary's
embedded_config.rs reads back this marker and config at startup.

Sentinel format (matches src/wraith/embedded_config.rs):
    WRAITH_EMBED_V1 = b"WRAITH_EMBED_V1"
    [15-byte sentinel][4-byte BE config_length][JSON config bytes]

The Rust side reads from the end of the binary backwards, finds the sentinel,
and deserializes the config. This is a zero-dependency approach that works
without recompiling the binary.

Usage:
    from pywraith import stamp_binary, WRAITH_EMBED_V1

    stamp_binary("/path/to/wraith", {"relay_host": "10.0.0.1", "relay_port": 4445},
                 output_path="/path/to/wraith_stamped")
"""

import json
import os
import struct
import shutil
import tempfile
from typing import Any, Dict


# Sentinel marker matching embedded_config.rs WRAITH_EMBED_V1
WRAITH_EMBED_V1 = b"WRAITH_EMBED_V1"
SENTINEL_LEN = len(WRAITH_EMBED_V1)  # 15 bytes

# Backward-compatible alias
SHELLY_EMBED_V1 = WRAITH_EMBED_V1


def _build_config_blob(config: Dict[str, Any]) -> bytes:
    """
    Build the stamped config blob.

    Format (matches Rust embedded_config.rs):
        [15-byte WRAITH_EMBED_V1 sentinel]
        [4-byte BE config_length]
        [JSON config bytes]

    Returns:
        The full config blob to append to the binary.
    """
    json_bytes = json.dumps(config, separators=(",", ":")).encode("utf-8")
    header = struct.pack(">I", len(json_bytes))
    return WRAITH_EMBED_V1 + header + json_bytes


def stamp_binary(
    binary_path: str,
    config: Dict[str, Any],
    output_path: str,
) -> bool:
    """
    Embed configuration into a Wraith binary.

    Reads the original binary, appends the config blob, and writes to output_path.
    The output binary is a drop-in replacement that reads the embedded config at runtime.

    Args:
        binary_path: Path to the original wraith binary
        config: Configuration dict. Keys should be strings, values should be
                JSON-serializable (str, int, float, bool, list, dict).
        output_path: Path to write the stamped binary

    Returns:
        True if stamping succeeded, False otherwise.

    Raises:
        FileNotFoundError: If binary_path does not exist
        OSError: If the binary cannot be read or written
    """
    if not os.path.isfile(binary_path):
        raise FileNotFoundError(f"Binary not found: {binary_path}")

    # Validate config values are JSON-serializable
    try:
        json.dumps(config)
    except (TypeError, ValueError) as e:
        raise ValueError(f"Config is not JSON-serializable: {e}")

    # Read original binary
    try:
        with open(binary_path, "rb") as f:
            original = f.read()
    except OSError as e:
        raise OSError(f"Failed to read binary {binary_path}: {e}")

    # Build config blob
    config_blob = _build_config_blob(config)

    # Write to temp file first, then move (atomic on POSIX)
    stamped = original + config_blob

    tmp_fd, tmp_path = tempfile.mkstemp(dir=os.path.dirname(output_path) or ".")
    try:
        with os.fdopen(tmp_fd, "wb") as f:
            f.write(stamped)
        shutil.move(tmp_path, output_path)
    except OSError as e:
        # Clean up temp file if move failed
        try:
            os.unlink(tmp_path)
        except OSError:
            pass
        raise OSError(f"Failed to write stamped binary to {output_path}: {e}")

    return True


def read_stamped_config(binary_path: str) -> Dict[str, Any]:
    """
    Read embedded config from a stamped binary.

    Searches for the WRAITH_EMBED_V1 sentinel from the end of the binary
    and deserializes the config JSON.

    Args:
        binary_path: Path to a stamped wraith binary

    Returns:
        The embedded configuration dict.

    Raises:
        ValueError: If no config is embedded or if the config is malformed
        FileNotFoundError: If the binary doesn't exist
    """
    if not os.path.isfile(binary_path):
        raise FileNotFoundError(f"Binary not found: {binary_path}")

    with open(binary_path, "rb") as f:
        data = f.read()

    # Search backwards for the sentinel
    pos = data.rfind(WRAITH_EMBED_V1)
    if pos == -1:
        raise ValueError("No embedded config found in binary")

    # Read length prefix (4 bytes BE, after sentinel)
    header_start = pos + SENTINEL_LEN
    if header_start + 4 > len(data):
        raise ValueError("Embedded config header truncated")

    (config_len,) = struct.unpack(">I", data[header_start : header_start + 4])

    # Read JSON payload
    payload_start = header_start + 4
    payload_end = payload_start + config_len
    if payload_end > len(data):
        raise ValueError("Embedded config payload truncated")

    json_bytes = data[payload_start:payload_end]
    return json.loads(json_bytes.decode("utf-8"))


def probe_binary(binary_path: str) -> bool:
    """
    Check if a binary has embedded config.

    Args:
        binary_path: Path to wraith binary

    Returns:
        True if the binary contains a WRAITH_EMBED_V1 sentinel, False otherwise.
    """
    try:
        with open(binary_path, "rb") as f:
            # Only read the last 1MB to check for sentinel
            try:
                f.seek(-1024 * 1024, os.SEEK_END)
            except OSError:
                # File smaller than 1MB, read from beginning
                pass
            tail = f.read()
        return WRAITH_EMBED_V1 in tail
    except OSError:
        return False
