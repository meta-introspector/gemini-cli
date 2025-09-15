#!/usr/bin/env bash
set -euo pipefail

# Define the target directory for the new Rust crate
RUST_VENDOR_DIR="rust-vendor"
RUST_CRATE_NAME="rust-vendor-lib"
FULL_CRATE_PATH="${RUST_VENDOR_DIR}/${RUST_CRATE_NAME}"

# Function to create the Rust crate if it doesn't exist
create_rust_crate() {
  if [ ! -d "${FULL_CRATE_PATH}" ]; then
    echo "Creating new Rust library crate: ${FULL_CRATE_PATH}"
    mkdir -p "${RUST_VENDOR_DIR}"
    (cd "${RUST_VENDOR_DIR}" && cargo new --lib "${RUST_CRATE_NAME}")
  else
    echo "Rust library crate already exists: ${FULL_CRATE_PATH}"
  fi
}

# Function to add a dependency to the Rust crate
add_dependency() {
  local dep_name="$1"
  echo "Adding dependency: ${dep_name}"
  (cd "${FULL_CRATE_PATH}" && cargo add "${dep_name}")
}

# Main execution
create_rust_crate

add_dependency "nrr"
add_dependency "bole"
add_dependency "please-install"
add_dependency "santa"

echo "All dependencies added."
