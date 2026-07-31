#!/usr/bin/env bash
# Rebuilds the LNReader JS runtime bundle from the TypeScript sources in
# lnreader_js/ into a single assets/libs.js (requires bun).
set -euo pipefail

cd "$(dirname "$0")/../backend/shared/src/source/lnreader/lnreader_js"
bun install
bun run build
echo "libs.js rebuilt: $(wc -c < ../assets/libs.js) bytes"
