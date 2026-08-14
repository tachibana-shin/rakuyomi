#!/usr/bin/env bash

set -e

TARGET=$1
BUILD_NAME=$2
TYPE_BUILD=$3

OUT="build/${BUILD_NAME}"
mkdir -p "$OUT"

cd frontend/rakuyomi.koplugin/l10n
make mo
rm -rf */*.po .gitignore *.sh Makefile *.md *.po

cd ../../..

cp -r frontend/rakuyomi.koplugin/* "$OUT/"

if [ "$TARGET" != "none" ]; then
    cp "backend/target/$TARGET/release/cbz_metadata_reader" "$OUT/"
    cp "backend/target/$TARGET/release/server" "$OUT/"
    cp "backend/target/$TARGET/release/uds_http_request" "$OUT/"
    # Only built when the `lnreader` Cargo feature is enabled (on by
    # default, but fully removable) -- a build with it disabled must still
    # succeed under `set -e` above.
    if [ -f "backend/target/$TARGET/release/lnreader_worker" ]; then
        cp "backend/target/$TARGET/release/lnreader_worker" "$OUT/"
    fi
fi

VERSION="${SEMANTIC_RELEASE_VERSION:-1.0.0}"
echo "{ \"version\": \"$VERSION\", \"build\": \"$TYPE_BUILD\" }" \
    > "$OUT/BUILD_INFO.json"

echo "DONE → $OUT (version=$VERSION)"
