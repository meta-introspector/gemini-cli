#!/bin/bash

# This script patches the mcpd function in .zshrc to prevent GEMINI_CONFIG_DIR from being passed to MCP servers

# Create a backup of the original .zshrc
cp ~/.zshrc ~/.zshrc.bak

# Use sed to replace the line that sets GEMINI_CONFIG_DIR with a line that doesn't pass it to MCP servers
sed -i '/export GEMINI_CONFIG_DIR="$_DAEMON_CONFIG_DIR"/c\        # We explicitly do not export GEMINI_CONFIG_DIR to prevent the HAPPE client from being triggered' ~/.zshrc

echo "Patched .zshrc to prevent GEMINI_CONFIG_DIR leakage to MCP servers"
echo "A backup of the original file has been saved as ~/.zshrc.bak" 