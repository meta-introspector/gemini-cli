#!/usr/bin/env bash
set -euo pipefail

# Define the root directory of the parent project
PARENT_PROJECT_ROOT="/data/data/com.termux.nix/files/home/pick-up-nix"

echo "Changing directory to ${PARENT_PROJECT_ROOT} and launching Gemini CLI..."
cd "${PARENT_PROJECT_ROOT}"

# Launch the Gemini CLI
nix run nixpkgs/master#gemini-cli