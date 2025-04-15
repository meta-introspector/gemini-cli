#!/bin/bash
set -e

# Build the filesystem MCP server
echo "Building filesystem MCP server..."
cargo build --release

# Create directory for MCP servers in the installation directory
echo "Creating MCP servers directory..."
INSTALL_DIR="${1:-../../..}"
MCP_SERVERS_DIR="$INSTALL_DIR/mcp-servers"
mkdir -p "$MCP_SERVERS_DIR"

# Copy the binary to the MCP servers directory
echo "Copying filesystem MCP server to $MCP_SERVERS_DIR..."
cp target/release/filesystem-mcp "$MCP_SERVERS_DIR/"

# Create an example configuration
echo "Creating example configuration..."
CONFIG_FILE="$INSTALL_DIR/mcp_filesystem_config.example.json"
cat > "$CONFIG_FILE" << 'EOF'
[
  {
    "name": "filesystem",
    "enabled": true,
    "transport": "stdio",
    "command": ["./mcp-servers/filesystem-mcp"],
    "args": []
  }
]
EOF

echo "Filesystem MCP server successfully built and installed."
echo "To enable it, add its configuration from $CONFIG_FILE to your MCP servers configuration." 