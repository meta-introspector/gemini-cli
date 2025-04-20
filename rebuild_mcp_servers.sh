#!/bin/bash
set -e

echo "Rebuilding MCP servers with shutdown handling fixes..."

# Navigate to the project root
cd "$(dirname "$0")"

# Build Rust MCP servers
echo "Building Rust MCP servers..."
cd mcp
cargo build --release

# Install built binaries to the CLI servers directory
echo "Installing Rust MCP servers..."
cp target/release/filesystem-mcp ../cli/mcp-servers/
cp target/release/command-mcp ../cli/mcp-servers/
cp target/release/memory-store-mcp ../cli/mcp-servers/

# Python embedding server doesn't need compilation, but ensure the file is executable
echo "Ensuring Python embedding server is executable..."
chmod +x ../mcp_embedding_server/server.py

echo "MCP servers rebuilt and installed successfully."
echo "You can now test the shutdown handling with: RUST_LOG=gemini_mcp=debug,gemini_cli=debug gemini \"Your query\"" 