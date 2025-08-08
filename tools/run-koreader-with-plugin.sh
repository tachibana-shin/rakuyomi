#!/usr/bin/env bash

export RAKUYOMI_SERVER_COMMAND_OVERRIDE="$(which cargo) run --manifest-path backend/Cargo.toml -p server --"
if [[ "$1" == "--debug" ]]; then
    export RAKUYOMI_SERVER_COMMAND_OVERRIDE="$(which cargo) debugger --manifest-path backend/Cargo.toml -p server --"
fi

export RAKUYOMI_SERVER_WORKING_DIRECTORY="$(pwd)"
[ -z "${RAKUYOMI_SERVER_STARTUP_TIMEOUT+x}" ] && export RAKUYOMI_SERVER_STARTUP_TIMEOUT="600"

export RAKUYOMI_UDS_HTTP_REQUEST_COMMAND_OVERRIDE="$(which cargo) run --manifest-path backend/Cargo.toml -p uds_http_request --"
export RAKUYOMI_UDS_HTTP_REQUEST_WORKING_DIRECTORY="$(pwd)"

export RAKUYOMI_CBZ_METADATA_READER_COMMAND_OVERRIDE="$(which cargo) run --manifest-path backend/Cargo.toml -p cbz_metadata_reader --"
export RAKUYOMI_CBZ_METADATA_READER_WORKING_DIRECTORY="$(pwd)"

exec nix run .#rakuyomi.koreader-with-plugin -- "$HOME"
