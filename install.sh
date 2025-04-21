#!/usr/bin/env bash
set -e

# Colors for better readability
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )" # Directory of the install script
VERBOSE=0

# Process command line arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    -v|--verbose)
      VERBOSE=1
      shift
      ;;
    *)
      # Pass remaining arguments to the installer
      break
      ;;
  esac
done

# Verbose logging function
log_verbose() {
  if [ $VERBOSE -eq 1 ]; then
    echo -e "${BLUE}[VERBOSE] $1${NC}"
  fi
}

echo -e "${BLUE}=== Gemini CLI Suite Installer ===${NC}"

# Check if rust is installed
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: Rust/Cargo is not installed.${NC}"
    echo "To install Rust, please visit: https://rustup.rs/"
    exit 1
fi

# Check if we are in the workspace structure
if [ -f "$SCRIPT_DIR/Cargo.toml" ] && grep -q "workspace" "$SCRIPT_DIR/Cargo.toml"; then
    WORKSPACE_DIR="$SCRIPT_DIR"
    echo "Detected workspace structure in $WORKSPACE_DIR"
    
    log_verbose "Checking workspace members:"
    if [ $VERBOSE -eq 1 ]; then
      grep -A 10 "members" "$WORKSPACE_DIR/Cargo.toml"
    fi
else
    echo -e "${RED}Error: Not in a workspace root directory. Please run this script from the workspace root directory containing Cargo.toml.${NC}"
    exit 1
fi

# Build the installer binary
echo -e "\n${YELLOW}Building installer binary...${NC}"

# Make sure the installer package has the right dependencies
log_verbose "Checking installer dependencies:"
if [ $VERBOSE -eq 1 ]; then
  cat "$WORKSPACE_DIR/install/Cargo.toml"
fi

# Build command with extra verbose output if requested
BUILD_CMD="cargo build --release --bin gemini-installer"
if [ $VERBOSE -eq 1 ]; then
  BUILD_CMD="$BUILD_CMD -v"
fi

(cd "$WORKSPACE_DIR" && eval "$BUILD_CMD")
if [ $? -ne 0 ]; then
    echo -e "${RED}Error: Failed to build installer binary.${NC}"
    echo -e "${RED}Try running with -v or --verbose for more information.${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Installer binary built successfully${NC}"

# Define the path to the installer binary
INSTALLER_BIN="$WORKSPACE_DIR/target/release/gemini-installer"

# Verify the binary exists
if [ ! -f "$INSTALLER_BIN" ]; then
    echo -e "${RED}Error: Installer binary not found at $INSTALLER_BIN${NC}"
    echo -e "Checking target/release directory contents:"
    ls -la "$WORKSPACE_DIR/target/release/"
    exit 1
fi

# Make the installer binary executable
chmod +x "$INSTALLER_BIN"

# Copy the installer binary to the workspace root
cp "$INSTALLER_BIN" "$WORKSPACE_DIR/gemini-installer"
if [ ! -f "$WORKSPACE_DIR/gemini-installer" ]; then
    echo -e "${RED}Error: Failed to copy installer binary to $WORKSPACE_DIR/gemini-installer${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Installer binary copied to $WORKSPACE_DIR/gemini-installer${NC}"

# Run the installer with potential verbose flag
INSTALLER_CMD="$WORKSPACE_DIR/gemini-installer"
if [ $VERBOSE -eq 1 ]; then
    INSTALLER_CMD="$INSTALLER_CMD -v"
fi

# Add other arguments
for arg in "$@"; do
    INSTALLER_CMD="$INSTALLER_CMD $arg"
done

echo -e "\n${YELLOW}Running installer: $INSTALLER_CMD${NC}"
eval "$INSTALLER_CMD"

EXIT_CODE=$?
if [ $EXIT_CODE -ne 0 ]; then
    echo -e "\n${RED}Installation failed with exit code $EXIT_CODE${NC}"
    echo -e "${RED}Try running with -v or --verbose for more information.${NC}"
    exit $EXIT_CODE
fi

echo -e "\n${GREEN}Installation process completed.${NC}"
echo -e "Please reload your shell (or start a new terminal) to use the installed components." 