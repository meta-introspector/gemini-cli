#!/usr/bin/env bash
set -e

# Colors for better readability
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

INSTALL_DIR="$HOME/.local/bin"
BINARY_NAME="gemini-cli-bin" # Renamed binary
FUNCTION_NAME="gemini"
INSTALL_PATH="$INSTALL_DIR/$BINARY_NAME"
SOURCE_DIR="."
SHELL_CONFIG_FILE=""
CURRENT_SHELL="$(basename "$SHELL")"
WRAPPER_FUNCTION_START="# Gemini CLI Wrapper Function Start"
WRAPPER_FUNCTION_END="# Gemini CLI Wrapper Function End"

echo -e "${BLUE}=== Gemini CLI Installer/Updater ===${NC}"

# Check if rust is installed
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: Rust/Cargo is not installed.${NC}"
    echo "To install Rust, please visit: https://rustup.rs/"
    exit 1
fi

# Detect shell config file
if [ "$CURRENT_SHELL" = "bash" ]; then
    SHELL_CONFIG_FILE="$HOME/.bashrc"
elif [ "$CURRENT_SHELL" = "zsh" ]; then
    SHELL_CONFIG_FILE="$HOME/.zshrc"
else
    echo -e "${YELLOW}Warning: Unsupported shell '$CURRENT_SHELL'. Cannot automatically add wrapper function.${NC}"
    echo "Please add the wrapper function manually to your shell configuration."
fi

# Check if we are in the project directory or one level up
if [ -f "./Cargo.toml" ]; then
    SOURCE_DIR="."
    echo "Detected project root in current directory."
elif [ -f "./gemini-cli/Cargo.toml" ] && [ -d "./gemini-cli" ]; then
    SOURCE_DIR="./gemini-cli"
    echo "Detected project root in ./gemini-cli/ directory."
else 
    echo -e "${RED}Error: Could not find Cargo.toml. Please run this script from the project root directory or its parent.${NC}"
    exit 1
fi

# Uninstall previous version if exists
echo -e "\n${YELLOW}Checking for existing installation...${NC}"
HAD_PREVIOUS_INSTALL=false

# Remove existing binary
if [ -f "$INSTALL_PATH" ]; then
    echo "- Removing existing binary: $INSTALL_PATH"
    rm -f "$INSTALL_PATH"
    HAD_PREVIOUS_INSTALL=true
fi

# Remove existing wrapper function from shell config
if [ -n "$SHELL_CONFIG_FILE" ] && [ -f "$SHELL_CONFIG_FILE" ]; then
    if grep -q "$WRAPPER_FUNCTION_START" "$SHELL_CONFIG_FILE"; then
        echo "- Removing existing wrapper function from $SHELL_CONFIG_FILE"
        sed -i.bak "/$WRAPPER_FUNCTION_START/,/$WRAPPER_FUNCTION_END/d" "$SHELL_CONFIG_FILE"
        # Remove backup file created by sed -i on macOS
        rm -f "${SHELL_CONFIG_FILE}.bak"
        HAD_PREVIOUS_INSTALL=true
    fi
fi

if [ "$HAD_PREVIOUS_INSTALL" = true ]; then
    echo -e "${GREEN}✓ Previous installation components removed.${NC}"
else
    echo "- No previous installation found."
fi

# Confirmation
INSTALL_ACTION="Install"
if [ "$HAD_PREVIOUS_INSTALL" = true ]; then
    INSTALL_ACTION="Reinstall"
fi

echo -e "\nWill install binary to: ${GREEN}$INSTALL_PATH${NC}"
if [ -n "$SHELL_CONFIG_FILE" ]; then
    echo -e "Will add wrapper function '$FUNCTION_NAME' to: ${GREEN}$SHELL_CONFIG_FILE${NC}"
else
    echo -e "${YELLOW}Warning: Unsupported shell. Wrapper function requires manual setup.${NC}"
fi

read -p "$INSTALL_ACTION Gemini CLI? [Y/n] " -n 1 -r REPLY
echo
if [[ ! $REPLY =~ ^[Yy]$ ]] && [[ ! -z $REPLY ]]; then
    echo -e "${RED}Operation aborted.${NC}"
    exit 1
fi

# Create installation directory if it doesn't exist
if [ ! -d "$INSTALL_DIR" ]; then
    echo -e "\n${YELLOW}Creating installation directory...${NC}"
    mkdir -p "$INSTALL_DIR"
    echo -e "${GREEN}✓ Created $INSTALL_DIR${NC}"
fi

# Build the release binary
echo -e "\n${YELLOW}Building release binary in '$SOURCE_DIR' (this may take a moment)...${NC}"
(cd "$SOURCE_DIR" && cargo build --release)
if [ $? -ne 0 ]; then
    echo -e "${RED}Error: Build failed.${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Build completed${NC}"

# Install MCP servers if the script exists
if [ -f "$SOURCE_DIR/install_mcp_servers.sh" ]; then
    echo -e "\n${BLUE}Installing MCP servers...${NC}"
    # Make script executable
    chmod +x "$SOURCE_DIR/install_mcp_servers.sh"
    # Run the MCP server installation script
    "$SOURCE_DIR/install_mcp_servers.sh" || {
        echo -e "${YELLOW}Warning: Failed to install MCP servers. Some functionality may be limited.${NC}"
        echo -e "        Check the error messages above for details."
    }
    echo -e "${GREEN}✓ MCP servers installation completed${NC}"
else
    echo -e "\n${YELLOW}Warning: MCP server installation script not found at '$SOURCE_DIR/install_mcp_servers.sh'${NC}"
    echo -e "         Some functionality may be limited. Consider reinstalling from the repository."
fi

# Copy the binary
echo -e "\n${YELLOW}Installing $BINARY_NAME command...${NC}"
# Force remove existing file/symlink first to avoid dangling symlink errors
rm -f "$INSTALL_PATH"
cp "$SOURCE_DIR/target/release/$BINARY_NAME" "$INSTALL_PATH"
chmod +x "$INSTALL_PATH"
echo -e "${GREEN}✓ Binary installed: $INSTALL_PATH${NC}"

# Add wrapper function to shell config
if [ -n "$SHELL_CONFIG_FILE" ] && [ -f "$SHELL_CONFIG_FILE" ]; then
    echo -e "\\n${YELLOW}Adding shell function \'$FUNCTION_NAME\' to $SHELL_CONFIG_FILE...${NC}"
    
    # Use cat WITHOUT quoted heredoc delimiter (EOM) to allow expansion
    # Escape internal '$' and '\' that should be part of the function literal
    cat << EOM >> "$SHELL_CONFIG_FILE"
$WRAPPER_FUNCTION_START
# This function wraps the gemini-cli-bin
gemini() {
    # BINARY_NAME is expanded by install.sh shell HERE
    local gemini_bin="$HOME/.local/bin/$BINARY_NAME"

    if [ ! -x "\$gemini_bin" ]; then
        # BINARY_NAME is expanded by install.sh shell HERE
        echo "Error: $BINARY_NAME not found or not executable at [\$gemini_bin]" >&2
        return 1
    fi

    # Create or use an existing session ID for persistence
    # Only generate a new one if --new-chat is specified
    if [[ "\$*" == *--new-chat* ]]; then
        # Generate a new session ID for --new-chat
        unset GEMINI_SESSION_ID
    fi

    # If GEMINI_SESSION_ID is not set, generate one
    if [ -z "\${GEMINI_SESSION_ID}" ]; then
        # Use timestamp + terminal PID as a simple session ID
        local timestamp=\$(date +%s)
        local ppid=\$(ps -o ppid= -p \$\$)
        export GEMINI_SESSION_ID="term_\${ppid}_\${timestamp}"
        if [[ "\$*" != *--set-api-key* ]] && [[ "\$*" != *--set-system-prompt* ]] && [[ "\$*" != *--show-config* ]]; then
            echo "Started new conversation (session: \$GEMINI_SESSION_ID)"
        fi
    fi

    # Simply execute the binary with all arguments passed to the function
    # Pass the GEMINI_SESSION_ID to the binary directly
    "\$gemini_bin" "\$@"
    return \$? # Return the exit code of the binary
}
$WRAPPER_FUNCTION_END
EOM

    # Verification step
    echo "Syncing filesystem before verification..." # Debug
    sync # Attempt to ensure file write is flushed
    echo "Running verification: grep \"$WRAPPER_FUNCTION_START\" \"$SHELL_CONFIG_FILE\"" # Debug
    grep "$WRAPPER_FUNCTION_START" "$SHELL_CONFIG_FILE" # Removed -q for debugging
    VERIFICATION_EXIT_CODE=$?
    echo "Verification grep exit code: $VERIFICATION_EXIT_CODE" # Debug
    
    # Check the exit code explicitly
    if [ $VERIFICATION_EXIT_CODE -eq 0 ]; then 
        echo -e "${GREEN}✓ Shell function added successfully.${NC}"
    else
        echo -e "${RED}Error: Failed verification step after adding shell function to $SHELL_CONFIG_FILE. Please check permissions or add manually.${NC}"
        exit 1
    fi
    
    echo -e "\n${YELLOW}Important: Please reload your shell configuration to use the '$FUNCTION_NAME' command:${NC}"
    if [ "$CURRENT_SHELL" = "bash" ]; then
        echo "  Run: source ~/.bashrc"
    elif [ "$CURRENT_SHELL" = "zsh" ]; then
        echo "  Run: source ~/.zshrc"
    fi
    echo "(Or simply open a new terminal window)"

else
    echo -e "\n${YELLOW}Could not automatically add shell function.${NC}"
    echo "Please manually add the wrapper function (see README.md) to your shell config file."
fi

# Show success message and usage
echo -e "\n${GREEN}=== ${INSTALL_ACTION} Complete! ===${NC}"

# PATH reminder if needed
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo -e "\n${YELLOW}Reminder: Ensure $INSTALL_DIR is in your PATH for the wrapper function to find the binary.${NC}"
fi

echo -e "After reloading your shell, you can use Gemini CLI by running: ${BLUE}$FUNCTION_NAME \"your prompt\"${NC}"

if [ "$HAD_PREVIOUS_INSTALL" = false ]; then
    echo
    echo -e "${YELLOW}First Steps:${NC}"
    echo "1. Reload your shell (e.g., 'source ~/.zshrc' or new terminal)"
    echo "2. Set your API key: $FUNCTION_NAME --set-api-key YOUR_API_KEY"
    echo "3. Set a custom system prompt (optional): $FUNCTION_NAME --set-system-prompt \"You are a helpful assistant.\""
    echo "4. Try a test query: $FUNCTION_NAME \"Hello, Gemini!\""
    echo "5. For command help: $FUNCTION_NAME -c \"how to list all processes\""
fi

echo
echo -e "Run ${BLUE}$FUNCTION_NAME --help${NC} for more options." 