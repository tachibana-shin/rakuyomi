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
    sources (`boa_engine`), sibling to `uds_http_request`/`cbz_metadata_reader`
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

Adopted after a real incident: a destructive `git checkout HEAD --` during a
commit-split attempt discarded uncommitted work in 6 files, plus a 7th
untracked file deleted via `rm -f` with no backup at all.

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

## LNReader Constraints

Binding project constraints for the LNReader/JS source execution mode
(`shared/src/source/sdk_lnreader/`).

- **All-native-Rust**: never bundle/interpret a real JS library (`dayjs`,
  `htmlparser2`, `lodash-es`, cheerio) inside `boa_engine` — polyfill only
  their JS-visible shape, do the real work in native Rust.
- **No hardcoded sources**: `source_lists` (`Vec<Url>`) is the only
  legitimate discovery mechanism; no vendored test fixture with real
  source URLs/names in committed code.
- **No new Lua widget/screen, ever.**
- **Feature bar = Rakuyomi's own existing manga functionality**, not the
  theoretical ceiling of either SDK.
- **Minimize changes to existing Rakuyomi/Aidoku code**; isolate new work
  in separate modules; never break the existing Aidoku/WASM mode.
- **LNReader discovery must look identical to Aidoku's** to the user: one
  URL in `source_lists`, no manual CLI step.
- **Target device is a low-end e-reader** (single/dual-core SoC) — a real
  hardware constraint on every design decision.
- **Native calls per JS-level call**: `N+1`/`1+N` acceptable by default;
  heavier shapes (`2N+1`, `2N+3`, ...) are not acceptable by default —
  look for something cheaper. Ambiguous case → produce a cost table and
  decide together, not a rigid rule applied without data.
- **Pre-implementing ahead of confirmed need is justified** when a native
  equivalent is already available, the cost is low, and the domain
  relevance (light-novel scraping) is plausible — even with no current
  corpus evidence for that exact method.
- **Follow Aidoku's own established patterns** rather than inventing new
  ones, absent a real, documented, empirical reason otherwise.
- **`lnreader` Cargo feature**: on by default, fully removable.
  **`lnreader_enabled` config toggle**: on-disk settings with the key
  missing (the real-world default for existing/fresh installs, via
  `#[serde(default = "default_true")]` in `settings/schema.rs`) load as
  `true` — a deliberate reversal of the original spec. `Settings::default()`
  itself (the `#[derive(Default)]` used when constructing one in Rust, e.g.
  in tests) is `false`, since serde's per-field `default` attribute doesn't
  affect the `Default` trait derive. Do not "fix" the on-disk default back
  to `false` without an explicit decision to end LNReader's active testing
  period.
- **All of `docs/lnreader/` stays out of git tracking** (local-only working
  notes).
- **Validation**: prefer the full real corpus over a sample when feasible;
  confirm a method's real argument shapes from actual call sites, not just
  occurrence counts; validate end-to-end against the real pipeline (real
  `lnreader_worker` subprocess, a real live install), not only in-process
  tests.

## Update translation texts

```sh
cd frontend/rakuyomi.koplugin/l10n
make update-trans
```

## superpowers skills overrides

### subagent-driven-development

Modified agent selection and escalation flow:

1. **BLOCKED** → Orchestrator collects fix_hint, escalates via `subagent_type: expert`
2. **`expert` subagent BLOCKED** → mark task failed, return to user

Otherwise follow the skill exactly as written.

<!-- codebase-memory-mcp:start -->
# Codebase Knowledge Graph (codebase-memory-mcp)

This project uses codebase-memory-mcp to maintain a knowledge graph of the codebase.
ALWAYS prefer MCP graph tools over grep/glob/file-search for code discovery.

## Priority Order
1. `search_graph` — find functions, classes, routes, variables by pattern
2. `trace_path` — trace who calls a function or what it calls
3. `get_code_snippet` — read specific function/class source code
4. `query_graph` — run Cypher queries for complex patterns
5. `get_architecture` — high-level project summary

## When to fall back to grep/glob
- Searching for string literals, error messages, config values
- Searching non-code files (Dockerfiles, shell scripts, configs)
- When MCP tools return insufficient results

## Examples
- Find a handler: `search_graph(name_pattern=".*OrderHandler.*")`
- Who calls it: `trace_path(function_name="OrderHandler", direction="inbound")`
- Read source: `get_code_snippet(qualified_name="pkg/orders.OrderHandler")`
<!-- codebase-memory-mcp:end -->
