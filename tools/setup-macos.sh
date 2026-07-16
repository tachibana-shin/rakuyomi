#!/usr/bin/env bash
# Sets up a local macOS development environment without requiring Nix/devenv.
# Safe to re-run — existing extracted KOReader is reused, plugin is always refreshed.

set -e

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="$REPO_ROOT/build/macos"
KOREADER_APP="$BUILD_DIR/KOReader.app"
PLUGIN_DEST="$KOREADER_APP/Contents/koreader/plugins/rakuyomi.koplugin"
KOREADER_ZIP="$REPO_ROOT/packages/koreader-macos-arm64.zip"

ensure_cargo_in_shell() {
    local shell_rc="$HOME/.zshrc"
    local cargo_env_line='. "$HOME/.cargo/env"'
    if ! grep -qF "$cargo_env_line" "$shell_rc" 2>/dev/null; then
        echo "" >> "$shell_rc"
        echo "# Added by rakuyomi setup-macos.sh" >> "$shell_rc"
        echo "$cargo_env_line" >> "$shell_rc"
        echo "Added Cargo to PATH in $shell_rc"
    fi
}

install_rust() {
    echo "Rust not found. Installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    source "$HOME/.cargo/env"
    ensure_cargo_in_shell
    echo "Rust installed."
}

install_toolchain() {
    local version
    version="$(grep '^channel' "$REPO_ROOT/rust-toolchain.toml" | sed 's/.*= *"//' | sed 's/".*//')"
    echo "Installing Rust toolchain $version (required by rust-toolchain.toml)..."
    rustup toolchain install "$version"
}

install_p7zip() {
    if ! command -v brew &>/dev/null; then
        echo "Homebrew is required to install p7zip but was not found."
        echo "Install Homebrew from https://brew.sh, then re-run this script."
        exit 1
    fi
    echo "p7zip not found. Installing via Homebrew..."
    brew install p7zip
}

check_deps() {
    if ! command -v cargo &>/dev/null; then
        # cargo may exist but not be on PATH yet (e.g. fresh rustup install)
        if [[ -f "$HOME/.cargo/env" ]]; then
            source "$HOME/.cargo/env"
        fi
        if ! command -v cargo &>/dev/null; then
            install_rust
        fi
    fi

    install_toolchain

    if ! command -v 7za &>/dev/null && ! command -v 7zz &>/dev/null; then
        install_p7zip
    fi
}

extract_koreader() {
    if [[ -d "$KOREADER_APP" ]]; then
        echo "KOReader already extracted, skipping."
        return
    fi

    echo "Extracting KOReader..."
    mkdir -p "$BUILD_DIR"

    local tmp
    tmp="$(mktemp -d)"
    trap "rm -rf '$tmp'" EXIT

    unzip -q "$KOREADER_ZIP" -d "$tmp"

    local sevenz_bin
    sevenz_bin="$(command -v 7zz 2>/dev/null || command -v 7za)"

    local archive
    archive="$(find "$tmp" -name "*.7z" | head -1)"
    "$sevenz_bin" x "$archive" -o"$BUILD_DIR" -y >/dev/null

    echo "KOReader extracted to $KOREADER_APP"
}

copy_plugin_to() {
    local dest="$1"
    rm -rf "$dest"
    mkdir -p "$dest"
    rsync -a --exclude='*_spec.lua' "$REPO_ROOT/frontend/rakuyomi.koplugin/" "$dest/"
}

install_plugin() {
    echo "Installing plugin..."

    local user_plugin_dest="$HOME/Library/Application Support/koreader/plugins/rakuyomi.koplugin"

    if [[ -d "$HOME/Library/Application Support/koreader" ]]; then
        # KOReader already has a user data dir — install only there.
        # KOReader scans both the app bundle and the user dir; installing to both
        # causes the plugin to be loaded twice (backend was already initialized).
        copy_plugin_to "$user_plugin_dest"
        # Remove from app bundle to prevent double-loading.
        rm -rf "$PLUGIN_DEST"
        echo "Plugin installed to $user_plugin_dest"
    else
        # Fresh KOReader — no user data dir yet. Install to the app bundle so
        # KOReader finds it on first launch and populates its user dir.
        copy_plugin_to "$PLUGIN_DEST"
        echo "Plugin installed to $PLUGIN_DEST"
    fi
}

echo "=== rakuyomi macOS dev setup ==="
check_deps
extract_koreader
install_plugin
echo ""
echo "Done! Run './tools/dev-macos.sh' to launch KOReader with the plugin."
