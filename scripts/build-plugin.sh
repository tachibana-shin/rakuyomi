#!/usr/bin/env bash

set -e

TARGET=$1
BUILD_NAME=$2

OUT="build/${BUILD_NAME}"
mkdir -p "$OUT"

cp -r frontend/rakuyomi.koplugin/* "$OUT/"

cp backend/target/$TARGET/release/cbz_metadata_reader "$OUT/"
cp backend/target/$TARGET/release/server "$OUT/"
cp backend/target/$TARGET/release/uds_http_request "$OUT/"

echo "{ \"version\": \"dev\", \"build\": \"$BUILD_NAME\" }" > "$OUT/BUILD_INFO.json"

echo "DONE → $OUT"

