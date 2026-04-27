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
echo "To start development server:"
echo "  miniserve . --port 8080"
echo ""
echo "Or use Python:"
echo "  python -m http.server 8080"
echo ""
echo "Then open http://localhost:8080/demo/"
