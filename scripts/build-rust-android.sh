#!/usr/bin/env bash
#
# Build the Android Rust shared libraries only
#
# Usage:
#
#   # Release build (All targets)
#   ./scripts/build-rust-android.sh
#
#   # Fast development build (arm64 only)
#   ./scripts/build-rust-android.sh dev
#

set -e

MODE="release"

for arg in "$@"; do
  case "$arg" in
    dev)
      MODE="dev"
      ;;
    *)
      echo "Unknown argument: $arg"
      echo ""
      echo "Usage:"
      echo "  $0"
      echo "  $0 dev"
      exit 1
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BRIDGE_DIR="$PROJECT_DIR/../bridge"
BACKEND_DIR="$PROJECT_DIR/backend"

JNILIBS_BASE="$BRIDGE_DIR/androidApp/src/main/jniLibs"
HEADLESS_JNILIBS_BASE="$BRIDGE_DIR/androidHeadless/src/main/jniLibs"

mkdir -p "$JNILIBS_BASE/arm64-v8a" "$JNILIBS_BASE/armeabi-v7a" "$JNILIBS_BASE/x86_64"
mkdir -p "$HEADLESS_JNILIBS_BASE/arm64-v8a" "$HEADLESS_JNILIBS_BASE/armeabi-v7a" "$HEADLESS_JNILIBS_BASE/x86_64"

echo "========================================"
echo "Build mode      : $MODE"
echo "Target Action   : Rust Libraries Only"
echo "========================================"

echo ""
echo "=== Step 1: Build Rust shared library ==="

cd "$BACKEND_DIR"

mkdir -p .cargo

cat > .cargo/config.toml << 'EOF'
[env]
RUST_FONTCONFIG_DLOPEN = "on"
FONTCONFIG_NO_PKG_CONFIG = "1"
EOF

TARGETS=("aarch64-linux-android")

if [[ "$MODE" != "dev" ]]; then
  TARGETS=(
    "aarch64-linux-android"
    "armv7-linux-androideabi"
    "x86_64-linux-android"
  )
fi

for target in "${TARGETS[@]}"; do
  echo ""
  echo "+ Building server for $target"

  case "$target" in
    aarch64-linux-android|x86_64-linux-android)
      PLATFORM=21
      ;;
    armv7-linux-androideabi)
      PLATFORM=18
      ;;
    *)
      PLATFORM=21
      ;;
  esac

  echo "  Android API level: $PLATFORM"
  if [[ "$PLATFORM" -lt 21 ]]; then
    FEATURES="ffi,api_18"
  else
    FEATURES="ffi"
  fi

  cargo ndk \
      --target "$target" \
      --platform "$PLATFORM" \
      build \
      --release \
      --package server \
      --features "$FEATURES"

  LIB_PATH="$BACKEND_DIR/target/$target/release/libserver.so"
  if [[ ! -f "$LIB_PATH" ]]; then
    echo "❌ Missing library:"
    echo "   $LIB_PATH"
    exit 1
  fi

  case "$target" in
    aarch64-linux-android)
      cp "$LIB_PATH" "$JNILIBS_BASE/arm64-v8a/librakuyomi_server.so"
      cp "$LIB_PATH" "$HEADLESS_JNILIBS_BASE/arm64-v8a/librakuyomi_server.so"
      ;;
    armv7-linux-androideabi)
      cp "$LIB_PATH" "$JNILIBS_BASE/armeabi-v7a/librakuyomi_server.so"
      cp "$LIB_PATH" "$HEADLESS_JNILIBS_BASE/armeabi-v7a/librakuyomi_server.so"
      ;;
    x86_64-linux-android)
      cp "$LIB_PATH" "$JNILIBS_BASE/x86_64/librakuyomi_server.so"
      cp "$LIB_PATH" "$HEADLESS_JNILIBS_BASE/x86_64/librakuyomi_server.so"
      ;;
  esac
done

echo ""
echo "✅ Rust libraries build and copy completed successfully!"
