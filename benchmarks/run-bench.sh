#!/usr/bin/env bash
set -euo pipefail
BENCH=$(dirname "$0")

echo "=== Generate test data ==="
cargo run --release --manifest-path "$BENCH/gen_data/Cargo.toml"

echo ""
"$BENCH/jpeg-bench/run-bench.sh"

echo ""
"$BENCH/png-bench/run-bench.sh"

echo ""
"$BENCH/png-zlib-bench/run-bench.sh"

echo ""
"$BENCH/webp-bench/run-bench.sh"

echo ""
echo "=== Benchmark: BMP ==="
cargo bench --manifest-path "$BENCH/bmp-bench/Cargo.toml"

echo ""
echo "=== Benchmark: AVIF ==="
cargo bench --manifest-path "$BENCH/avif-bench/Cargo.toml"

echo ""
echo "=== Benchmark: JPEG XL ==="
cargo bench --manifest-path "$BENCH/jxl-bench/Cargo.toml"

echo ""
echo "=== All benchmarks done ==="
