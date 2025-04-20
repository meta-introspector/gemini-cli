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
exec "$GEMINI_CLI" --filesystem-mcp "$@"
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
exec "$GEMINI_CLI" --command-mcp "$@"
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
exec "$GEMINI_CLI" --memory-store-mcp "$@"
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

# Execute the Python server script
exec python3 "$SERVER_SCRIPT" "$@"
EOF
            # Make the wrapper script executable
            chmod +x "$EMBEDDING_WRAPPER_SCRIPT"
        fi
    fi
else
    echo "Warning: Python 3 or pip3 not found. Skipping embedding server installation."
fi


# --- Configuration Setup ---

# Create or update the default MCP servers configuration
MCP_CONFIG_DIR="$HOME/.config/gemini-cli"
mkdir -p "$MCP_CONFIG_DIR"

MCP_CONFIG_FILE="$MCP_CONFIG_DIR/mcp_servers.json"
MCP_CONFIG_EXAMPLE="$MCP_CONFIG_DIR/mcp_servers.example.json"

# Create an example configuration
echo "Creating/Updating example configuration..."
# Base example structure with filesystem, command, memory_store servers
FS_CMD="$MCP_WRAPPER_INSTALL_PATH/filesystem-mcp"
CMD_CMD="$MCP_WRAPPER_INSTALL_PATH/command-mcp"
MEM_CMD="$MCP_WRAPPER_INSTALL_PATH/memory-store-mcp"

cat > "$MCP_CONFIG_EXAMPLE" << EOF
[
  {
    "name": "filesystem",
    "enabled": true,
    "transport": "stdio",
    "command": [
      "$FS_CMD"
    ],
    "args": [],
    "env": {
      "GEMINI_MCP_TIMEOUT": "120"
    },
    "auto_execute": []
  },
  {
    "name": "command",
    "enabled": true,
    "transport": "stdio",
    "command": [
      "$CMD_CMD"
    ],
    "args": [],
    "env": {
      "GEMINI_MCP_TIMEOUT": "120"
    },
    "auto_execute": []
  },
  {
    "name": "memory_store",
    "enabled": true,
    "transport": "stdio",
    "command": [
      "$MEM_CMD"
    ],
    "args": [],
    "env": {
      "GEMINI_MCP_TIMEOUT": "120"
    },
    "auto_execute": ["store_memory", "list_all_memories", "retrieve_memory_by_key", "retrieve_memory_by_tag", "delete_memory_by_key"]
  }
]
EOF

# Add embedding server to example if it was enabled
if [ "$EMBEDDING_SERVER_ENABLED" = true ]; then
    EMB_CMD="$MCP_WRAPPER_INSTALL_PATH/embedding-mcp"
    # Use jq if available for cleaner JSON manipulation
    if command -v jq > /dev/null; then
        jq --arg path "$EMB_CMD" \
           '. += [{"name": "embedding", "enabled": true, "transport": "stdio", "command": [$path], "args": [], "env": {"GEMINI_MCP_TIMEOUT": "120"}, "auto_execute": ["embed"]}]' \
           "$MCP_CONFIG_EXAMPLE" > "$MCP_CONFIG_EXAMPLE.tmp" && mv "$MCP_CONFIG_EXAMPLE.tmp" "$MCP_CONFIG_EXAMPLE"
    else
        # Fallback to string manipulation (less robust)
        # Remove last ']'
        sed -i '$ d' "$MCP_CONFIG_EXAMPLE"
        # Add comma and new entry
        cat >> "$MCP_CONFIG_EXAMPLE" << EOF
  },
  {
    "name": "embedding",
    "enabled": true,
    "transport": "stdio",
    "command": ["$EMB_CMD"],
    "args": [],
    "env": {
      "GEMINI_MCP_TIMEOUT": "120"
    },
    "auto_execute": ["embed"]
  }
]
EOF
    fi
fi


# Copy the example to the actual config if it doesn't exist
if [ ! -f "$MCP_CONFIG_FILE" ]; then
    echo "Creating initial MCP servers configuration..."
    cp "$MCP_CONFIG_EXAMPLE" "$MCP_CONFIG_FILE"
    echo "Created MCP configuration at $MCP_CONFIG_FILE"
else
    echo "Existing MCP configuration found at $MCP_CONFIG_FILE"

    # Ensure 'jq' is available for reliable JSON updates
    if ! command -v jq &> /dev/null; then
        echo "Warning: 'jq' command not found. Configuration updates might be skipped or less reliable." >&2
        echo "         Please install 'jq' for automatic configuration management." >&2
        
        # Without jq, just copy the example file but warn the user
        echo "Warning: Overwriting existing configuration with example. You may need to manually restore custom settings." >&2
        cp "$MCP_CONFIG_EXAMPLE" "$MCP_CONFIG_FILE"
    else
        echo "Updating existing configuration using jq..."
        # Update existing configuration, maintaining any custom settings but ensuring
        # all servers exist and have proper environment variables/paths
        
        # Create a temporary file with the target configuration based on the example
        TEMP_TARGET_CONFIG="$TEMP_WRAPPER_DIR/target_config.json"
        cp "$MCP_CONFIG_EXAMPLE" "$TEMP_TARGET_CONFIG"

        # Merge the target config into the existing one using jq
        # This adds missing servers and updates command paths and default env/auto_execute for existing ones.
        # It preserves user changes like 'enabled' status or custom args/env.
        JQ_SCRIPT='map( \
            . as $target_server | \
            ($existing[0][] | select(.name == $target_server.name)) as $existing_server | \
            if $existing_server then \
                # Merge existing with target: target command overrides, env merges, auto_execute merges uniquely \
                $existing_server + \
                {command: $target_server.command} + \
                {env: ($target_server.env // {} + $existing_server.env // {})} + \
                {auto_execute: ($target_server.auto_execute // [] + $existing_server.auto_execute // [] | unique)} \
            else \
                # Add the new server from target config \
                $target_server \
            end \
        )'
        MERGED_CONFIG=$(jq --slurpfile existing "$MCP_CONFIG_FILE" "$JQ_SCRIPT" "$TEMP_TARGET_CONFIG")

        # Check if jq command succeeded
        if [ $? -ne 0 ]; then
            echo "Error: jq command failed during configuration merge." >&2
            echo "       Original configuration file preserved." >&2
            rm "$TEMP_TARGET_CONFIG"
        else \
            # Write the merged config back \
            echo "$MERGED_CONFIG" | jq '.' > "$MCP_CONFIG_FILE" # Pretty print with jq \
            rm "$TEMP_TARGET_CONFIG" \
            echo "Configuration updated." \
        fi \
    fi # End jq check
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
echo "Configuration at: $MCP_CONFIG_FILE" 