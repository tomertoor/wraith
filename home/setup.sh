#!/bin/bash
set -e

cd "$(dirname "$0")"

echo "Installing PyWraith..."
pip install -e .