#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR=$(dirname "$(readlink -f "$0")")
ORIGINAL_PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Create a temporary directory for the test
TEST_DIR=$(mktemp -d)

echo "Running pre-commit nix-statix test in: $TEST_DIR"

# Function to clean up the temporary directory
cleanup() {
  echo "Cleaning up $TEST_DIR"
  rm -rf "$TEST_DIR"
}

trap cleanup EXIT

# Initialize a Git repository
cd "$TEST_DIR"
git init -b main

# Copy necessary files
mkdir -p .husky scripts
cp "$ORIGINAL_PROJECT_ROOT/.husky/pre-commit" .husky/
cp "$ORIGINAL_PROJECT_ROOT/scripts/pre-commit.js" scripts/
cat << EOF > flake.nix
{
  description = "Test flake";

  inputs = {
    nixpkgs.url = "github:meta-introspector/nixpkgs?ref=feature/CRQ-016-nixify";
    flake-utils.url = "github:meta-introspector/flake-utils?ref=feature/CRQ-016-nixify";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            pkgs.statix # Use pkgs.statix as found in the project's flake.nix
            pkgs.nodejs_22 # Required for npm and node scripts
          ];
        };
      }
    );
}
EOF

# Create a minimal package.json for lint-staged
cat << EOF > package.json
{
  "name": "test-project",
  "version": "1.0.0",
  "type": "module",
  "devDependencies": {
    "lint-staged": "^16.1.6"
  },
  "lint-staged": {
    "*.nix": [
      "statix fix --check"
    ],
    "*.{js,jsx,ts,tsx}": [
      "true"
    ]
  }
}
EOF

# Install npm dependencies for lint-staged to work
npm install

# Add and commit initial files
git add .husky/pre-commit scripts/pre-commit.js flake.nix package.json
git commit -m "Initial commit"

# Explicitly update flake.lock before running pre-commit hook
nix flake update

# --- Test Case 1: Malformed Nix file (should fail) ---
echo "Testing with malformed Nix file..."

MALFORMED_NIX_FILE="test.nix"
cat << EOF > "$MALFORMED_NIX_FILE"
{
  bad-attribute = "value";
}
EOF

git add "$MALFORMED_NIX_FILE"

# Run the pre-commit hook (should fail)
if ! .husky/pre-commit; then
  echo "✓ Pre-commit hook failed as expected for malformed Nix file."
else
  echo "✗ Pre-commit hook unexpectedly passed for malformed Nix file."
  exit 1
fi

# --- Test Case 2: Correctly formatted Nix file (should pass) ---
echo "Testing with correctly formatted Nix file..."

cat << EOF > "$MALFORMED_NIX_FILE"
{
  goodAttribute = "value";
}
EOF

git add "$MALFORMED_NIX_FILE"

# Run the pre-commit hook (should pass)
if .husky/pre-commit; then
  echo "✓ Pre-commit hook passed as expected for correctly formatted Nix file."
else
  echo "✗ Pre-commit hook unexpectedly failed for correctly formatted Nix file."
  exit 1
fi

echo "All pre-commit nix-statix tests passed!"
