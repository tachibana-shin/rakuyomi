#!/usr/bin/env bash
# Launches KOReader with the rakuyomi plugin.
#
# - The server runs via `cargo run` (recompiles on restart when source changes).
# - uds_http_request and cbz_metadata_reader are pre-built and copied into the
#   plugin directory on each launch. Rebuild them manually with:
#   cargo build -p uds_http_request -p cbz_metadata_reader
#   if you change their source.
#
# Run setup-macos.sh first if you haven't already.

set -e

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KOREADER_BIN="$REPO_ROOT/build/macos/KOReader.app/Contents/MacOS/koreader"
# setup-macos.sh installs the plugin to the user data dir when it exists (to avoid
# double-loading), so resolve the effective plugin dir the same way.
if [[ -d "$HOME/Library/Application Support/koreader" ]]; then
    PLUGIN_DIR="$HOME/Library/Application Support/koreader/plugins/rakuyomi.koplugin"
else
    PLUGIN_DIR="$REPO_ROOT/build/macos/KOReader.app/Contents/koreader/plugins/rakuyomi.koplugin"
fi

if [[ ! -x "$KOREADER_BIN" ]]; then
    echo "KOReader not found. Run './tools/setup-macos.sh' first."
    exit 1
fi

# Source cargo env in case this is a fresh shell and PATH isn't updated yet.
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"

# Reinstall Lua plugin on every launch so frontend changes are picked up immediately.
rsync -a --exclude='*_spec.lua' "$REPO_ROOT/frontend/rakuyomi.koplugin/" "$PLUGIN_DIR/"

# Build request-path binaries and copy them into the plugin directory.
# KOReader looks for these at plugin_dir/uds_http_request and plugin_dir/cbz_metadata_reader.
# We avoid env var overrides here since macOS app bundles don't reliably inherit
# shell environment variables.
echo "Building uds_http_request and cbz_metadata_reader..."
cargo build --manifest-path "$REPO_ROOT/backend/Cargo.toml" \
    -p uds_http_request -p cbz_metadata_reader -q
cp "$REPO_ROOT/backend/target/debug/uds_http_request" "$PLUGIN_DIR/uds_http_request"
cp "$REPO_ROOT/backend/target/debug/cbz_metadata_reader" "$PLUGIN_DIR/cbz_metadata_reader"

# The server uses `cargo run` via execl (not io.popen), so env var inheritance
# works correctly for it. This gives hot recompilation on server source changes.
export RAKUYOMI_SERVER_COMMAND_OVERRIDE="$HOME/.cargo/bin/cargo run --manifest-path $REPO_ROOT/backend/Cargo.toml -p server --"
export RAKUYOMI_SERVER_WORKING_DIRECTORY="$REPO_ROOT"
export RAKUYOMI_SERVER_STARTUP_TIMEOUT="${RAKUYOMI_SERVER_STARTUP_TIMEOUT:-600}"

# uds_http_request and cbz_metadata_reader are called via io.popen in Lua.
# The plugin directory on macOS is under ~/Library/Application Support/... which
# has spaces and breaks shell invocation. Point directly to the pre-built binaries
# in the repo (no spaces in that path).
export RAKUYOMI_UDS_HTTP_REQUEST_COMMAND_OVERRIDE="$REPO_ROOT/backend/target/debug/uds_http_request"
export RAKUYOMI_CBZ_METADATA_READER_COMMAND_OVERRIDE="$REPO_ROOT/backend/target/debug/cbz_metadata_reader"

exec "$KOREADER_BIN" "$HOME"
