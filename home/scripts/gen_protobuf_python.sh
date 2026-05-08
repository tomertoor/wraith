#!/bin/bash
set -e

# Generate Python protobuf files for PyWraith
# Usage: ./scripts/gen_protobuf_python.sh [proto_file] [output_dir]

PROTO_DIR="${1:-../proto}"
OUTPUT_DIR="${2:-PyWraith/proto_gen}"
PROTO_FILE="$PROTO_DIR/wraith.proto"

echo "=== Wraith Protobuf Generator (Python) ==="
echo "Proto file: $PROTO_FILE"
echo "Output dir: $OUTPUT_DIR"

# Check dependencies
if ! command -v protoc &> /dev/null; then
    echo "Error: protoc is not installed"
    echo "Install with: apt install protobuf-compiler  # or brew install protobuf on macOS"
    exit 1
fi

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"

# Check if in a virtualenv or has protobuf installed
PYTHON_BIN="python3"
VENV_PYTHON=""
if [ -d ".venv" ] && [ -f ".venv/bin/python" ]; then
    VENV_PYTHON=".venv/bin/python"
elif [ -d ".venv" ] && [ -f "PyWraith/.venv/bin/python" ]; then
    VENV_PYTHON=".venv/bin/python"
fi

if [ -n "$VENV_PYTHON" ]; then
    PYTHON_BIN="$VENV_PYTHON"
    echo "Using virtualenv: $VENV_PYTHON"
fi

if ! $PYTHON_BIN -c "import google.protobuf" 2>/dev/null; then
    echo "Installing protobuf Python runtime..."
    $PYTHON_BIN -m pip install protobuf
fi

# Generate
echo "Generating Python protobuf files..."
protoc \
    --python_out="$OUTPUT_DIR" \
    --proto_path="$PROTO_DIR" \
    "$PROTO_FILE"

# Rename output to follow Python conventions
PROTO_OUT="$OUTPUT_DIR/wraith_pb2.py"
if [ -f "$PROTO_OUT" ]; then
    echo "Generated: $PROTO_OUT"

    # Create __init__.py if it doesn't exist
    INIT_FILE="$OUTPUT_DIR/__init__.py"
    if [ ! -f "$INIT_FILE" ]; then
        echo '# PyWraith protobuf package' > "$INIT_FILE"
        echo "Created: $INIT_FILE"
    fi

    echo "Done!"
else
    echo "Error: Expected output file $PROTO_OUT was not created"
    exit 1
fi
