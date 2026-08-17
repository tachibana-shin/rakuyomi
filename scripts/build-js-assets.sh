#!/usr/bin/env bash
# Rebuilds the embedded JS runtime bundles from TypeScript sources (requires
# bun):
#   - LNReader:    lnreader_js/ -> lnreader/assets/libs.js
#   - MangaYomi:   mangayomi/polyfill/ -> mangayomi/js_assets/polyfill.js
set -euo pipefail

# Resolve the script's own directory once: every `cd` below must be
# relative to it, because the working directory changes as we build.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

cd "$SCRIPT_DIR/../backend/shared/src/source/lnreader/lnreader_js"
bun install
bun run build
echo "libs.js rebuilt: $(wc -c < ../assets/libs.js) bytes"

cd "$SCRIPT_DIR/../backend/shared/src/source/mangayomi/polyfill"
bun install
bun run build
echo "polyfill.js rebuilt: $(wc -c < ../js_assets/polyfill.js) bytes"