#!/bin/bash
set -e

# Get the root directory of the project (where the script is located)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$SCRIPT_DIR"
echo "Setting up MCP server wrappers in workspace: $WORKSPACE_DIR"

# Expect the CLI binary name as the first argument
if [ -z "$1" ]; then
  echo "Error: CLI binary name not provided as the first argument." >&2
  exit 1
fi
CLI_BINARY_NAME="$1"
echo "Using CLI binary name: $CLI_BINARY_NAME"

# Create directory for temporary MCP server wrappers
echo "Creating temporary wrapper directory..."
TEMP_WRAPPER_DIR="$WORKSPACE_DIR/mcp-servers-tmp"
mkdir -p "$TEMP_WRAPPER_DIR"

# Define the target binary directory
BINARY_DIR="$HOME/.local/bin"
mkdir -p "$BINARY_DIR"

# Define installation path for MCP servers (wrappers)
MCP_WRAPPER_INSTALL_PATH="$BINARY_DIR/mcp-servers"

# Define the path for the main CLI binary using the argument
GEMINI_CLI_BIN_PATH="$BINARY_DIR/$CLI_BINARY_NAME"

# Define configuration directories
UNIFIED_CONFIG_DIR="$HOME/.config/gemini-suite"
mkdir -p "$UNIFIED_CONFIG_DIR"

# --- Built-in Rust Servers ---

# Create a simple wrapper script for the filesystem MCP
echo "Creating filesystem-mcp wrapper script..."
FILESYSTEM_WRAPPER_SCRIPT="$TEMP_WRAPPER_DIR/filesystem-mcp"

# Use unquoted EOF to expand variables like GEMINI_CLI_BIN_PATH *now*
# Escape $ for variables that should be evaluated *when the wrapper runs*
cat > "$FILESYSTEM_WRAPPER_SCRIPT" << EOF
#!/bin/bash
# This is a wrapper script for the filesystem MCP server
# It forwards calls to the $CLI_BINARY_NAME binary

# Path to the main binary (Resolved during install script run)
GEMINI_CLI="$GEMINI_CLI_BIN_PATH"

# Check if the binary exists
if [ ! -x "$GEMINI_CLI" ]; then
    echo "Error: $CLI_BINARY_NAME not found at $GEMINI_CLI" >&2
    exit 1
fi

# Forward all input to the binary with the --filesystem-mcp flag
GEMINI_CONFIG_DIR="$UNIFIED_CONFIG_DIR" exec "$GEMINI_CLI" --filesystem-mcp "$@"
EOF

# Make the wrapper script executable
chmod +x "$FILESYSTEM_WRAPPER_SCRIPT"

# Create a simple wrapper script for the command MCP
echo "Creating command-mcp wrapper script..."
COMMAND_WRAPPER_SCRIPT="$TEMP_WRAPPER_DIR/command-mcp"

# Use unquoted EOF
cat > "$COMMAND_WRAPPER_SCRIPT" << EOF
#!/bin/bash
# This is a wrapper script for the command MCP server
# It forwards calls to the $CLI_BINARY_NAME binary

# Path to the main binary (Resolved during install script run)
GEMINI_CLI="$GEMINI_CLI_BIN_PATH"

# Check if the binary exists
if [ ! -x "$GEMINI_CLI" ]; then
    echo "Error: $CLI_BINARY_NAME not found at $GEMINI_CLI" >&2
    exit 1
fi

# Forward all input to the binary with the --command-mcp flag
GEMINI_CONFIG_DIR="$UNIFIED_CONFIG_DIR" exec "$GEMINI_CLI" --command-mcp "$@"
EOF

# Make the wrapper script executable
chmod +x "$COMMAND_WRAPPER_SCRIPT"

# Create a simple wrapper script for the memory store MCP
echo "Creating memory-store-mcp wrapper script..."
MEMORY_STORE_WRAPPER_SCRIPT="$TEMP_WRAPPER_DIR/memory-store-mcp"

# Use unquoted EOF
cat > "$MEMORY_STORE_WRAPPER_SCRIPT" << EOF
#!/bin/bash
# This is a wrapper script for the memory store MCP server
# It forwards calls to the $CLI_BINARY_NAME binary

# Path to the main binary (Resolved during install script run)
GEMINI_CLI="$GEMINI_CLI_BIN_PATH"

# Check if the binary exists
if [ ! -x "$GEMINI_CLI" ]; then
    echo "Error: $CLI_BINARY_NAME not found at $GEMINI_CLI" >&2
    exit 1
fi

# Forward all input to the binary with the --memory-store-mcp flag
GEMINI_CONFIG_DIR="$UNIFIED_CONFIG_DIR" exec "$GEMINI_CLI" --memory-store-mcp "$@"
EOF

# Make the wrapper script executable
chmod +x "$MEMORY_STORE_WRAPPER_SCRIPT"


# --- Python Embedding Server ---

EMBEDDING_SERVER_ENABLED=false
# Check for Python 3 and pip
if command -v python3 &> /dev/null && command -v pip3 &> /dev/null; then
    echo "Found Python 3 and pip3."
    EMBEDDING_SERVER_ENABLED=true

    # Define paths for embedding server
    EMBEDDING_SERVER_SRC_DIR="$WORKSPACE_DIR/mcp_embedding_server"
    EMBEDDING_SERVER_INSTALL_DIR="$HOME/.local/share/gemini-cli/mcp-servers/embedding"
    VENV_DIR="$EMBEDDING_SERVER_INSTALL_DIR/venv"
    SERVER_SCRIPT_DEST_PATH="$EMBEDDING_SERVER_INSTALL_DIR/server.py"
    EMBEDDING_WRAPPER_SCRIPT="$TEMP_WRAPPER_DIR/embedding-mcp"

    # Check if source directory exists
    if [ ! -d "$EMBEDDING_SERVER_SRC_DIR" ] || [ ! -f "$EMBEDDING_SERVER_SRC_DIR/server.py" ] || [ ! -f "$EMBEDDING_SERVER_SRC_DIR/requirements.txt" ]; then
        echo "Warning: Embedding server source directory or required files not found at $EMBEDDING_SERVER_SRC_DIR."
        echo "         Skipping embedding server installation."
        EMBEDDING_SERVER_ENABLED=false
    else
        echo "Setting up Python embedding server..."
        mkdir -p "$EMBEDDING_SERVER_INSTALL_DIR"

        # Create Python virtual environment
        echo "Creating virtual environment at $VENV_DIR..."
        python3 -m venv "$VENV_DIR"

        # Install dependencies into virtual environment
        echo "Installing dependencies from $EMBEDDING_SERVER_SRC_DIR/requirements.txt..."
        # Redirect stdout to /dev/null but keep stderr visible for errors
        if ! "$VENV_DIR/bin/pip3" install --upgrade pip > /dev/null; then echo "pip upgrade failed"; fi
        if ! "$VENV_DIR/bin/pip3" install -r "$EMBEDDING_SERVER_SRC_DIR/requirements.txt" > /dev/null; then
            echo "Error: Failed to install Python dependencies for embedding server." >&2
            echo "       Check stderr above for details. Skipping embedding server installation." >&2
            EMBEDDING_SERVER_ENABLED=false
            # Clean up partial install
            rm -rf "$EMBEDDING_SERVER_INSTALL_DIR"
        else
            echo "Dependencies installed successfully."

            # Copy the server script
            echo "Copying server script to $SERVER_SCRIPT_DEST_PATH..."
            cp "$EMBEDDING_SERVER_SRC_DIR/server.py" "$SERVER_SCRIPT_DEST_PATH"

            # Create the wrapper script
            echo "Creating embedding-mcp wrapper script..."
            # Use unquoted EOF
            cat > "$EMBEDDING_WRAPPER_SCRIPT" << EOF
#!/bin/bash
# This is a wrapper script for the Python embedding MCP server

VENV_DIR="$VENV_DIR"
SERVER_SCRIPT="$SERVER_SCRIPT_DEST_PATH"

# Activate virtual environment
source "$VENV_DIR/bin/activate"

# Execute the Python server script with the unified config directory
GEMINI_CONFIG_DIR="$UNIFIED_CONFIG_DIR" exec python3 "$SERVER_SCRIPT" "$@"
EOF
            # Make the wrapper script executable
            chmod +x "$EMBEDDING_WRAPPER_SCRIPT"
        fi
    fi
else
    echo "Warning: Python 3 or pip3 not found. Skipping embedding server installation."
fi


# --- Unified Configuration Setup ---

# Create or update the unified configuration
UNIFIED_CONFIG_FILE="$UNIFIED_CONFIG_DIR/config.toml"

# Check if we have a unified configuration file generation tool 
# and the config doesn't already exist
if [ -f "$WORKSPACE_DIR/target/release/generate_unified_config" ] && [ ! -f "$UNIFIED_CONFIG_FILE" ]; then
    echo "Generating unified configuration file using the generator tool..."
    "$WORKSPACE_DIR/target/release/generate_unified_config"
elif [ ! -f "$UNIFIED_CONFIG_FILE" ]; then
    # Create a minimal template file if the generator isn't available
    echo "Creating a minimal unified configuration template..."
    
    mkdir -p "$UNIFIED_CONFIG_DIR"
    
    cat > "$UNIFIED_CONFIG_FILE" << EOF
# Gemini Suite Unified Configuration

# Gemini API configuration
[gemini]
api_key = "YOUR_API_KEY_HERE"
model_name = "gemini-2.5-pro-preview-03-25"
system_prompt = "You are a helpful assistant. Answer the user's questions concisely and accurately."
save_history = true
enable_memory_broker = true
enable_auto_memory = true
memory_broker_model = "gemini-2.0-flash"

# HAPPE daemon configuration
[happe]
ida_socket_path = "/tmp/gemini_suite_ida.sock"
happe_socket_path = "/tmp/gemini_suite_happe.sock"
http_enabled = true
http_bind_addr = "127.0.0.1:8080"

# IDA daemon configuration
[ida]
socket_path = "/tmp/gemini_suite_ida.sock"

# Memory configuration
[memory]
storage_path = "$UNIFIED_CONFIG_DIR/memory"
embedding_model = "gemini-2.0-flash"

# MCP server configuration
[[mcp.servers]]
name = "filesystem"
enabled = true
transport = "stdio"
command = ["$MCP_WRAPPER_INSTALL_PATH/filesystem-mcp"]
args = []
auto_execute = []

[[mcp.servers]]
name = "command"
enabled = true
transport = "stdio"
command = ["$MCP_WRAPPER_INSTALL_PATH/command-mcp"]
args = []
auto_execute = []

[[mcp.servers]]
name = "memory_store"
enabled = true
transport = "stdio"
command = ["$MCP_WRAPPER_INSTALL_PATH/memory-store-mcp"]
args = []
auto_execute = ["store_memory", "list_all_memories", "retrieve_memory_by_key", "retrieve_memory_by_tag", "delete_memory_by_key"]
EOF

    # Add embedding server if enabled
    if [ "$EMBEDDING_SERVER_ENABLED" = true ]; then
        cat >> "$UNIFIED_CONFIG_FILE" << EOF

[[mcp.servers]]
name = "embedding"
enabled = true
transport = "stdio"
command = ["$MCP_WRAPPER_INSTALL_PATH/embedding-mcp"]
args = []
auto_execute = ["embed"]
EOF
    fi
    
    echo "Created unified configuration template at $UNIFIED_CONFIG_FILE"
    echo "Please edit this file to set your API key and customize other settings."
else
    echo "Unified configuration file already exists at $UNIFIED_CONFIG_FILE"
    echo "Not overwriting existing configuration."
fi

# Create installation path for MCP servers
mkdir -p "$MCP_WRAPPER_INSTALL_PATH"

# Copy MCP wrapper scripts to installation path
echo "Installing MCP server wrappers to $MCP_WRAPPER_INSTALL_PATH..."
cp "$FILESYSTEM_WRAPPER_SCRIPT" "$MCP_WRAPPER_INSTALL_PATH/"
chmod +x "$MCP_WRAPPER_INSTALL_PATH/filesystem-mcp"

cp "$COMMAND_WRAPPER_SCRIPT" "$MCP_WRAPPER_INSTALL_PATH/"
chmod +x "$MCP_WRAPPER_INSTALL_PATH/command-mcp"

cp "$MEMORY_STORE_WRAPPER_SCRIPT" "$MCP_WRAPPER_INSTALL_PATH/"
chmod +x "$MCP_WRAPPER_INSTALL_PATH/memory-store-mcp"

# Copy embedding wrapper only if successfully created
if [ "$EMBEDDING_SERVER_ENABLED" = true ] && [ -f "$EMBEDDING_WRAPPER_SCRIPT" ]; then
    cp "$EMBEDDING_WRAPPER_SCRIPT" "$MCP_WRAPPER_INSTALL_PATH/"
    chmod +x "$MCP_WRAPPER_INSTALL_PATH/embedding-mcp"
    echo "Embedding server wrapper installed."
else
    echo "Skipping embedding server wrapper installation."
fi

# Clean up temporary directory
echo "Cleaning up temporary wrapper directory..."
rm -rf "$TEMP_WRAPPER_DIR"

echo "MCP server wrappers installed successfully!"
echo "Unified configuration at: $UNIFIED_CONFIG_FILE"
echo ""
echo "NOTE: All daemons will now use this unified configuration file"
echo "      located at $UNIFIED_CONFIG_FILE" 