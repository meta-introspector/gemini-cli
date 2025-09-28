#!/usr/bin/env bash
# Nix inspection script for Gemini CLI
# Provides detailed analysis of Nix build and bundle status

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
LOGS_DIR="$PROJECT_DIR/logs"

# Ensure logs directory exists
mkdir -p "$LOGS_DIR"

echo "=== Gemini CLI Nix Inspector ==="
echo "Timestamp: $(date -Iseconds)"
echo "Script: $0"
echo "Project Dir: $PROJECT_DIR" 
echo ""

# Function to check if file exists and get info
check_file() {
    local file="$1"
    local desc="$2"
    
    if [ -f "$file" ]; then
        local size=$(stat -c%s "$file" 2>/dev/null || echo "unknown")
        local readable_size=$(numfmt --to=iec "$size" 2>/dev/null || echo "$size bytes")
        echo "✓ $desc found: $file ($readable_size)"
        return 0
    else
        echo "✗ $desc missing: $file"
        return 1
    fi
}

# Function to check directory
check_directory() {
    local dir="$1"
    local desc="$2"
    
    if [ -d "$dir" ]; then
        echo "✓ $desc exists: $dir"
        return 0
    else
        echo "✗ $desc missing: $dir"
        return 1
    fi
}

echo "=== Step 1: Basic File Checks ==="
check_file "$PROJECT_DIR/flake.nix" "Flake configuration"
check_file "$PROJECT_DIR/package.json" "Package configuration"
check_directory "$PROJECT_DIR/bundle" "Bundle directory"
check_file "$PROJECT_DIR/bundle/gemini.js" "Bundle gemini.js"

echo ""
echo "=== Step 2: Nix Build Status ==="
cd "$PROJECT_DIR"

if [ -L result ]; then
    result_path=$(readlink result)
    echo "✓ Nix result found: $result_path"
    
    echo "Result structure:"
    find result -type f -name "gemini*" 2>/dev/null | head -10
    
    echo "Checking result/bin/:"
    ls -la result/bin/ 2>/dev/null || echo "No bin directory in result"
    
    echo "Checking result/share/:"
    find result/share -name "*gemini*" 2>/dev/null | head -5 || echo "No gemini files in share"
    
else
    echo "✗ No Nix result found"
fi

echo ""
echo "=== Step 3: Bundle Analysis ==="
if [ -f bundle/gemini.js ]; then
    echo "Bundle gemini.js analysis:"
    echo "Size: $(stat -c%s bundle/gemini.js | numfmt --to=iec)"
    echo "Permissions: $(stat -c%A bundle/gemini.js)"
    echo "Modified: $(stat -c%y bundle/gemini.js)"
    
    echo "File type:"
    file bundle/gemini.js
    
    echo "Executable test:"
    if [ -x bundle/gemini.js ]; then
        echo "✓ gemini.js is executable"
    else
        echo "✗ gemini.js is not executable"
    fi
    
    echo "Node.js syntax check:"
    if node --check bundle/gemini.js 2>/dev/null; then
        echo "✓ JavaScript syntax is valid"
    else
        echo "✗ JavaScript syntax errors found"
    fi
else
    echo "✗ bundle/gemini.js not found"
fi

echo ""
echo "=== Step 4: Flake Information ==="
echo "Flake outputs:"
if ! nix flake show 2>&1 | head -20; then
    echo "Warning: flake show failed, trying basic info"
    nix flake metadata 2>/dev/null || echo "Flake metadata unavailable"
fi

echo ""
echo "=== Step 5: Package Information ==="
if [ -f package.json ]; then
    echo "Package name: $(jq -r '.name // "unknown"' package.json)"
    echo "Version: $(jq -r '.version // "unknown"' package.json)"
    echo "Bin entry: $(jq -r '.bin // "none"' package.json)"
    
    if jq -e '.scripts.bundle' package.json >/dev/null 2>&1; then
        echo "✓ Bundle script available"
        echo "Bundle command: $(jq -r '.scripts.bundle' package.json)"
    else
        echo "✗ No bundle script found"
    fi
fi

echo ""
echo "=== Step 6: Build Logs ==="
if [ -f "$LOGS_DIR/nix-build.log" ]; then
    echo "Recent Nix build log entries:"
    tail -10 "$LOGS_DIR/nix-build.log"
else
    echo "No Nix build log found"
fi

echo ""
echo "=== Inspection Complete ==="
echo "For full build: make nix-build"
echo "For testing: make nix-test"
echo "Logs location: $LOGS_DIR/"