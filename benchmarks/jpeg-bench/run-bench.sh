#!/usr/bin/env bash
set -euo pipefail

echo "=== Generate test JPEG images ==="
cargo run --release --manifest-path "$(dirname "$0")/../gen_data/Cargo.toml"
echo ""

echo "=== Benchmark: zune-jpeg + turbojpeg + libjpeg-turbo-rs ==="
cargo bench --manifest-path "$(dirname "$0")/Cargo.toml"
echo ""

echo "=== Benchmark: mozjpeg (separate, no linker conflict) ==="
cargo bench --manifest-path "$(dirname "$0")/benches_moz/Cargo.toml"
echo ""

echo "=== All benchmarks done ==="
