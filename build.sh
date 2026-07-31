#!/bin/bash
set -e

echo "=== subrass Build Script ==="
echo ""

# Check if wasm-pack is installed
if ! command -v wasm-pack &> /dev/null; then
    echo "Installing wasm-pack..."
    cargo install wasm-pack
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
echo "Then open http://localhost:3001"
