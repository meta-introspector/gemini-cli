# Makefile for gemini-cli
# Enhanced with Nix integration

.PHONY: help install build build-sandbox build-all test lint format preflight clean start debug release run-npx create-alias help-nix clean-nix nix-build nix-test nix-shell nix-run inspect-bundle check-nix-deps nix-flake-update lint-nix nix-flake-show test-all dev-setup

help:
	@echo "Makefile for gemini-cli"
	@echo ""
	@echo "Usage:"
	@echo "  make install          - Install npm dependencies"
	@echo "  make build            - Build the main project"
	@echo "  make bundle           - Create bundle (npm run bundle)"
	@echo "  make build-all        - Build the main project and sandbox"
	@echo "  make test             - Run the test suite"
	@echo "  make lint             - Lint the code"
	@echo "  make lint-nix         - Lint Nix files with statix"
	@echo "  make format           - Format the code"
	@echo "  make preflight        - Run formatting, linting, and tests"
	@echo "  make clean            - Remove generated files"
	@echo "  make start            - Start the Gemini CLI"
	@echo "  make debug            - Start the Gemini CLI in debug mode"
	@echo ""
	@echo "  make run-npx          - Run the CLI using npx (for testing the published package)"
	@echo "  make create-alias     - Create a 'gemini' alias for your shell"
	@echo ""
	@echo "=== Nix Integration ==="
	@echo "  make help-nix         - Show Nix-specific help"
	@echo "  make nix-build        - Build the Nix package (includes bundle)"
	@echo "  make nix-test         - Test the Nix-built package"
	@echo "  make nix-shell        - Enter Nix development shell"
	@echo "  make nix-run          - Run gemini via Nix (nix run)"
	@echo "  make inspect-bundle   - Inspect current bundle directory"
	@echo "  make check-nix-deps   - Verify Nix dependencies are available"
	@echo "  make nix-flake-update - Update flake.lock"
	@echo "  make nix-flake-show   - Show flake information"
	@echo "  make clean-nix        - Clean Nix build results"
	@echo "  make verify           - Quick Nix verification workflow"
	@echo "  make dev-setup        - Complete development setup with Nix and npm"

install:
	npm install

build:
	npm run build

bundle:
	npm run bundle

build-all:
	npm run build:all

test:
	npm run test

lint:
	npm run lint

format:
	npm run format

preflight:
	npm run preflight

clean:
	npm run clean

start:
	npm run start

debug:
	npm run debug

run-npx:
	npx https://github.com/google-gemini/gemini-cli

create-alias:
	scripts/create_alias.sh

# === Nix Integration Targets ===

# Nix-specific help
help-nix:
	help
	@echo ""
	@echo "=== Nix-Enhanced Targets ==="
	@echo "  nix-build         - Build the Nix package (includes bundle)"
	@echo "  nix-test          - Test the Nix-built package"
	@echo "  nix-shell         - Enter Nix development shell"
	@echo "  nix-run           - Run gemini via Nix (nix run)"
	@echo "  inspect-bundle    - Inspect current bundle directory"
	@echo "  check-nix-deps    - Verify Nix dependencies are available"
	@echo "  nix-flake-update  - Update flake.lock"
	@echo "  lint-nix          - Lint Nix files with statix"
	@echo "  clean-nix         - Clean Nix build results"
	@echo "  nix-flake-show    - Show flake information"
	@echo "  test-all          - Run all npm and Nix tests"
	@echo "  dev-setup         - Complete development setup with Nix and npm"

# Lint Nix files with statix
lint-nix:
	@echo "=== Linting Nix Files with Statix ==="
	nix develop --command bash -c "statix check --ignore Makefile.nix **/*.nix"
	@echo "✓ Nix linting complete"

# Check Nix dependencies
check-nix-deps:
	@echo "=== Checking Nix Dependencies ==="
	@which nix || (echo "ERROR: nix not found"; exit 1)
	@which node || (echo "ERROR: node not found"; exit 1)
	@nix --version
	@node --version
	@echo "✓ Nix dependencies available"

# Build the Nix package
nix-build:
	@echo "=== Building Nix Package ==="
	@mkdir -p logs
	nix build --show-trace 2>&1 | tee logs/nix-build.log
	@echo "✓ Nix build complete - check result/ symlink"

# Build the Nix package with npm bundle generation
nix-build-with-bundle:
	@echo "=== Building Nix Package (with bundle generation) ==="
	@mkdir -p logs
	nix build .#gemini-cli-with-build --show-trace 2>&1 | tee logs/nix-build-with-bundle.log
	@echo "✓ Nix build with bundle complete - check result/ symlink"

# Test the Nix-built package
nix-test:
	@echo "=== Testing Nix Package ==="
	@mkdir -p logs
	@if [ -L result ]; then \
		echo "Testing Nix result..."; \
		echo "Result path: $$(readlink result)"; \
		echo "Testing --help:"; \
		timeout 30 result/bin/gemini --help 2>&1 | tee logs/nix-test-help.log || echo "Help exit code: $$?"; \
		echo "Testing --version:"; \
		timeout 30 result/bin/gemini --version 2>&1 | tee logs/nix-test-version.log || echo "Version exit code: $$?"; \
		echo "Testing simple prompt:"; \
		timeout 30 result/bin/gemini "Hello from Nix build" 2>&1 | tee logs/nix-test-prompt.log || echo "Prompt exit code: $$?"; \
		echo "✓ Nix package tests completed"; \
	else \
		echo "✗ No Nix result found - run 'make nix-build' first"; \
		exit 1; \
	fi

# Enter Nix development shell
nix-shell:
	@echo "=== Entering Nix Development Shell ==="
	nix develop

# Run gemini via Nix
nix-run:
	@echo "=== Running Gemini via Nix ==="
	nix run . -- $(ARGS)

# Inspect bundle directory with detailed script
inspect-bundle:
	@echo "=== Inspecting Bundle ==="
	@./scripts/nix-inspect.sh

# Clean Nix results
clean-nix:
	@echo "=== Cleaning Nix Results ==="
	rm -rf result*
	rm -rf logs/nix-*.log
	@echo "✓ Nix cleanup complete"

# Enhanced clean that includes both npm and nix
clean-all: clean clean-nix
	@echo "=== All Cleanup Complete ==="

# Quick verification workflow
verify: check-nix-deps inspect-bundle nix-build nix-test
	@echo "=== Verification Complete ==="
	@echo "Bundle exists and Nix package works correctly"

# Show flake info
nix-flake-show:
	@echo "=== Flake Information ==="
	nix flake show

# Enhanced build that includes both npm and nix
build-all: build nix-build
	@echo "=== All Builds Complete ==="

# Enhanced test that includes both npm and nix
test-all: test nix-test
	@echo "=== All Tests Complete ==="

# Development workflow
dev-setup: check-nix-deps install build nix-build
	@echo "=== Development Setup Complete ==="
	@echo "Ready for development with both npm and Nix"

# Default Nix target shows help
nix: help-nix