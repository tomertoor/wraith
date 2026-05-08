#!/bin/bash
set -e

echo "=== Wraith Integration Test ==="

# Start wraith in command listen mode
echo "Starting wraith server..."
cargo run --bin wraith -- command-listen 127.0.0.1:4444 &
WRAITH_PID=$!
sleep 2

# Check wraith is running
if ! kill -0 $WRAITH_PID 2>/dev/null; then
    echo "FAILED: wraith did not start"
    exit 1
fi

echo "Wraith started (PID: $WRAITH_PID)"

# Use pywraith to create a relay
echo "Testing create_relay command..."
PYTHONPATH=/home/user/tools/wraith/PyWraith/src python3 -c "
import sys
sys.path.insert(0, '/home/user/tools/wraith/PyWraith/src')
from pywraith.client import WraithClient
client = WraithClient('127.0.0.1', 4444)
if client.connect():
    success, result = client.create_relay('127.0.0.1', 12345, '127.0.0.1', 54321)
    print(f'Result: success={success}, output={result}')
    client.disconnect()
else:
    print('Failed to connect')
    exit(1)
"

# Cleanup
kill $WRAITH_PID 2>/dev/null || true

echo "=== Integration test complete ==="