#!/usr/bin/env bash

# This script calls the Nix-built Gemini CLI.
# It assumes that 'nix build' has been run successfully in the project root.

# Path to the Nix-built Gemini executable
GEMINI_CLI="$(pwd)/result/bin/gemini"

# Check if the executable exists
if [ ! -f "$GEMINI_CLI" ]; then
  echo "Error: Gemini CLI executable not found at $GEMINI_CLI"
  echo "Please ensure 'nix build' has been run successfully."
  exit 1
fi

# Execute the Gemini CLI with all arguments passed to this script
exec "$GEMINI_CLI" "$@"
