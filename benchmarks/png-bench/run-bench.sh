#!/usr/bin/env bash
set -euo pipefail

echo "=== Benchmark: zune-png + png + lodepng + png_pong + zenpng ==="
cargo bench --manifest-path "$(dirname "$0")/Cargo.toml"
