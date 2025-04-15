#!/bin/bash
# Build script for the command MCP server
# This compiles the server as a standalone binary

set -e

# Get directory of this script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_ROOT="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"

# Ensure we're in the crate root directory
cd "${CRATE_ROOT}"

echo "Building command MCP server..."
cargo build --release --bin command-mcp

# Where the binary should be installed
MCP_SERVER_DIR="${HOME}/.local/bin"
mkdir -p "${MCP_SERVER_DIR}"

# Copy binary to destination
echo "Installing command MCP server to ${MCP_SERVER_DIR}/command-mcp-bin"
cp "target/release/command-mcp" "${MCP_SERVER_DIR}/command-mcp-bin"

# Create wrapper script
echo "Creating wrapper script at ${MCP_SERVER_DIR}/command-mcp"
cat > "${MCP_SERVER_DIR}/command-mcp" << 'EOF'
#!/bin/bash
exec "$HOME/.local/bin/command-mcp-bin" "$@"
EOF

chmod +x "${MCP_SERVER_DIR}/command-mcp"

echo "Command MCP server installation complete" 