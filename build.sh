#!/bin/bash
set -e

# Pinned tool versions (must match .github/workflows/ci.yml and docs).
WASM_PACK_VERSION="0.15.0"
# Dev-server default port (must match server.ts DEFAULT_PORT).
DEFAULT_PORT="8001"

echo "=== subrass Build Script ==="
echo ""

# Check if wasm-pack is installed
if ! command -v wasm-pack &> /dev/null; then
    echo "Installing wasm-pack ${WASM_PACK_VERSION}..."
    cargo install wasm-pack --version "$WASM_PACK_VERSION" --locked
fi

# Build for web target
echo "Building WASM for web target..."
wasm-pack build --target web --release

echo ""
echo "Build complete!"
echo ""
echo "To start the demo server (transpiles TypeScript on the fly):"
echo "  bun run server.ts"
echo ""
echo "Then open http://localhost:${PORT:-$DEFAULT_PORT}"
