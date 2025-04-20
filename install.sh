#!/usr/bin/env bash
set -e

# Colors for better readability
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

INSTALL_DIR="$HOME/.local/bin"
CLI_BINARY_NAME="gemini-cli-bin" # Name of the main CLI binary to build and install
MCP_HOSTD_BINARY_NAME="mcp-hostd" # Name of the daemon binary
FUNCTION_NAME="gemini"
CLI_INSTALL_PATH="$INSTALL_DIR/$CLI_BINARY_NAME"
MCP_HOSTD_INSTALL_PATH="$INSTALL_DIR/$MCP_HOSTD_BINARY_NAME"
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )" # Directory of the install script
SHELL_CONFIG_FILE=""
CURRENT_SHELL="$(basename "$SHELL")"
WRAPPER_FUNCTION_START="# Gemini CLI Wrapper Function Start"
WRAPPER_FUNCTION_END="# Gemini CLI Wrapper Function End"

echo -e "${BLUE}=== Gemini CLI and MCP Host Daemon Installer/Updater ===${NC}"

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

# Check if we are in the workspace structure
if [ -f "$SCRIPT_DIR/Cargo.toml" ] && grep -q "workspace" "$SCRIPT_DIR/Cargo.toml"; then
    WORKSPACE_DIR="$SCRIPT_DIR"
    if [ -d "$WORKSPACE_DIR/cli" ] && [ -f "$WORKSPACE_DIR/cli/Cargo.toml" ] && [ -d "$WORKSPACE_DIR/mcp" ] && [ -f "$WORKSPACE_DIR/mcp/Cargo.toml" ]; then
        CLI_DIR="$WORKSPACE_DIR/cli"
        MCP_DIR="$WORKSPACE_DIR/mcp"
        echo "Detected workspace structure with CLI in ./cli/ and MCP in ./mcp/ directories."
    else
        echo -e "${RED}Error: CLI or MCP directory not found in workspace. Expected $WORKSPACE_DIR/cli and $WORKSPACE_DIR/mcp${NC}"
        exit 1
    fi
else
    echo -e "${RED}Error: Not in a workspace root directory. Please run this script from the workspace root directory containing Cargo.toml.${NC}"
    exit 1
fi

# Uninstall previous version if exists
echo -e "\n${YELLOW}Checking for existing installation...${NC}"
HAD_PREVIOUS_INSTALL=false

# Remove existing CLI binary
if [ -f "$CLI_INSTALL_PATH" ]; then
    echo "- Removing existing CLI binary: $CLI_INSTALL_PATH"
    rm -f "$CLI_INSTALL_PATH"
    HAD_PREVIOUS_INSTALL=true
fi

# Remove existing MCP Hostd binary
if [ -f "$MCP_HOSTD_INSTALL_PATH" ]; then
    echo "- Removing existing MCP Host Daemon binary: $MCP_HOSTD_INSTALL_PATH"
    rm -f "$MCP_HOSTD_INSTALL_PATH"
    HAD_PREVIOUS_INSTALL=true
fi

# Remove existing wrapper function from shell config
if [ -n "$SHELL_CONFIG_FILE" ] && [ -f "$SHELL_CONFIG_FILE" ]; then
    if grep -q "$WRAPPER_FUNCTION_START" "$SHELL_CONFIG_FILE"; then
        echo "- Removing existing wrapper function from $SHELL_CONFIG_FILE"
        # Use awk for more robust start/end marker deletion
        awk "/$WRAPPER_FUNCTION_START/{flag=1;next}/$WRAPPER_FUNCTION_END/{flag=0;next}flag==0" "$SHELL_CONFIG_FILE" > "$SHELL_CONFIG_FILE.tmp" && mv "$SHELL_CONFIG_FILE.tmp" "$SHELL_CONFIG_FILE"
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

echo -e "\nWill install Gemini CLI binary to: ${GREEN}$CLI_INSTALL_PATH${NC}"
echo -e "Will install MCP Host Daemon binary to: ${GREEN}$MCP_HOSTD_INSTALL_PATH${NC}"
if [ -n "$SHELL_CONFIG_FILE" ]; then
    echo -e "Will add wrapper function '$FUNCTION_NAME' to: ${GREEN}$SHELL_CONFIG_FILE${NC}"
else
    echo -e "${YELLOW}Warning: Unsupported shell. Wrapper function requires manual setup.${NC}"
fi

read -p "$INSTALL_ACTION Gemini CLI and MCP Host Daemon? [Y/n] " -n 1 -r REPLY
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

# Build the release binaries
echo -e "\n${YELLOW}Building release binaries in workspace (this may take a moment)...${NC}"
(cd "$WORKSPACE_DIR" && cargo build --release --bin gemini-cli-bin --bin mcp-hostd)
if [ $? -ne 0 ]; then
    echo -e "${RED}Error: Build failed.${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Build completed${NC}"

# Define the path of the built binaries
BUILT_CLI_BINARY="$WORKSPACE_DIR/target/release/$CLI_BINARY_NAME"
BUILT_MCP_HOSTD_BINARY="$WORKSPACE_DIR/target/release/$MCP_HOSTD_BINARY_NAME"

# Define MCP install script path in the workspace
MCP_INSTALL_SCRIPT="$SCRIPT_DIR/install_mcp_servers.sh"

# Install MCP servers if the script exists
if [ -f "$MCP_INSTALL_SCRIPT" ]; then
    echo -e "\n${BLUE}Installing/Updating MCP server wrappers and configuration...${NC}"
    # Make script executable
    chmod +x "$MCP_INSTALL_SCRIPT"
    # Run the MCP server installation script
    # Pass the CLI binary name needed by the wrappers
    "$MCP_INSTALL_SCRIPT" "$CLI_BINARY_NAME" || {
        echo -e "${YELLOW}Warning: Failed to install MCP server wrappers/config. MCP features may be limited.${NC}"
        echo -e "        Check the error messages above for details."
    }
    echo -e "${GREEN}✓ MCP server wrappers installation completed${NC}"
else
    echo -e "\n${YELLOW}Warning: MCP server installation script not found at '$MCP_INSTALL_SCRIPT'${NC}"
    echo -e "         MCP features will likely not work. Consider obtaining it."
fi

# Copy the binaries
echo -e "\n${YELLOW}Installing $CLI_BINARY_NAME command...${NC}"
# Force remove existing file/symlink first to avoid dangling symlink errors
rm -f "$CLI_INSTALL_PATH"
cp "$BUILT_CLI_BINARY" "$CLI_INSTALL_PATH"
chmod +x "$CLI_INSTALL_PATH"
echo -e "${GREEN}✓ CLI Binary installed: $CLI_INSTALL_PATH${NC}"

echo -e "\n${YELLOW}Installing $MCP_HOSTD_BINARY_NAME command...${NC}"
# Force remove existing file/symlink first
rm -f "$MCP_HOSTD_INSTALL_PATH"
cp "$BUILT_MCP_HOSTD_BINARY" "$MCP_HOSTD_INSTALL_PATH"
chmod +x "$MCP_HOSTD_INSTALL_PATH"
echo -e "${GREEN}✓ MCP Host Daemon Binary installed: $MCP_HOSTD_INSTALL_PATH${NC}"

# Add wrapper function to shell config
if [ -n "$SHELL_CONFIG_FILE" ] && [ -f "$SHELL_CONFIG_FILE" ]; then
    echo -e "\n${YELLOW}Adding shell function '$FUNCTION_NAME' to $SHELL_CONFIG_FILE...${NC}"
    
    # Use cat WITHOUT quoted heredoc delimiter (EOM) to allow expansion of $CLI_BINARY_NAME by this script
    # Escape internal '$' and '\\' that should be part of the function literal
    cat << EOM >> "$SHELL_CONFIG_FILE"
$WRAPPER_FUNCTION_START
# This function wraps the gemini-cli-bin
gemini() {
    # The binary name is expanded by install.sh HERE
    local gemini_bin="$INSTALL_DIR/$CLI_BINARY_NAME"

    if [ ! -x "\$gemini_bin" ]; then
        # The binary name is expanded by install.sh HERE
        echo "Error: $CLI_BINARY_NAME not found or not executable at [\$gemini_bin]" >&2
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
    # Pass the GEMINI_SESSION_ID environment variable implicitly
    "\$gemini_bin" "\$@"
    return \$? # Return the exit code of the binary
}
$WRAPPER_FUNCTION_END
EOM

    # Verification step
    # Use awk for verification as well for consistency
    if awk "/$WRAPPER_FUNCTION_START/{found=1} END{exit !found}" "$SHELL_CONFIG_FILE"; then
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

# Add MCPD management function if using Zsh
if [ "$CURRENT_SHELL" = "zsh" ]; then
    echo -e "\n${YELLOW}Adding MCP Daemon management function 'mcpd' to $SHELL_CONFIG_FILE...${NC}"
    MCPD_FUNCTION_START="# MCPD Management Function Start"
    MCPD_FUNCTION_END="# MCPD Management Function End"

    # Remove existing function block first
    if grep -q "$MCPD_FUNCTION_START" "$SHELL_CONFIG_FILE"; then
        echo "- Removing existing mcpd function block..."
        awk "/$MCPD_FUNCTION_START/{flag=1;next}/$MCPD_FUNCTION_END/{flag=0;next}flag==0" "$SHELL_CONFIG_FILE" > "$SHELL_CONFIG_FILE.tmp" && mv "$SHELL_CONFIG_FILE.tmp" "$SHELL_CONFIG_FILE"
    fi

    # Define paths used by the function (resolved now)
    _MCPD_BIN_PATH="$MCP_HOSTD_INSTALL_PATH"
    # Use XDG standard dirs if possible, fallback otherwise
    _MCPD_RUNTIME_BASE="${XDG_RUNTIME_DIR:-$HOME/.local/share}/gemini-cli"
    _MCPD_PID_FILE="$_MCPD_RUNTIME_BASE/mcp-hostd.pid"
    _MCPD_LOG_FILE="$_MCPD_RUNTIME_BASE/mcp-hostd.log"

    # Write the start marker (variable is expanded)
    echo "" >> "$SHELL_CONFIG_FILE"
    echo "$MCPD_FUNCTION_START" >> "$SHELL_CONFIG_FILE"

    # Write the function definition
    # Use echo for lines needing expansion, use quoted heredoc for the rest
    echo "# Function to manage the mcp-hostd daemon" >> "$SHELL_CONFIG_FILE"
    echo "mcpd() {" >> "$SHELL_CONFIG_FILE"
    # Define internal paths, expanding install-time variables
    echo "    local _MCPD_BIN='$_MCPD_BIN_PATH'" >> "$SHELL_CONFIG_FILE"
    echo "    local _MCPD_RUNTIME_DIR='$_MCPD_RUNTIME_BASE'" >> "$SHELL_CONFIG_FILE"
    echo "    local _MCPD_PID_FILE='$_MCPD_PID_FILE'" >> "$SHELL_CONFIG_FILE"
    echo "    local _MCPD_LOG_FILE='$_MCPD_LOG_FILE'" >> "$SHELL_CONFIG_FILE"
    echo "    local _MCPD_SOCKET_PATH=\"\${_MCPD_RUNTIME_DIR}/mcp-hostd.sock\"" >> "$SHELL_CONFIG_FILE"

    # Use quoted heredoc for the rest of the function body to prevent internal expansion
    cat << 'EOMCPDBODY' >> "$SHELL_CONFIG_FILE"

    # Colors (optional, remove if causing issues)
    local GREEN='\033[0;32m'
    local YELLOW='\033[1;33m'
    local RED='\033[0;31m'
    local BLUE='\033[0;34m'
    local NC='\033[0m' # No Color

    # Ensure runtime directory exists
    mkdir -p "$_MCPD_RUNTIME_DIR"

    _mcpd_get_pid() {
        if [ -f "$_MCPD_PID_FILE" ]; then
            cat "$_MCPD_PID_FILE"
        else
            echo ""
        fi
    }

    _mcpd_is_running() {
        local pid=$(_mcpd_get_pid)
        if [ -z "$pid" ]; then
            return 1 # Not running (no PID file)
        fi
        # Check if the process exists
        if ps -p $pid > /dev/null; then
            return 0 # Running (process exists)
        else
            # Process doesn't exist, cleanup stale PID file
            echo "${YELLOW}Warning: Stale PID file found ($_MCPD_PID_FILE). Cleaning up.${NC}" >&2
            rm -f "$_MCPD_PID_FILE"
            return 1 # Not running
        fi
    }

    _mcpd_start() {
        if _mcpd_is_running; then
            echo "${YELLOW}Daemon is already running (PID: $(_mcpd_get_pid)).${NC}"
            return 1
        fi

        if [ ! -x "$_MCPD_BIN" ]; then
            echo "${RED}Error: Daemon binary not found or not executable at $_MCPD_BIN${NC}" >&2
            return 1
        fi

        echo "${BLUE}Starting mcp-hostd daemon...${NC}"
        # Start in background, redirect stdout/stderr to log file
        "$_MCPD_BIN" &> "$_MCPD_LOG_FILE" & 
        local pid=$!
        echo $pid > "$_MCPD_PID_FILE"
        sleep 1 # Give it a moment to start
        if _mcpd_is_running; then
            echo "${GREEN}Daemon started successfully (PID: $pid).${NC}"
            echo "  Log file: $_MCPD_LOG_FILE"
            return 0
        else
            echo "${RED}Error: Failed to start the daemon. Check logs:${NC}"
            echo "  $_MCPD_LOG_FILE"
            rm -f "$_MCPD_PID_FILE" # Clean up pid file on failure
            return 1
        fi
    }

    _mcpd_stop() {
        if ! _mcpd_is_running; then
            echo "${YELLOW}Daemon is not running.${NC}"
            return 1
        fi

        local pid=$(_mcpd_get_pid)
        echo "${BLUE}Stopping mcp-hostd daemon (PID: $pid)...${NC}"
        kill $pid
        # Wait for process to terminate
        local count=0
        while ps -p $pid > /dev/null && [ $count -lt 10 ]; do
            sleep 0.5
            count=$((count + 1))
        done

        if ps -p $pid > /dev/null; then
            echo "${RED}Error: Daemon process $pid did not stop gracefully.${NC}"
            echo "${YELLOW}Consider using 'mcpd force-stop'.${NC}"
            return 1
        else
            rm -f "$_MCPD_PID_FILE"
            echo "${GREEN}Daemon stopped successfully.${NC}"
            return 0
        fi
    }

    _mcpd_force_stop() {
        if ! _mcpd_is_running; then
             # Try to remove potentially stale PID file even if not running
             if [ -f "$_MCPD_PID_FILE" ]; then
                echo "${YELLOW}Daemon not running, but removing stale PID file: $_MCPD_PID_FILE${NC}"
                rm -f "$_MCPD_PID_FILE"
             else
                echo "${YELLOW}Daemon is not running.${NC}"
             fi
            return 1
        fi

        local pid=$(_mcpd_get_pid)
        echo "${YELLOW}Force stopping mcp-hostd daemon (PID: $pid)...${NC}"
        kill -9 $pid
        sleep 0.5 # Give kill signal time
        if ps -p $pid > /dev/null; then
             echo "${RED}Error: Failed to force stop process $pid.${NC}" >&2
             return 1
        else
            rm -f "$_MCPD_PID_FILE"
            echo "${GREEN}Daemon force-stopped successfully.${NC}"
            return 0
        fi
    }

    case "$1" in
        start)
            _mcpd_start
            ;;
        stop)
            _mcpd_stop
            ;;
        force-stop|force)
            _mcpd_force_stop
            ;;
        restart)
            _mcpd_stop
            sleep 1
            _mcpd_start
            ;;
        reload) # Reload isn't supported, make it alias restart
             echo "${YELLOW}Reload not supported, performing restart...${NC}"
            _mcpd_stop
            sleep 1
            _mcpd_start
            ;;
        status)
            if _mcpd_is_running; then
                echo "${GREEN}Daemon is running (PID: $(_mcpd_get_pid)).${NC}"
            else
                echo "${RED}Daemon is not running.${NC}"
            fi
            ;;
        logs)
            echo "${BLUE}Showing logs (Ctrl+C to stop)...${NC}"
            if [ -f "$_MCPD_LOG_FILE" ]; then
                 tail -f "$_MCPD_LOG_FILE"
            else
                 echo "${YELLOW}Log file not found: $_MCPD_LOG_FILE${NC}"
                 echo "Try starting the daemon first ('mcpd start')."
            fi
            ;;
        *)
            echo "Usage: mcpd {start|stop|restart|reload|force-stop|status|logs}"
            return 1
            ;;
    esac

    return $?
EOMCPDBODY

    # Write the closing brace for the function
    echo "}" >> "$SHELL_CONFIG_FILE"
    # Write the end marker (variable is expanded)
    echo "$MCPD_FUNCTION_END" >> "$SHELL_CONFIG_FILE"

    # Verification step for mcpd function
    if awk "/$MCPD_FUNCTION_START/{found=1} END{exit !found}" "$SHELL_CONFIG_FILE"; then
        echo -e "${GREEN}✓ MCPD management function added successfully.${NC}"
    else
        echo -e "${RED}Error: Failed verification step after adding mcpd function to $SHELL_CONFIG_FILE.${NC}"
        # Don't exit, just warn
    fi
fi

# Show success message and usage
echo -e "\n${GREEN}=== ${INSTALL_ACTION} Complete! ===${NC}"

# PATH reminder if needed
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo -e "\n${YELLOW}Reminder: Ensure the installation directory ($INSTALL_DIR) is in your PATH.${NC}"
    echo -e "          This is needed for both the '$FUNCTION_NAME' wrapper and the '$MCP_HOSTD_BINARY_NAME' daemon.\"${NC}"
fi

echo -e "After reloading your shell, you can use Gemini CLI by running: ${BLUE}$FUNCTION_NAME \"your prompt\"${NC}"
echo -e "You can manage the MCP Host Daemon using: ${BLUE}$MCP_HOSTD_BINARY_NAME${NC}"
echo -e " (Note: Daemon is not started automatically by this script. See documentation.)"

if [ "$HAD_PREVIOUS_INSTALL" = false ]; then
    echo
    echo -e "${YELLOW}First Steps:${NC}"
    echo "1. Reload your shell (e.g., 'source ~/.zshrc' or new terminal)"
    echo "2. Set your API key: $FUNCTION_NAME --set-api-key YOUR_API_KEY"
    echo "3. Start the MCP Host Daemon (e.g., run '$MCP_HOSTD_BINARY_NAME &' in the background or set up a service)"
    echo "4. Try a test query: $FUNCTION_NAME \"Hello, Gemini!\""
    echo "5. For command help: $FUNCTION_NAME -c \"how to list all processes\""
fi

echo
echo -e "Run ${BLUE}$FUNCTION_NAME --help${NC} for more options."
if [ "$CURRENT_SHELL" = "zsh" ]; then
    echo -e "Manage the MCP Host Daemon with: ${BLUE}mcpd {start|stop|status|...}${NC}"
fi 