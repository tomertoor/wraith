#!/bin/bash
set -e

echo "=== PyWraith Setup ==="
echo ""

WRAITH_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$WRAITH_DIR"

# Check for protoc
if ! command -v protoc &> /dev/null; then
    echo "[!] protoc not found. Installing..."
    if command -v apt-get &> /dev/null; then
        sudo apt-get update && sudo apt-get install -y protobuf-compiler
    elif command -v yum &> /dev/null; then
        sudo yum install -y protobuf-compiler
    elif command -v brew &> /dev/null; then
        brew install protobuf
    else
        echo "[!] Cannot install protoc automatically. Please install protobuf-compiler manually."
        exit 1
    fi
fi

echo "[+] protoc found: $(protoc --version)"

# Create virtual environment
if [ ! -d "venv" ]; then
    echo "[+] Creating virtual environment..."
    python3 -m venv venv
else
    echo "[+] Virtual environment already exists"
fi

# Activate venv
echo "[+] Activating virtual environment..."
source venv/bin/activate

# Install dependencies
echo "[+] Installing dependencies..."
pip install --upgrade pip
pip install -e .

# Verify installation
echo ""
echo "[+] Verifying installation..."
python -c "from pywraith import WraithServer; print('    Import successful!')"

echo ""
echo "=== Setup Complete ==="
echo ""
echo "To activate the environment, run:"
echo "    source venv/bin/activate"
echo ""
echo "To start the interactive CLI:"
echo "    wraith"
echo ""
