#!/usr/bin/env bash
set -e

# Use Podman as container backend
export CROSS_CONTAINER_ENGINE=podman

# Generate the LNReader JS runtime bundle on the host (cross containersandroid-armv7 have
# no bun, and the generated file is mounted into them by cross).
bash ./scripts/build-js-assets.sh

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

  # The 32-bit ARM musl targets have no 64-bit atomics, so the quickjs
  # runtime needs libatomic and libgcc at link time. Force a fully static
  # binary: rustc emits `-Wl,-Bdynamic` after its crt objects, so plain
  # `-static` is not enough — libs passed via `-C link-arg` would be linked
  # dynamically (libatomic.so.1), and Kindles have no musl dynamic linker
  # or those .so files, so the server would fail to start.
  if [[ "$name" == "kindle" || "$name" == "kindlea9" || "$name" == "kindlehf" ]]; then
    base_flags="-C link-arg=-static -C link-arg=-Wl,-Bstatic -C link-arg=-latomic -C link-arg=-lgcc -C link-arg=-Wl,-Bdynamic"
  fi

  if [[ "$name" == "kindlea9" ]]; then
    echo "🚀 Applying aggressive optimizations for Cortex-A9..."
    base_flags="$base_flags -C target-cpu=cortex-a9 -C target-feature=+thumb2,+neon"
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

  # The e-readers have no dynamic linker or shared libs for the 32-bit ARM
  # musl binaries; fail the build instead of shipping a broken plugin.
  if [[ "$name" == "kindle" || "$name" == "kindlea9" || "$name" == "kindlehf" ]]; then
    if readelf -d build/rakuyomi.koplugin/server 2>/dev/null | grep -q NEEDED; then
      echo "❌ $name server binary has dynamic library dependencies; refusing to package"
      exit 1
    fi
  fi

  echo "=== DONE: $name ==="
}

# --- Parse input arguments ---
if [[ $# -eq 1 ]]; then
  # Single argument → must be a valid build key
  key="$1"

  if [[ "$key" == android* ]]; then
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
