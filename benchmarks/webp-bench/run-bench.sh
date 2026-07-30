#!/usr/bin/env bash
set -euo pipefail

echo "=== Generate test WebP images ==="
cargo run --release --manifest-path "$(dirname "$0")/../gen_data/Cargo.toml"
echo ""

echo "=== Benchmark: zenwebp + image-webp + webp-rust + webpx ==="
cargo bench --manifest-path "$(dirname "$0")/Cargo.toml"
echo ""

echo "=== Benchmark: webp-rs (separate, no linker conflict) ==="
cargo bench --manifest-path "$(dirname "$0")/benches_webp_rs/Cargo.toml"
echo ""

echo "=== Benchmark: webp crate v0.3 (separate, no linker conflict) ==="
cargo bench --manifest-path "$(dirname "$0")/benches_webp/Cargo.toml"
echo ""

echo "=== All WebP benchmarks done ==="
