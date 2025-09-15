#!/usr/bin/env bash

# This script automates the process of building the Gemini CLI outside of Nix
# and then packaging it using Nix.

set -e # Exit immediately if a command exits with a non-zero status.

echo "Step 1: Running npm install..."
npm install

echo "Step 2: Running npm run build..."
npm run build

echo "Step 3: Running nix build to package the project..."
nix build

echo "Build and packaging complete. You can now run ./run-gemini.sh"
