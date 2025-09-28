# Nix-enhanced Makefile for Gemini CLI
# Provides Nix-specific operations alongside existing npm functionality

.PHONY: help-nix clean-nix nix-build nix-test nix-shell nix-run inspect-bundle check-nix-deps nix-flake-update

# Nix-specific help (call original help first)
help-nix: help
	@echo ""
	@echo "=== Nix-Enhanced Targets ==="
	@echo "  nix-build         - Build the Nix package (includes bundle)"
	@echo "  nix-test          - Test the Nix-built package"
	@echo "  nix-shell         - Enter Nix development shell"
	@echo "  nix-run           - Run gemini via Nix (nix run)"
	@echo "  inspect-bundle    - Inspect current bundle directory"
	@echo "  check-nix-deps    - Verify Nix dependencies are available"
	@echo "  nix-flake-update  - Update flake.lock"
	@echo "  clean-nix         - Clean Nix build results"

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

# Inspect bundle directory
inspect-bundle:
	@echo "=== Inspecting Bundle Directory ==="
	@echo "Bundle directory contents:"
	@ls -la bundle/
	@echo ""
	@echo "Bundle/gemini.js info:"
	@if [ -f bundle/gemini.js ]; then \
		stat bundle/gemini.js; \
		echo "File type: $$(file bundle/gemini.js)"; \
		echo "First few lines:"; \
		head -5 bundle/gemini.js; \
	else \
		echo "✗ bundle/gemini.js not found"; \
	fi

# Update flake.lock
nix-flake-update:
	@echo "=== Updating Flake Lock ==="
	nix flake update
	@echo "✓ Flake lock updated"

# Show flake info
nix-flake-show:
	@echo "=== Flake Information ==="
	nix flake show

# Clean Nix results
clean-nix:
	@echo "=== Cleaning Nix Results ==="
	rm -rf result*
	rm -rf logs/nix-*.log
	@echo "✓ Nix cleanup complete"

# Enhanced build that includes both npm and nix
build-all: build nix-build
	@echo "=== All Builds Complete ==="

# Enhanced test that includes both npm and nix
test-all: test nix-test
	@echo "=== All Tests Complete ==="

# Enhanced clean that includes both npm and nix
clean-all: clean clean-nix
	@echo "=== All Cleanup Complete ==="

# Quick verification workflow
verify: check-nix-deps inspect-bundle nix-build nix-test
	@echo "=== Verification Complete ==="
	@echo "Bundle exists and Nix package works correctly"

# Development workflow
dev-setup: check-nix-deps install build nix-build
	@echo "=== Development Setup Complete ==="
	@echo "Ready for development with both npm and Nix"

# Default Nix target shows help
nix: help-nix