#!/bin/bash

# Script to easily view Gemini Suite daemon logs
# Shows logs for all daemons or a specific daemon

set -e

# ANSI color codes
BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Define runtime directories - updated to match actual location
runtime_dir="${XDG_RUNTIME_DIR:-$HOME/.local/share}/gemini-suite/gemini-suite-logs"

# Create logs directory if it doesn't exist
mkdir -p "$runtime_dir"

# Usage function
usage() {
    echo -e "${BLUE}Usage:${NC} $0 [daemon_name]"
    echo
    echo "View logs for Gemini Suite daemons."
    echo
    echo -e "${BLUE}Options:${NC}"
    echo "  <daemon_name>    Name of the daemon to view logs for (happe, ida, mcp-hostd)"
    echo "  No argument      View logs for all daemons"
    echo
    echo -e "${BLUE}Examples:${NC}"
    echo "  $0 happe         View logs for HAPPE daemon"
    echo "  $0               View logs for all daemons"
    exit 1
}

# Check if tail command exists
if ! command -v tail &> /dev/null; then
    echo -e "${RED}Error: 'tail' command not found. Please install it to use this script.${NC}"
    exit 1
fi

# Function to view logs for a single daemon
view_daemon_logs() {
    local daemon=$1
    local log_file="$runtime_dir/$daemon.log"
    
    if [ -f "$log_file" ]; then
        echo -e "${GREEN}=== Logs for $daemon daemon ===${NC}"
        echo -e "${YELLOW}Log file: $log_file${NC}"
        echo
        cat "$log_file"
        echo
    else
        echo -e "${RED}No log file found for $daemon daemon. Has it been started with recent changes?${NC}"
        echo -e "${YELLOW}Expected log file: $log_file${NC}"
    fi
}

# No arguments, view all logs
if [ $# -eq 0 ]; then
    # Check if any log files exist
    if [ ! "$(ls -A $runtime_dir 2>/dev/null)" ]; then
        echo -e "${YELLOW}No log files found in $runtime_dir.${NC}"
        echo -e "${YELLOW}Have you started the daemons with the updated logging support?${NC}"
        echo -e "${BLUE}Try: gemini-manager stop && gemini-manager start${NC}"
        exit 0
    fi
    
    # View logs for all known daemons
    for daemon in happe ida mcp-hostd; do
        view_daemon_logs "$daemon"
    done
    
    # List any other log files
    other_logs=$(find "$runtime_dir" -name "*.log" -not -name "happe.log" -not -name "ida.log" -not -name "mcp-hostd.log")
    if [ -n "$other_logs" ]; then
        echo -e "${GREEN}=== Other log files ===${NC}"
        echo "$other_logs"
    fi
    
    exit 0
fi

# Specific daemon provided
daemon=$1
case "$daemon" in
    happe|ida|mcp-hostd)
        view_daemon_logs "$daemon"
        exit 0
        ;;
    -h|--help)
        usage
        ;;
    *)
        echo -e "${RED}Error: Unknown daemon '$daemon'${NC}"
        echo -e "${YELLOW}Supported daemons: happe, ida, mcp-hostd${NC}"
        usage
        ;;
esac 