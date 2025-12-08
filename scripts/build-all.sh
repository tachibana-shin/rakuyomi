#!/usr/bin/env bash
set -e

# Use Podman as container backend
export CROSS_CONTAINER_ENGINE=podman

# --- Mapping build names to actual Rust targets ---
declare -A TARGETS=(
  ["desktop"]="x86_64-unknown-linux-musl"
  ["aarch64"]="aarch64-unknown-linux-musl"
  ["kindle"]="arm-unknown-linux-musleabi"
  ["kindlehf"]="arm-unknown-linux-musleabihf"
)

# --- Helper function: build for one profile ---
build_one() {
  local name="$1"
  local target="${TARGETS[$name]}"

  cd backend
  echo "=== Building $name ($target) ==="
  mkdir -p .cargo
  cat > .cargo/config.toml << 'EOF'
[env]
RUST_FONTCONFIG_DLOPEN = "on"
FONTCONFIG_NO_PKG_CONFIG = "1"
EOF

  # Build all required crates
  cross build --release --target "$target"
  cd ..

  # Package osh output
  bash ./scripts/build-plugin.sh "$target" "rakuyomi.koplugin"

  echo "=== DONE: $name ==="
}

# --- Parse input arguments ---
if [[ $# -eq 1 ]]; then
  # Single argument → must be a valid build key
  key="$1"

  if [[ -n "${TARGETS[$key]}" ]]; then
    # Valid key → build only this one
    build_one "$key"
  else
    echo "❌ Unknown build target: '$key'"
    echo "Available targets:"
    for k in "${!TARGETS[@]}"; do
      echo "  - $k"
    done
    exit 1
  fi

else
  # No or multiple arguments → build all
  for name in "${!TARGETS[@]}"; do
    build_one "$name"
  done
fi

