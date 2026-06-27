# RakuYomi — AI Agent Guide

## Project Overview

Manga reader plugin for KOReader. Rust HTTP server + Lua plugin frontend.
Rust backend handles sources (WASM/JS), downloads, DB (SQLite); Lua plugin provides UI within KOReader.

Architecture: `Backend.lua` (Lua) → HTTP/JSON → `server` (axum, Rust) → SQLite + WASM sources.

## Repository Structure

- `backend/` — Rust workspace
  - `server/` — HTTP server (binary + cdylib for Android JNI)
  - `shared/` — core domain: manga models, DB (sqlx/SQLite), source manager (wasmi), downloader, settings
  - `uds_http_request/` — standalone UDS HTTP proxy binary
  - `cbz_metadata_reader/` — CBZ metadata extraction binary
  - `wasm_macros/` — proc-macro crate for WASM bindings
  - `wasm_shared/` — shared WASM interop types
- `frontend/rakuyomi.koplugin/` — Lua plugin (KOReader)
  - `Backend.lua` — central API, server communication
  - `Platform.lua` — platform dispatch (android vs generic_unix)
  - `platform/` — platform implementations (TCP vs UDS + fork/exec)
  - `main.lua` — plugin entry, registers menu & Dispatcher
  - `LibraryView.lua`, `ChapterListing.lua`, `MangaSearchResults.lua` etc. — UI views
  - `jobs/` — async download jobs
  - `l10n/` — translations (40+ languages)
- `docs/` — mdBook documentation
- `scripts/` — build scripts

## Rust Conventions

- Edition 2021, toolchain 1.95.0
- snake_case functions/vars, CamelCase types
- `anyhow::Result` in binaries, `thiserror` for library error enums
- axum with `FromRef` state pattern
- tokio multi-threaded async throughout
- JNI code in `server/src/jni.rs` behind `#[cfg(target_os = "android")]`
- Release profile: `opt-level=3`, `lto="fat"`, `codegen-units=1`, `panic="abort"`
- Cross-compile targets: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `arm-unknown-linux-musleabi[hf]`, `aarch64-linux-android` etc.

## Lua Conventions

- LuaJIT 5.1 compatibility (KOReader uses LuaJIT)
- Require-based modules returning tables
- CamelCase for module names/classes, snake_case for locals/functions
- EmmyLua annotations on all public APIs (`--- @class`, `--- @param`, `--- @return`)
- KOReader widget pattern: `local Foo = InputContainer:extend { ... }`
- UI via `UIManager:show()`, frame containers, etc.

## Build

```sh
scripts/build-all.sh <target>   # cross-compile + package plugin
scripts/build-android.sh        # build libserver.so + APK
```

CI (root): `.github/workflows/build.yml` — 5 targets via `cross` + Podman.
builds Rust `.so` via `scripts/build-rust-android.sh`, then runs Gradle
lint/test/assemble for the Android companion app.
Versioning: `semantic-release` from commit messages.

## Platform Architecture

- **Unix** (Kindle, Kobo, etc.): fork/exec `server` binary, UDS (`/tmp/rakuyomi.sock`), `uds_http_request` binary bridges HTTP→UDS
- **Android**: `libserver.so` loaded via JNI in companion app TCP `127.0.0.1:8787`
- **Linux (bridge mode)**: systemd user service runs `server` with TCP on `127.0.0.1:8787`, Lua plugin connects via LuaSocket when `RAKUYOMI_USE_BRIDGE=1`

Data directory: `$KOREARCHIVE_DIR/rakuyomi/` (Unix) or `/storage/emulated/0/koreader/rakuyomi` (Android)

## Key Rules

- No emojis in code or comments
- KDoc/Javadoc for all Rust public APIs, EmmyLua for Lua
- Keep Rust backend + Lua frontend loosely coupled via JSON API

## Update translation texts

```sh
cd frontend/rakuyomi.koplugin/l10n
make update-trans
```
