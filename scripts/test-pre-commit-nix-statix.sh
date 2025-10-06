#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR=$(dirname "$(readlink -f "$0")")
ORIGINAL_PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Create a temporary directory for the test
TEST_DIR=$(mktemp -d)

echo "Running statix test in: $TEST_DIR"

# Function to clean up the temporary directory
cleanup() {
  echo "Cleaning up $TEST_DIR"
  rm -rf "$TEST_DIR"
}

trap cleanup EXIT

# Create a simplified flake.nix for the test environment
cat << EOF > "$TEST_DIR/flake.nix"
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
          ];
        };
      }
    );
}
EOF

# --- Test Case 1: Malformed Nix file (should fail) ---
echo "Testing with malformed Nix file..."

MALFORMED_NIX_FILE="$TEST_DIR/malformed.nix"
cat << EOF > "$MALFORMED_NIX_FILE"
{
  bad-attribute = "value";
  unclosed-brace = {
EOF

# Run statix (should fail) and capture output
STATIX_OUTPUT=$(nix develop "$TEST_DIR" --command bash -c "statix check $MALFORMED_NIX_FILE" 2>&1 || true)

if echo "$STATIX_OUTPUT" | grep -q "error"; then
  echo "✓ statix failed as expected for malformed Nix file."
else
  echo "✗ statix unexpectedly passed for malformed Nix file."
  echo "Statix output:"
  echo "$STATIX_OUTPUT"
  exit 1
fi

# --- Test Case 2: Correctly formatted Nix file (should pass) ---
echo "Testing with correctly formatted Nix file..."

CORRECT_NIX_FILE="$TEST_DIR/correct.nix"
cat << EOF > "$CORRECT_NIX_FILE"
{
  goodAttribute = "value";
}
EOF

# Run statix (should pass) and capture output
STATIX_OUTPUT=$(nix develop "$TEST_DIR" --command bash -c "statix check $CORRECT_NIX_FILE" 2>&1 || true)

if ! echo "$STATIX_OUTPUT" | grep -q "error"; then
  echo "✓ statix passed as expected for correctly formatted Nix file."
else
  echo "✗ statix unexpectedly failed for correctly formatted Nix file."
  echo "Statix output:"
  echo "$STATIX_OUTPUT"
  exit 1
fi

echo "All statix tests passed!"
