# MCPD Management Function Start
# Function to manage the mcp-hostd daemon
mcpd() {
    local _DAEMON_BIN='/home/james/.local/bin/mcp-hostd'
    local _DAEMON_CONFIG_DIR='/home/james/.config/gemini-suite'
    local _DAEMON_RUNTIME_DIR='${XDG_RUNTIME_DIR:-$HOME/.local/share}/gemini-suite'
    local _DAEMON_PID_FILE="$_DAEMON_RUNTIME_DIR/mcp-hostd.pid"
    local _DAEMON_LOG_FILE="$_DAEMON_RUNTIME_DIR/mcp-hostd.log"
    local _DAEMON_SOCKET_PATH="$_DAEMON_RUNTIME_DIR/mcp-hostd.sock"

    # Colors for terminal output
    local GREEN='\033[0;32m'
    local YELLOW='\033[1;33m'
    local RED='\033[0;31m'
    local BLUE='\033[0;34m'
    local NC='\033[0m' # No Color

    # Ensure runtime directory exists
    mkdir -p "$_DAEMON_RUNTIME_DIR"

    _daemon_get_pid() {
        if [ -f "$_DAEMON_PID_FILE" ]; then
            cat "$_DAEMON_PID_FILE"
        else
            echo ""
        fi
    }

    _daemon_is_running() {
        local pid=$(_daemon_get_pid)
        if [ -z "$pid" ]; then
            return 1 # Not running (no PID file)
        fi
        # Check if the process exists
        if ps -p $pid > /dev/null; then
            return 0 # Running (process exists)
        else
            # Process doesn't exist, cleanup stale PID file
            echo "${YELLOW}Warning: Stale PID file found ($_DAEMON_PID_FILE). Cleaning up.${NC}" >&2
            rm -f "$_DAEMON_PID_FILE"
            return 1 # Not running
        fi
    }

    _daemon_start() {
        if _daemon_is_running; then
            echo "${YELLOW}Daemon is already running (PID: $(_daemon_get_pid)).${NC}"
            return 1
        fi

        if [ ! -x "$_DAEMON_BIN" ]; then
            echo "${RED}Error: Daemon binary not found or not executable at $_DAEMON_BIN${NC}" >&2
            return 1
        fi

        echo "${BLUE}Starting mcp-hostd daemon...${NC}"
        
        # Create a temporary wrapper script to modify environment
        local wrapper_script=$(mktemp)
        echo '#!/bin/sh' > "$wrapper_script"
        echo "# Wrapper script to prevent GEMINI_CONFIG_DIR leak to MCP servers" >> "$wrapper_script"
        echo "export MCP_HOST_CONFIG_DIR=\"$_DAEMON_CONFIG_DIR\"" >> "$wrapper_script"
        echo "unset GEMINI_CONFIG_DIR" >> "$wrapper_script"
        echo "exec \"$_DAEMON_BIN\" --config \"$_DAEMON_CONFIG_DIR/mcp/config.toml\"" >> "$wrapper_script"
        chmod +x "$wrapper_script"
        
        # Start in background, redirect stdout/stderr to log file
        "$wrapper_script" &> "$_DAEMON_LOG_FILE" & 
        local pid=$!
        echo $pid > "$_DAEMON_PID_FILE"
        
        # Remove the temporary wrapper script
        rm -f "$wrapper_script"
        
        sleep 1 # Give it a moment to start
        if _daemon_is_running; then
            echo "${GREEN}Daemon started successfully (PID: $pid).${NC}"
            echo "  Log file: $_DAEMON_LOG_FILE"
            return 0
        else
            echo "${RED}Error: Failed to start the daemon. Check logs:${NC}"
            echo "  $_DAEMON_LOG_FILE"
            rm -f "$_DAEMON_PID_FILE" # Clean up pid file on failure
            return 1
        fi
    }

    _daemon_stop() {
        if ! _daemon_is_running; then
            echo "${YELLOW}Daemon is not running.${NC}"
            return 1
        fi

        local pid=$(_daemon_get_pid)
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
            rm -f "$_DAEMON_PID_FILE"
            echo "${GREEN}Daemon stopped successfully.${NC}"
            return 0
        fi
    }

    _daemon_force_stop() {
        if ! _daemon_is_running; then
             # Try to remove potentially stale PID file even if not running
             if [ -f "$_DAEMON_PID_FILE" ]; then
                echo "${YELLOW}Daemon not running, but removing stale PID file: $_DAEMON_PID_FILE${NC}"
                rm -f "$_DAEMON_PID_FILE"
             else
                echo "${YELLOW}Daemon is not running.${NC}"
             fi
            return 1
        fi

        local pid=$(_daemon_get_pid)
        echo "${YELLOW}Force stopping mcp-hostd daemon (PID: $pid)...${NC}"
        kill -9 $pid
        sleep 0.5 # Give kill signal time
        if ps -p $pid > /dev/null; then
             echo "${RED}Error: Failed to force stop process $pid.${NC}" >&2
             return 1
        else
            rm -f "$_DAEMON_PID_FILE"
            echo "${GREEN}Daemon force-stopped successfully.${NC}"
            return 0
        fi
    }

    case "$1" in
        start)
            _daemon_start
            ;;
        stop)
            _daemon_stop
            ;;
        force-stop|force)
            _daemon_force_stop
            ;;
        restart)
            _daemon_stop
            sleep 1
            _daemon_start
            ;;
        reload)
            echo "${YELLOW}Reload not supported, performing restart...${NC}"
            _daemon_stop
            sleep 1
            _daemon_start
            ;;
        status)
            if _daemon_is_running; then
                echo "${GREEN}Daemon is running (PID: $(_daemon_get_pid)).${NC}"
            else
                echo "${RED}Daemon is not running.${NC}"
            fi
            ;;
        logs)
            echo "${BLUE}Showing logs (Ctrl+C to stop)...${NC}"
            if [ -f "$_DAEMON_LOG_FILE" ]; then
                 tail -f "$_DAEMON_LOG_FILE"
            else
                 echo "${YELLOW}Log file not found: $_DAEMON_LOG_FILE${NC}"
                 echo "Try starting the daemon first ('mcpd start')."
            fi
            ;;
        *)
            echo "Usage: mcpd {start|stop|restart|reload|force-stop|status|logs}"
            return 1
            ;;
    esac

    return $?
}
# MCPD Management Function End 