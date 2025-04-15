#!/bin/bash
set -e

# Build the memory MCP server
echo "Building memory MCP server..."
cargo build --release

# Create directory for MCP servers in the installation directory
echo "Creating MCP servers directory..."
INSTALL_DIR="${1:-../../..}"
MCP_SERVERS_DIR="$INSTALL_DIR/mcp-servers"
mkdir -p "$MCP_SERVERS_DIR"

# Copy the binary to the MCP servers directory
echo "Copying memory MCP server to $MCP_SERVERS_DIR..."
cp target/release/memory-mcp "$MCP_SERVERS_DIR/"

# Create an example configuration
echo "Creating example configuration..."
CONFIG_FILE="$INSTALL_DIR/mcp_memory_config.example.json"
cat > "$CONFIG_FILE" << 'EOF'
[
  {
    "name": "memory",
    "enabled": true,
    "transport": "stdio",
    "command": ["./mcp-servers/memory-mcp"],
    "args": [],
    "auto_execute": []
  }
]
EOF

echo "Memory MCP server successfully built and installed."
echo "To enable it, add its configuration from $CONFIG_FILE to your MCP servers configuration." 