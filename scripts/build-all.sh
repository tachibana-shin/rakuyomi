#!/usr/bin/env bash
set -e

# Use Podman as container backend
export CROSS_CONTAINER_ENGINE=podman

# --- Mapping build names to actual Rust targets ---
declare -A TARGETS=(
  ["desktop"]="x86_64-unknown-linux-musl"
  ["aarch64"]="aarch64-unknown-linux-musl"
  ["macos"]="aarch64-apple-darwin"
  ["kindle"]="arm-unknown-linux-musleabi"
  ["kindlehf"]="arm-unknown-linux-musleabihf"
  ["kindlea9"]="arm-unknown-linux-musleabi"
)

# --- Helper function: build for one profile ---
build_one() {
  local name="$1"
  local target="${TARGETS[$name]}"

  cd backend
  echo "=== Building $name ($target) ==="

  local base_flags=""

  if [[ "$name" == "kindlea9" ]]; then
    echo "🚀 Applying aggressive optimizations for Cortex-A9..."
    base_flags="-C target-cpu=cortex-a9 -C target-feature=+thumb2,+neon"
  fi

  mkdir -p .cargo
  cat > .cargo/config.toml << 'EOF'
[env]
RUST_FONTCONFIG_DLOPEN = "on"
FONTCONFIG_NO_PKG_CONFIG = "1"

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
EOF

  # Build all required crates
  if [[ "$name" == "macos" ]]; then
    RUSTFLAGS="$base_flags" cargo build --release --target "$target"
  else
    RUSTFLAGS="$base_flags" cross build --release --target "$target"
  fi
  cd ..

  # Package osh output
  bash ./scripts/build-plugin.sh "$target" "rakuyomi.koplugin" "$name"

  echo "=== DONE: $name ==="
}

# --- Parse input arguments ---
if [[ $# -eq 1 ]]; then
  # Single argument → must be a valid build key
  key="$1"

  if [[ "$key" == "android" ]]; then
    bash ./scripts/build-plugin.sh "none" "rakuyomi.koplugin" "android"
  elif [[ -n "${TARGETS[$key]}" ]]; then
    build_one "$key"
  else
    echo "❌ Unknown build target: '$key'"
    echo "Available targets:"
    for k in "${!TARGETS[@]}"; do
      echo "  - $k"
    done
    echo "  - android"
    exit 1
  fi

else
  # No or multiple arguments → build all
  for name in "${!TARGETS[@]}"; do
    build_one "$name"
  done
fi
