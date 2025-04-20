#!/bin/bash
set -e

echo "Rebuilding gemini-cli-bin with improved shutdown handling..."

# Navigate to the project root
cd "$(dirname "$0")"

# Build the CLI binary
echo "Building CLI binary..."
cargo build --release --bin gemini-cli-bin

# Install the new binary
echo "Installing CLI binary..."
mkdir -p ~/.local/bin
cp target/release/gemini-cli-bin ~/.local/bin/

echo "CLI rebuilt and installed successfully."
echo "You can now test the MCP server shutdown with: gemini \"Your query\"" 