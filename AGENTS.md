# RakuYomi — AI Agent Guide

## Project Overview

Manga reader plugin for KOReader. Rust HTTP server + Lua plugin frontend.
Rust backend handles sources (WASM/JS), downloads, DB (SQLite); Lua plugin provides UI within KOReader.

Architecture: `Backend.lua` (Lua) → HTTP/JSON → `server` (axum, Rust) → SQLite + WASM sources.

## Repository Structure

- `backend/` — Rust workspace
  - `server/` — HTTP server (binary + cdylib for Android JNI)
  - `shared/` — core domain: manga models, DB (sqlx/SQLite), source manager (wasmi for
    Aidoku WASM sources, `boa_engine` for LNReader/JS sources — see
    `shared/src/source/sdk_lnreader/`), downloader, settings
  - `lnreader_worker/` — persistent per-source subprocess running LNReader/JS plugin
    sources (`boa_engine`), sibling to `uds_http_request`/`cbz_metadata_reader`; see
    `docs/lnreader/README.md` for the investigation/decision history behind it
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

## Git Safety Rules

Adopted after a real incident (destructive `git checkout HEAD --` during a
commit-split attempt discarded uncommitted work in 6 files, plus a 7th
untracked file deleted via `rm -f` with no backup at all — see
`docs/lnreader/INCIDENT_GIT_CHECKOUT_DATA_LOSS.md` for the full postmortem).

- **Never run a destructive git operation** (`checkout HEAD --`,
  `reset --hard`, `clean -f`, a history-rewriting `rebase`) **on a file
  with uncommitted work without a backup verified for that specific file
  immediately beforehand** — an explicit copy, or a `git stash push`
  confirmed non-empty. Verify per file, right before that file's
  destructive command — not once, in bulk, at the start of a multi-file
  operation, on the assumption it covers everything that follows.
- **Prefer non-destructive alternatives** when reconstructing an
  intermediate state for a commit split: `git add -p` (interactive
  partial staging), or a separate `git worktree` on a temporary branch.
  Never rewrite the main working tree with a command that discards
  uncommitted content just to get to an intermediate state.
- **If a destructive operation goes wrong anyway:** stop immediately, do
  not attempt a silent fix, report exactly and completely what happened,
  and wait for explicit confirmation before attempting any recovery or
  reconstruction. This is expected behavior, not a judgment call to make
  in the moment.
- **Don't let uncommitted work accumulate across sessions.** Commit
  regularly, even in small increments — it's what turns a git mistake
  into a non-event instead of a real loss (the incident above only
  mattered because the lost content had never been committed in any prior
  session).

## Update translation texts

```sh
cd frontend/rakuyomi.koplugin/l10n
make update-trans
```
