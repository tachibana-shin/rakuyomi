#!/usr/bin/env bash

# Exit immediately if a command exits with a non-zero status
set -e

echo "=================================================="
echo "🚀 Starting Settings Schema Generation on Ubuntu"
echo "=================================================="

if ! command -v cargo &> /dev/null; then
    echo "⚠️  Cargo not found. Installing Rust via rustup..."
    sudo apt update && sudo apt install -y curl build-essential musl-tools
    curl --proto '=https' --tlsv1.2 -sSf https://rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    echo "✅ Cargo detected: $(cargo --version)"
fi

BACKEND_DIR="./backend"
OUTPUT_DIR="./build"

if [ ! -d "$BACKEND_DIR" ]; then
    echo "❌ Error: '$BACKEND_DIR' directory not found."
    echo "Please ensure you are running this script from the root folder of the project."
    exit 1
fi

export RUST_FONTCONFIG_DLOPEN="on"
export FONTCONFIG_NO_PKG_CONFIG="1"
export FREETYPE_NO_PKG_CONFIG="1"
export RUST_FREETYPE_DLOPEN="on"
export FREETYPE_DLOPEN="1"

echo "🛠️  Compiling the 'shared' package..."
cd "$BACKEND_DIR"

cargo build --package shared --release

cd .. # Return to the root project folder

SCHEMA_SOURCE="./backend/target/$TARGET/release/settings.schema.json"

if [ -f "$SCHEMA_SOURCE" ]; then
    mkdir -p "$OUTPUT_DIR"
    cp "$SCHEMA_SOURCE" "$OUTPUT_DIR/settings.schema.json"
    
    echo "=================================================="
    echo "🎉 SUCCESS!"
    echo "📂 Schema file generated at: $OUTPUT_DIR/settings.schema.json"
    echo "=================================================="
else
    echo "❌ Error: Build completed but '$SCHEMA_SOURCE' was not found."
    exit 1
fi
