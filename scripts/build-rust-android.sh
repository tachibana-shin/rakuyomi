#!/usr/bin/env bash
#
# Build the Android Rust shared libraries only
#
# Usage:
#
#   # Release build (All targets, sequential)
#   ./scripts/build-rust-android.sh
#
#   # Build selected targets only (any combination, sequential)
#   ./scripts/build-rust-android.sh aarch64 armv7
#
#   # Fast development build (arm64 only)
#   ./scripts/build-rust-android.sh dev
#
#   # Release build of all targets in parallel
#   ./scripts/build-rust-android.sh parallel
#
# With no arguments the script builds every target one after another into
# the shared `backend/target` directory. Downstream repos (e.g. the Android
# companion app) call the script without arguments, so that mode is
# guaranteed to stay unchanged. `parallel` builds all targets concurrently,
# each in its own `backend/target-parallel/<triple>` directory, because
# cargo serializes builds that share a target directory (`.cargo-lock`).

set -e

MODE="release"
PARALLEL=""
TARGET_ARGS=()

for arg in "$@"; do
  case "$arg" in
    dev)
      MODE="dev"
      ;;
    parallel)
      PARALLEL="1"
      ;;
    aarch64|armv7|x86_64)
      TARGET_ARGS+=("$arg")
      ;;
    *)
      echo "Unknown argument: $arg"
      echo ""
      echo "Usage:"
      echo "  $0"
      echo "  $0 dev"
      echo "  $0 parallel"
      echo "  $0 [aarch64] [armv7] [x86_64]"
      exit 1
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BRIDGE_DIR="$PROJECT_DIR/.."
BACKEND_DIR="$PROJECT_DIR/backend"

JNILIBS_BASE="$BRIDGE_DIR/androidApp/src/main/jniLibs"

mkdir -p "$JNILIBS_BASE/arm64-v8a" "$JNILIBS_BASE/armeabi-v7a" "$JNILIBS_BASE/x86_64"

echo "========================================"
echo "Build mode      : $MODE"
echo "Target Action   : Rust Libraries Only"
if [[ -n "$PARALLEL" ]]; then
  echo "Build strategy  : parallel (per-target target dirs)"
fi
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

# rquickjs-sys has no pre-generated bindings for Android targets, so it
# generates them with bindgen at build time. The clang driver and libclang
# come from the system packages installed in CI; the NDK sysroot headers
# are passed to bindgen per target.
if [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
  NDK_LLVM="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64"
  NDK_SYSROOT="$NDK_LLVM/sysroot/usr/include"
fi

if [[ "$MODE" == "dev" ]]; then
  TARGETS=("aarch64-linux-android")
elif [[ ${#TARGET_ARGS[@]} -gt 0 ]]; then
  TARGETS=()
  for target_arg in "${TARGET_ARGS[@]}"; do
    case "$target_arg" in
      aarch64) TARGETS+=("aarch64-linux-android") ;;
      armv7) TARGETS+=("armv7-linux-androideabi") ;;
      x86_64) TARGETS+=("x86_64-linux-android") ;;
    esac
  done
else
  TARGETS=(
    "aarch64-linux-android"
    "armv7-linux-androideabi"
    "x86_64-linux-android"
  )
fi

echo "Targets         : ${TARGETS[*]}"

# Builds one target. The first argument is the target triple, the second is
# the optional CARGO_TARGET_DIR override used by parallel mode.
build_target() {
  local target="$1"
  local target_dir="$2"
  local build_dir="${target_dir:-$BACKEND_DIR/target}"

  echo ""
  echo "+ Building server for $target"

  case "$target" in
    aarch64-linux-android)
      PLATFORM=21
      ANDROID_ARCH_DIR="aarch64-linux-android"
      ;;
    armv7-linux-androideabi)
      PLATFORM=18
      ANDROID_ARCH_DIR="arm-linux-androideabi"
      ;;
    x86_64-linux-android)
      PLATFORM=21
      ANDROID_ARCH_DIR="x86_64-linux-android"
      ;;
    *)
      PLATFORM=21
      ANDROID_ARCH_DIR="aarch64-linux-android"
      ;;
  esac

  if [[ -n "${NDK_SYSROOT:-}" ]]; then
    export BINDGEN_EXTRA_CLANG_ARGS="-isystem $NDK_SYSROOT/$ANDROID_ARCH_DIR -isystem $NDK_SYSROOT"
  fi

  echo "  Android API level: $PLATFORM"
  if [[ "$PLATFORM" -lt 21 ]]; then
    FEATURES="ffi,api_18"
    DEFAULT_FLAG="--no-default-features"
  else
    FEATURES="ffi"
    DEFAULT_FLAG=""
  fi

  if [[ -n "$target_dir" ]]; then
    export CARGO_TARGET_DIR="$target_dir"
  else
    unset CARGO_TARGET_DIR
  fi

  cargo ndk \
      --target "$target" \
      --platform "$PLATFORM" \
      build \
      --release \
      --package server \
      $DEFAULT_FLAG \
      --features "$FEATURES"

  LIB_PATH="$build_dir/$target/release/libserver.so"
  if [[ ! -f "$LIB_PATH" ]]; then
    echo "❌ Missing library:"
    echo "   $LIB_PATH"
    exit 1
  fi

  case "$target" in
    aarch64-linux-android)
      cp "$LIB_PATH" "$JNILIBS_BASE/arm64-v8a/librakuyomi_server.so"
      ;;
    armv7-linux-androideabi)
      cp "$LIB_PATH" "$JNILIBS_BASE/armeabi-v7a/librakuyomi_server.so"
      ;;
    x86_64-linux-android)
      cp "$LIB_PATH" "$JNILIBS_BASE/x86_64/librakuyomi_server.so"
      ;;
  esac
}

if [[ -n "$PARALLEL" ]]; then
  PARALLEL_TARGET_DIR="$BACKEND_DIR/target-parallel"
  mkdir -p "$PARALLEL_TARGET_DIR"

  pids=()
  for target in "${TARGETS[@]}"; do
    echo ""
    echo "+ Building server for $target (log: $PARALLEL_TARGET_DIR/$target.log)"
    build_target "$target" "$PARALLEL_TARGET_DIR/$target" \
      > "$PARALLEL_TARGET_DIR/$target.log" 2>&1 &
    pids+=("$!")
  done

  failed=0
  for i in "${!pids[@]}"; do
    target="${TARGETS[$i]}"
    if ! wait "${pids[$i]}"; then
      echo ""
      echo "❌ Build failed for $target:"
      echo ""
      cat "$PARALLEL_TARGET_DIR/$target.log"
      failed=1
    fi
  done

  if [[ "$failed" -ne 0 ]]; then
    exit 1
  fi
else
  for target in "${TARGETS[@]}"; do
    build_target "$target"
  done
fi

echo ""
echo "✅ Rust libraries build and copy completed successfully!"