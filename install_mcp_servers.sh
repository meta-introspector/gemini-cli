#!/bin/bash
set -e

# Get the root directory of the project
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
echo "Building and installing MCP servers from $ROOT_DIR..."

# Create directory for MCP servers
echo "Creating MCP servers directory..."
MCP_SERVERS_DIR="$ROOT_DIR/mcp-servers"
mkdir -p "$MCP_SERVERS_DIR"

# Define paths
FILESYSTEM_SERVER_DIR="$ROOT_DIR/src/mcp/servers/filesystem"
COMMAND_SERVER_DIR="$ROOT_DIR/src/mcp/servers/command"

# Check if directories exist
if [ ! -d "$FILESYSTEM_SERVER_DIR" ]; then
    echo "Error: Filesystem server directory not found at $FILESYSTEM_SERVER_DIR"
    exit 1
fi

if [ ! -d "$COMMAND_SERVER_DIR" ]; then
    echo "Error: Command server directory not found at $COMMAND_SERVER_DIR"
    exit 1
fi

# Create a binary directory for installed binaries
BINARY_DIR="$HOME/.local/bin"
mkdir -p "$BINARY_DIR"

# Create a simple wrapper script for the filesystem MCP
echo "Creating filesystem-mcp wrapper script..."
FILESYSTEM_WRAPPER_SCRIPT="$MCP_SERVERS_DIR/filesystem-mcp"

cat > "$FILESYSTEM_WRAPPER_SCRIPT" << 'EOF'
#!/bin/bash
# This is a wrapper script for the filesystem MCP server
# It forwards calls to the gemini-cli binary

# Get directory containing this script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GEMINI_CLI="${HOME}/.local/bin/gemini-cli-bin"

# Check if the binary exists
if [ ! -f "$GEMINI_CLI" ]; then
    echo "Error: gemini-cli-bin not found at $GEMINI_CLI" >&2
    exit 1
fi

# Forward all input to the binary with the --filesystem-mcp flag
exec "$GEMINI_CLI" --filesystem-mcp
EOF

# Make the wrapper script executable
chmod +x "$FILESYSTEM_WRAPPER_SCRIPT"

# Create a simple wrapper script for the command MCP
echo "Creating command-mcp wrapper script..."
COMMAND_WRAPPER_SCRIPT="$MCP_SERVERS_DIR/command-mcp"

cat > "$COMMAND_WRAPPER_SCRIPT" << 'EOF'
#!/bin/bash
# This is a wrapper script for the command MCP server
# It forwards calls to the gemini-cli binary

# Get directory containing this script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GEMINI_CLI="${HOME}/.local/bin/gemini-cli-bin"

# Check if the binary exists
if [ ! -f "$GEMINI_CLI" ]; then
    echo "Error: gemini-cli-bin not found at $GEMINI_CLI" >&2
    exit 1
fi

# Forward all input to the binary with the --command-mcp flag
exec "$GEMINI_CLI" --command-mcp
EOF

# Make the wrapper script executable
chmod +x "$COMMAND_WRAPPER_SCRIPT"

# Create a simple wrapper script for the memory store MCP
echo "Creating memory-store-mcp wrapper script..."
MEMORY_STORE_WRAPPER_SCRIPT="$MCP_SERVERS_DIR/memory-store-mcp"

cat > "$MEMORY_STORE_WRAPPER_SCRIPT" << 'EOF'
#!/bin/bash
# This is a wrapper script for the memory store MCP server
# It forwards calls to the gemini-cli binary

# Get directory containing this script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GEMINI_CLI="${HOME}/.local/bin/gemini-cli-bin"

# Check if the binary exists
if [ ! -f "$GEMINI_CLI" ]; then
    echo "Error: gemini-cli-bin not found at $GEMINI_CLI" >&2
    exit 1
fi

# Forward all input to the binary with the --memory-store-mcp flag
exec "$GEMINI_CLI" --memory-store-mcp
EOF

# Make the wrapper script executable
chmod +x "$MEMORY_STORE_WRAPPER_SCRIPT"

# Create or update the default MCP servers configuration
MCP_CONFIG_DIR="$HOME/.config/gemini-cli"
mkdir -p "$MCP_CONFIG_DIR"

MCP_CONFIG_FILE="$MCP_CONFIG_DIR/mcp_servers.json"
MCP_CONFIG_EXAMPLE="$MCP_CONFIG_DIR/mcp_servers.example.json"

# Create an example configuration
echo "Creating example configuration..."
cat > "$MCP_CONFIG_EXAMPLE" << 'EOF'
[
  {
    "name": "filesystem",
    "enabled": true,
    "transport": "stdio",
    "command": ["~/.local/bin/mcp-servers/filesystem-mcp"],
    "args": [],
    "auto_execute": []
  },
  {
    "name": "command",
    "enabled": true,
    "transport": "stdio",
    "command": ["~/.local/bin/mcp-servers/command-mcp"],
    "args": [],
    "auto_execute": []
  },
  {
    "name": "memory_store",
    "enabled": true,
    "transport": "stdio",
    "command": ["~/.local/bin/mcp-servers/memory-store-mcp"],
    "args": [],
    "auto_execute": []
  }
]
EOF

# Copy the example to the actual config if it doesn't exist
if [ ! -f "$MCP_CONFIG_FILE" ]; then
    echo "Creating initial MCP servers configuration..."
    cp "$MCP_CONFIG_EXAMPLE" "$MCP_CONFIG_FILE"
    echo "Created MCP configuration at $MCP_CONFIG_FILE"
else
    echo "Existing MCP configuration found at $MCP_CONFIG_FILE"
    
    # Check if auto_execute is missing from the configuration and add it
    if ! grep -q '"auto_execute"' "$MCP_CONFIG_FILE" && command -v jq > /dev/null; then
        echo "Adding auto_execute field to existing configuration..."
        
        # Create a temporary file for the updated configuration
        TMP_CONFIG_FILE=$(mktemp)
        
        # Add auto_execute array to each server config
        jq 'map(. + if has("auto_execute") then {} else {"auto_execute": []} end)' \
           "$MCP_CONFIG_FILE" > "$TMP_CONFIG_FILE"
        
        # If the temp file exists and has content, replace the original
        if [ -f "$TMP_CONFIG_FILE" ] && [ -s "$TMP_CONFIG_FILE" ]; then
            mv "$TMP_CONFIG_FILE" "$MCP_CONFIG_FILE"
            echo "Updated MCP configuration to include auto_execute field"
        else
            rm -f "$TMP_CONFIG_FILE"
            echo "Failed to add auto_execute field to configuration"
        fi
    fi
    
    echo "Checking for command MCP server in configuration..."
    
    # Check if the command server is already in the config
    if ! grep -q '"name": *"command"' "$MCP_CONFIG_FILE"; then
        echo "Command MCP server not found in configuration, adding it..."
        
        # Create a temporary file with updated configuration
        TMP_CONFIG_FILE=$(mktemp)
        
        # Use jq to add the command server if jq is available
        if command -v jq > /dev/null; then
            # Check if the config already has auto_execute fields
            if grep -q '"auto_execute"' "$MCP_CONFIG_FILE"; then
                # Use jq to add the command server config with auto_execute
                jq '. += [{"name": "command", "enabled": true, "transport": "stdio", "command": ["~/.local/bin/mcp-servers/command-mcp"], "args": [], "auto_execute": []}]' \
                   "$MCP_CONFIG_FILE" > "$TMP_CONFIG_FILE"
            else 
                # Add without auto_execute to maintain backward compatibility
                jq '. += [{"name": "command", "enabled": true, "transport": "stdio", "command": ["~/.local/bin/mcp-servers/command-mcp"], "args": []}]' \
                   "$MCP_CONFIG_FILE" > "$TMP_CONFIG_FILE"
            fi
        else
            # Simple append approach if jq is not available
            # This requires the file to not have whitespace after the last entry
            # Find the position of the last closing bracket
            LINE_COUNT=$(wc -l < "$MCP_CONFIG_FILE")
            LAST_LINE=$(tail -n 1 "$MCP_CONFIG_FILE")
            
            if [[ "$LAST_LINE" == "]" ]]; then
                # Copy all except the last line
                head -n $(($LINE_COUNT - 1)) "$MCP_CONFIG_FILE" > "$TMP_CONFIG_FILE"
                
                # Add the comma and new entry
                echo "  }," >> "$TMP_CONFIG_FILE"
                echo "  {" >> "$TMP_CONFIG_FILE"
                echo "    \"name\": \"command\"," >> "$TMP_CONFIG_FILE"
                echo "    \"enabled\": true," >> "$TMP_CONFIG_FILE"
                echo "    \"transport\": \"stdio\"," >> "$TMP_CONFIG_FILE"
                echo "    \"command\": [\"~/.local/bin/mcp-servers/command-mcp\"]," >> "$TMP_CONFIG_FILE"
                echo "    \"args\": []," >> "$TMP_CONFIG_FILE"
                
                # Add auto_execute if the existing config uses it
                if grep -q '"auto_execute"' "$MCP_CONFIG_FILE"; then
                    echo "    \"auto_execute\": []" >> "$TMP_CONFIG_FILE"
                fi
                
                echo "  }" >> "$TMP_CONFIG_FILE"
                echo "]" >> "$TMP_CONFIG_FILE"
            else
                echo "Cannot safely update config file without jq. Please install jq or manually add the command server."
                echo "Example configuration updated at $MCP_CONFIG_EXAMPLE"
                rm "$TMP_CONFIG_FILE"
                # Continue without erroring out
            fi
        fi
        
        # If the temp file exists and has content, replace the original
        if [ -f "$TMP_CONFIG_FILE" ] && [ -s "$TMP_CONFIG_FILE" ]; then
            mv "$TMP_CONFIG_FILE" "$MCP_CONFIG_FILE"
            echo "Updated MCP configuration to include command server"
        else
            rm -f "$TMP_CONFIG_FILE"
            echo "Failed to add command server to configuration"
        fi
    fi
    
    echo "Checking for memory_store MCP server in configuration..."
    
    # Check if the memory_store server is already in the config
    if ! grep -q '"name": *"memory_store"' "$MCP_CONFIG_FILE"; then
        echo "Memory store MCP server not found in configuration, adding it..."
        
        # Create a temporary file with updated configuration
        TMP_CONFIG_FILE=$(mktemp)
        
        # Use jq to add the memory_store server if jq is available
        if command -v jq > /dev/null; then
            # Check if the config already has auto_execute fields
            if grep -q '"auto_execute"' "$MCP_CONFIG_FILE"; then
                # Use jq to add the memory_store server config with auto_execute
                jq '. += [{"name": "memory_store", "enabled": true, "transport": "stdio", "command": ["~/.local/bin/mcp-servers/memory-store-mcp"], "args": [], "auto_execute": ["store_memory", "list_all_memories", "retrieve_memory_by_key", "retrieve_memory_by_tag", "delete_memory_by_key"]}]' \
                   "$MCP_CONFIG_FILE" > "$TMP_CONFIG_FILE"
            else 
                # Add without auto_execute to maintain backward compatibility
                jq '. += [{"name": "memory_store", "enabled": true, "transport": "stdio", "command": ["~/.local/bin/mcp-servers/memory-store-mcp"], "args": []}]' \
                   "$MCP_CONFIG_FILE" > "$TMP_CONFIG_FILE"
            fi
        else
            # Simple append approach if jq is not available
            # This requires the file to not have whitespace after the last entry
            # Find the position of the last closing bracket
            LINE_COUNT=$(wc -l < "$MCP_CONFIG_FILE")
            LAST_LINE=$(tail -n 1 "$MCP_CONFIG_FILE")
            
            if [[ "$LAST_LINE" == "]" ]]; then
                # Copy all except the last line
                head -n $(($LINE_COUNT - 1)) "$MCP_CONFIG_FILE" > "$TMP_CONFIG_FILE"
                
                # Add the comma and new entry
                echo "  }," >> "$TMP_CONFIG_FILE"
                echo "  {" >> "$TMP_CONFIG_FILE"
                echo "    \"name\": \"memory_store\"," >> "$TMP_CONFIG_FILE"
                echo "    \"enabled\": true," >> "$TMP_CONFIG_FILE"
                echo "    \"transport\": \"stdio\"," >> "$TMP_CONFIG_FILE"
                echo "    \"command\": [\"~/.local/bin/mcp-servers/memory-store-mcp\"]," >> "$TMP_CONFIG_FILE"
                echo "    \"args\": []," >> "$TMP_CONFIG_FILE"
                
                # Add auto_execute if the existing config uses it
                if grep -q '"auto_execute"' "$MCP_CONFIG_FILE"; then
                    echo "    \"auto_execute\": [\"store_memory\", \"list_all_memories\", \"retrieve_memory_by_key\", \"retrieve_memory_by_tag\", \"delete_memory_by_key\"]" >> "$TMP_CONFIG_FILE"
                fi
                
                echo "  }" >> "$TMP_CONFIG_FILE"
                echo "]" >> "$TMP_CONFIG_FILE"
            else
                echo "Cannot safely update config file without jq. Please install jq or manually add the memory_store server."
                echo "Example configuration updated at $MCP_CONFIG_EXAMPLE"
                rm "$TMP_CONFIG_FILE"
                # Continue without erroring out
            fi
        fi
        
        # If the temp file exists and has content, replace the original
        if [ -f "$TMP_CONFIG_FILE" ] && [ -s "$TMP_CONFIG_FILE" ]; then
            mv "$TMP_CONFIG_FILE" "$MCP_CONFIG_FILE"
            echo "Updated MCP configuration to include memory_store server"
        else
            rm -f "$TMP_CONFIG_FILE"
            echo "Failed to add memory_store server to configuration"
        fi
    fi
fi

# Create installation path for MCP servers
INSTALL_PATH="$HOME/.local/bin/mcp-servers"
mkdir -p "$INSTALL_PATH"

# Copy MCP wrapper scripts to installation path
echo "Installing MCP servers to $INSTALL_PATH..."
cp "$FILESYSTEM_WRAPPER_SCRIPT" "$INSTALL_PATH/"
cp "$COMMAND_WRAPPER_SCRIPT" "$INSTALL_PATH/"
cp "$MEMORY_STORE_WRAPPER_SCRIPT" "$INSTALL_PATH/"

echo "MCP servers installed successfully!"
echo "Configuration at: $MCP_CONFIG_FILE" 