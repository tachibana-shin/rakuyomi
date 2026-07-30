#!/usr/bin/env bash
set -euo pipefail

echo "=== Benchmark: png (zlib-rs) + spng + raw inflate backends ==="
cargo bench --manifest-path "$(dirname "$0")/Cargo.toml"
