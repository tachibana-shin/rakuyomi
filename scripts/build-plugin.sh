#!/usr/bin/env bash

set -e

TARGET=$1
BUILD_NAME=$2
TYPE_BUILD=$3

OUT="build/${BUILD_NAME}"
mkdir -p "$OUT"

cp -r frontend/rakuyomi.koplugin/* "$OUT/"

cp backend/target/$TARGET/release/cbz_metadata_reader "$OUT/"
cp backend/target/$TARGET/release/server "$OUT/"
cp backend/target/$TARGET/release/uds_http_request "$OUT/"

VERSION="${SEMANTIC_RELEASE_VERSION:-1.0.0}"
echo "{ \"version\": \"$VERSION\", \"build\": \"$TYPE_BUILD\" }" \
    > "$OUT/BUILD_INFO.json"

echo "DONE → $OUT (version=$VERSION)"
