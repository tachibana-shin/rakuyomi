# LNReader module reference

Single reference document for the LNReader/JS source execution mode —
covers what was deferred out of the active build (§1), the feature flag and
config toggle that isolate the whole mode (§2), a reference of what's
actually active in `sdk_lnreader`/`lnreader_packager` today (§3), the
`docs/lnreader/` cleanup history (§4), LNReader's own upstream plugin
discovery index (§5), and a full lifecycle parity audit against Aidoku
sources, step by step (§6). Written at the end of Phase 3.5, updated across
three same-phase follow-up passes: the first added config-default reversal +
initial upstream index support (§2.2/§5.2-§5.4 as originally written), the
second closed the remaining gap between LNReader and Aidoku discovery
end-to-end and fixed the language-mapping gap the first pass had flagged as
unresolved (§5.1/§5.3, as they read now), the third audited every other
lifecycle step (install through uninstall) the same way and fixed one real
bug it found along the way (§6) — this is the day-to-day reference; see
`README.md`'s index for the historical documents (`FEASIBILITY.md`,
`FINDINGS.md`, `ENV_SETUP.md`) this supersedes for "current state" purposes
without duplicating their content.

Format note: `AGENTS.md`'s only rule on comments is "no emojis in code or
comments" plus "KDoc/Javadoc for all Rust public APIs" — no restriction on
doc-comment length or on non-code reference documents. This file is prose
documentation, not inline comments, so it's free to be as detailed as it
needs to be; the existing `///` doc comments throughout `sdk_lnreader` (which
are already extensive and already satisfy the Javadoc requirement) are not
duplicated here — this is an index/map, not a copy.

---

## 1. Deferred code audit (§3.5.1–3.5.6 of the handoff)

The brief was to find code "built by anticipation but never exercised by
NovelBuddy/LNori/Ranobes nor required architecturally," document it, and
pull it out of the active compile path.

**Finding: there isn't any such code left to pull out.** Two categories were
checked and both came back clean:

### 1.1 Genuinely unimplemented features — already stubs, not "built" code

`js_runtime.rs`'s `__lnreader_makeLoudStub` (the `require()` fallback for
`lodash-es`, `urlencode`, `@libs/aes`, `protobufjs`, and `@libs/fetch`'s
`fetchFile`/`fetchProto`) throws a specific, attributable
`"not implemented: require('X').Y"` error only when a plugin actually calls
one of those members — never at `require()` time. There is no dead logic
behind these; they were never built out beyond the stub, per the
all-native-Rust / minimize-dead-weight principle recorded in `FEASIBILITY.md`
(Option 2's "non-negotiable principles" — not worth building for 1 source out
of ~274). Nothing to remove — the restraint already happened at write time,
not after the fact.

### 1.2 Full API surfaces (cheerio, dayjs, htmlparser2 callbacks) — necessary for corpus generality, not speculative

Checked cheerio method usage, dayjs usage, and htmlparser2 handler usage
against the 3 validated sources (NovelBuddy, LNori, Ranobes) plus 2 fixtures
known-broken for unrelated reasons since Phase 2 (NovelUpdates: its search
engine returns 0 results for the test query, not investigated further;
FreeWebNovel: HTTP 403, site-side anti-bot measure, out of scope — neither is
a shim-coverage gap):

- **dayjs**: not called by any of the 5 fixtures at all (confirmed by
  grepping the compiled `.js` for `dayjs`). Still not a removal candidate —
  it's a faithful, bounded port of a real library's date-formatting surface
  that a large fraction of the wider ~274-source `lnreader-plugins` corpus
  uses for release-date parsing (a very common scraper idiom), and Phase 5's
  own stated goal is 80–90% coverage of that corpus, not just the 5 sources
  tested so far.
- **cheerio**: of the ~30 methods implemented, `siblings`/`closest`/`has`/
  `not`/`prev`/`last`/`outerHtml`/`setAttr`/`nodeType` aren't hit by any of
  the 5 fixtures either — but these are standard, widely-used DOM traversal
  methods in the real cheerio API being ported 1:1, not invented extras.
- **htmlparser2**: `onattribute`/`onend` aren't exercised by `ranobes.js`
  (the one fixture that uses `htmlparser2` at all) — and the module's own
  doc comment already says so explicitly: "implemented anyway for fidelity
  with the wider ~133-source corpus." This is the exact same judgment call
  already made and documented in the code itself, not a new finding.

This is precisely the distinction the handoff warned not to blur: "not yet
exercised by these 3 sources" is not "architecturally unnecessary" when the
component being built is a generic library surface (cheerio, dayjs) that
other, not-yet-tried sources will need — the same reasoning that already
protects `@libs/storage`/`@libs/fetch` applies here too, just at a larger
scale.

### 1.3 Non-candidates confirmed correct as-is (not "dead", just narrow)

- `LnReaderSource::get_manga_list` — unreachable from the real app
  (`Backend.lua` has no browse/popular screen) but required by the `Source`
  trait shape; already documented as such at its definition. Kept.
- `BlockingSourceKind`'s 5 `_next`-suffixed `LnReader` match arms
  (`get_search_manga_list_next`, `get_manga_update_next`,
  `get_page_list_next`, `get_image_request_next`, `get_manga_list_next`) —
  already documented at `source/mod.rs` as "dead code in practice, kept only
  so `Source`'s `wrap_blocking_source_fn!` macro... has something to call."
  This is Rust match-exhaustiveness boilerplate, not speculative feature
  code — can't be deleted without changing the macro itself, which is out of
  scope here. Left as-is, now also `#[cfg(feature = "lnreader")]`-gated
  along with the rest of the variant (§2).
- `test_fixtures/` (5 vendored `.js` files, `#[ignore]`d network tests) —
  a Phase 2 implementation note once flagged these for removal as having "no
  precedent in the project" (a stricter policy than `tls.rs`'s own
  hand-written-synthetic-snippet pattern), but by Phase 3 they had become the
  established, working validation mechanism used throughout that phase and
  are still exercised today (`cargo test -p shared --features all -- --ignored
  sdk_lnreader`). Revisiting that reversal was not in this session's scope
  (it's a testing-strategy question, not a dead-code one) — left untouched.

**Conclusion**: no code was removed or relocated this session. The
`sdk_lnreader` tree was already lean by the time Phase 3.5 started.

---

## 2. Isolation: Cargo feature + config toggle (§3.5.4–3.5.7)

Two independent gates, deliberately not conflated:

| Gate | Controls | Default | Changed via |
|---|---|---|---|
| `lnreader` Cargo feature (on `shared`, forwarded by `server`) | Whether the mode is **compiled into the binary at all** | **On** | Build flags (`cargo build --no-default-features --features use_nix` on `server` to drop it) |
| `Settings::lnreader_enabled` (`settings.json`) | Whether a compiled-in mode **actually activates** | **On** — deliberately reversed from the original off-by-default spec, see §2.2 | Edit `settings.json`, restart the server |

### 2.1 Cargo feature (`lnreader`)

- Declared on `shared/Cargo.toml`, default-on, gating: the `sdk_lnreader`
  module (`#[cfg(feature = "lnreader")] mod sdk_lnreader;`), the
  `BlockingSourceKind::LnReader` enum variant and every match arm that
  touches it, and the two crate-external entry points
  (`lnreader_extract_plugin_metadata`, `lnreader_worker_main`) that
  `lnreader_packager`/`lnreader_worker` call into `shared` through.
- `server/Cargo.toml` forwards it (`lnreader = ["shared/lnreader"]`, in
  `server`'s own default features) rather than just inheriting `shared`'s
  default, so `server` alone can be built without it:
  `cargo build -p server --no-default-features --features use_nix`.
- `lnreader_worker`/`lnreader_packager` (the two standalone binaries that
  only make sense with the mode present) declare `shared`'s `lnreader`
  feature explicitly in their own `Cargo.toml` rather than relying on
  `shared`'s default — they're not linked into `server`, so they're
  unaffected by whatever feature set `server` is built with.
- **`boa_engine` itself stays an unconditional dependency of `shared`** —
  per the handoff's own conditional ("si elle n'est utilisée nulle part
  ailleurs"), it's also used by `wasm_imports::next::js` and
  `wasm_store::JsContext` for Aidoku-"next" sources' own embedded JS
  contexts, confirmed by grep before touching anything. Only `boa_gc`
  (used exclusively by `sdk_lnreader/{cheerio,net,js_runtime}.rs`, confirmed
  by grep) was made optional, gated by the feature. Note this doesn't
  actually shrink `cargo tree` — `boa_engine` pulls `boa_gc` in transitively
  as its own internal dependency either way. The real, verified effect of
  the feature is excluding `sdk_lnreader`'s ~4,800 lines and the
  `lnreader_worker` binary from the build, not trimming the crate graph.
- Validated: `cargo check`/`cargo build`/`cargo test` all pass on `server`/
  `shared` both with and without the feature; `cargo test -p shared` goes
  from 142 passed/11 ignored (with) to 125 passed/6 ignored (without) — the
  missing 22 are exactly `sdk_lnreader`'s own tests (5 `#[ignore]`d network
  fixtures + 1 hang/respawn test + 5 cheerio unit tests + 11 dayjs unit
  tests), nothing from the WASM/Aidoku side changes.

### 2.2 Config toggle (`lnreader_enabled`)

> **⚠ Default is `true`, on purpose — this is not a bug, do not "fix" it back
> to `false`.** The original Phase 3.5 spec called for `lnreader_enabled` to
> default to `false` (safe-by-default, opt-in activation). That decision was
> **explicitly and deliberately reversed in a same-phase follow-up**: LNReader
> is currently in an active real-world testing period (packaging/validating
> against the full upstream `lnreader-plugins` catalog, §5) where the
> friction of manually flipping the toggle on for every test session outweighs
> the safety benefit of off-by-default. **Reason, so a future session doesn't
> silently revert this**: active testing phase, not an oversight. Revisit
> only on an explicit decision that the testing phase is over (e.g. once
> Phase 4's UI toggle ships and LNReader is meant to be an opt-in feature for
> end users again) — flip `default_true` back to a plain `#[serde(default)]`
> on `Settings::lnreader_enabled` (`shared/src/settings/schema.rs`) at that
> point, nowhere else needs to change.

- New field on `Settings` (`shared/src/settings/schema.rs`), `bool`,
  `#[serde(default = "default_true")]` → an absent key in an existing
  `settings.json` reads as `true` (see the callout above for why). Note this
  differs from `Settings::default()` (the plain `#[derive(Default)]`, used
  only in unit tests) — that gives `false` regardless, since `#[serde(...)]`
  attributes don't influence the unrelated `Default` derive. The field that
  matters for real usage is the serde one: `Settings::from_file` (the actual
  startup path, `server/src/app.rs`) always goes through deserialization,
  never through `Settings::default()`.
- Enforced at `BlockingSource::from_aix_file` (`shared/src/source/mod.rs`):
  an archive containing `Payload/main.js` is checked against
  `manager.settings.lnreader_enabled` (and the Cargo feature) before being
  handed to `sdk_lnreader::LnReaderSource::from_aix_file` — if either gate is
  closed, it fails with a specific message ("LNReader support is disabled…"
  vs "…compiled without LNReader support") instead of silently falling
  through to the WASM loader and failing with an unrelated "no main.wasm"
  error.
- **Install-time vs. load-time behave differently on purpose.**
  `SourceManager::install_source` (an explicit user action via the HTTP API)
  propagates that error normally — the install just fails, cleanly, with an
  explanation. `SourceManager::load_all_sources` (called at startup and on
  every settings/source-setting update, over *every* installed source in one
  pass) can't do the same: it feeds each `Source::from_aix_file` result
  through `?`, so one archive erroring would have aborted loading every
  other installed source — Aidoku ones included — alongside it. A cheap
  pre-check (`source::is_lnreader_archive`, plain zip inspection, no
  `sdk_lnreader` involved, unconditionally compiled) lets it skip a
  previously-installed-but-now-disabled LNReader source with a `log::warn!`
  instead, so flipping the toggle off never takes the rest of the library
  down. This matters concretely for the toggle-on → install → toggle-off →
  restart sequence, which is otherwise an easy way to brick the backend.
- Covered by `shared/src/source_manager.rs`'s `lnreader_toggle_tests` module:
  `disabled_toggle_rejects_install`/`disabled_toggle_is_skipped_on_load`
  construct `Settings::default()` explicitly (`false`, see above) to exercise
  the disabled path regardless of the Cargo feature; `enabled_toggle_allows_install`
  (`#[cfg(feature = "lnreader")]`) proves the positive path — the one that
  actually matches a real `settings.json` today (§2.2's default-`true`
  reversal) — once both gates are open.
- **No UI toggle this session** (by design — see §3.5 handoff, deferred to
  Phase 4's `Switch` widget reuse). Editing `settings.json` + restarting is
  the only way to flip it for now, same mechanism as every other server-side
  setting in this file.
- **Former scope limitation, resolved in the second same-phase follow-up
  (§5.1)**: `/available-sources` used to list whatever a remote index said
  without being able to tell an LNReader entry from an Aidoku one without
  downloading the `.aix` — so a disabled LNReader source could appear by
  name in the listing even though installing it would be rejected. Now that
  `list_available_sources`/`install_source` detect the shape directly from
  the JSON (an LNReader entry has `url` and no `file`/`downloadURL` — see
  §5.1), a disabled-mode LNReader entry is filtered out of the list
  entirely rather than shown-then-rejected — no download needed to tell the
  two apart, since the shape difference is visible in the index JSON
  itself.

---

## 3. Active reference: `sdk_lnreader` + `lnreader_packager`

What's live today, one paragraph per file/concept rather than a full API
listing (the `///` doc comments already in each file are the API listing).

### 3.1 `shared/src/source/sdk_lnreader/` (gated by the `lnreader` feature)

- **`mod.rs`** — `LnReaderSource`: the third `BlockingSourceKind` variant.
  Implements the same 7 base `Source` operations as `WasmBlockingSource`, but
  delegates every JS-touching call to a persistent, disposable-on-crash
  worker subprocess (`WorkerProcess`, NDJSON over stdin/stdout,
  `WORKER_READ_TIMEOUT` = 120s) instead of an in-process runtime — the
  crash-containment strategy for `boa_engine`'s known native-memory issue on
  large catalogs (see `FINDINGS.md` §1.1/§1.3 for the crash/hang history and
  why process isolation is the mitigation, not a fix).
  `spawn_worker`/`worker_binary_path`/`read_line_with_timeout` are the
  subprocess plumbing; `is_lnreader_archive` (new this session, §2.2) is the
  one function that stays unconditional.
- **`worker.rs`** — the subprocess's own entry point (`run`, called from the
  `lnreader_worker` binary's `main()`), the `WorkerRequest`/`WorkerResponse`
  wire schema, and `execute_*` handlers, one per `Operation` variant
  (`SearchMangas`/`GetMangaDetails`/`GetChapterList`/`GetPageList`/
  `GetImageRequestInitHeaders`). `parse_and_convert_novel` is the shared path
  behind both `GetMangaDetails` and `GetChapterList`, backed by
  `JsRuntime`'s short-lived novel cache (`take_cached_novel`/`cache_novel`,
  the commit `4d89737` fix for the double-`parseNovel()` call documented in
  `FINDINGS.md` §2.1).
- **`js_runtime.rs`** — `JsRuntime`: owns the `boa_engine::Context`, the
  cheerio store, the settings snapshot/pending-writes cell, and the novel
  cache. `RUNTIME_PRELUDE` is the JS-level `require()` shim plus
  `fetch`/`Response`/`FormData`/`URLSearchParams`/`Dayjs`/
  `HtmlParser2Parser` polyfills — thin JS wrappers around the native
  `__native_*` functions the other modules register. `__lnreader_modules`
  is the `require()` table; anything not listed resolves through
  `__lnreader_makeLoudStub` (§1.1).
- **`cheerio.rs`** — the ~30 `__native_*` DOM/selector primitives backing
  the cheerio shim, built on `dom_query` (the same crate the WASM/Aidoku
  side's `html.rs` uses). `Store`/`SharedStore` hold parsed documents and
  element handles; `Store::clear()` runs unconditionally after every
  top-level plugin call (`JsRuntime::call_plugin_method`) — the fix for the
  historical `Box::leak` memory issue, and (per
  `FINDINGS.md` §2.3) more thorough than the WASM/Aidoku
  side's own best-effort reference counting.
- **`net.rs`** — `__native_fetch`: one native HTTP call via the same
  `crate::tls::client_builder()` every other outbound request in the backend
  uses, blocking on `reqwest` via `futures::executor::block_on` (not
  `tokio::task::spawn_blocking`, since the worker process's own thread
  layout already keeps a separate reactor thread free — see
  `lnreader_worker/src/main.rs`'s doc comment on `MIN_WORKER_THREADS`).
- **`htmlparser2.rs`** — `__native_htmlparser2_parse`: one `dom_query`
  parse, replayed as `onopentag`/`onattribute`/`ontext`/`onclosetag`/`onend`
  calls into the JS handler object — not a second parser.
- **`dayjs.rs`** — native `chrono`/`chrono_tz`-backed implementations of
  `.format()`/`.add()`/`.subtract()`/`.diff()`/`.fromNow()` etc., behind
  `__native_dayjs_*`, prioritized by the token frequency measured in the
  original feasibility corpus scan (not by the 5 local fixtures, none of
  which call `dayjs` — see §1.2).
- **`convert.rs`** — `JsValue` ↔ `Manga`/`Chapter`/`Page` conversions
  following the mapping table in `FEASIBILITY.md` §3:
  `manga_from_novel_item` (search results), `manga_from_source_novel`/
  `chapters_from_source_novel` (`parseNovel()`'s dual output),
  `page_from_chapter_html` (the "1 chapter = 1 Page" strategy —
  `chapter_downloader.rs` needs no changes to turn this into an EPUB).
  `parse_release_time` deliberately parses only the documented
  `YYYY-MM-DD` `ChapterItem::releaseTime` format natively, without going
  through the `dayjs` shim (that shim is for JS-visible use only).
- **`metadata.rs`** — `extract`: runs a plugin in a throwaway `JsRuntime`
  and reads its own declared `id`/`name`/`site`/`lang`/`version`/`filters`/
  `pluginSettings` off the live object (not by parsing the source text) —
  the only consumer is `lnreader_packager`, not the runtime itself.
- **`test_fixtures/`** — 5 real, vendored `lnreader-plugins` `.js` builds
  (NovelBuddy, LNori, Ranobes, NovelUpdates, FreeWebNovel), used only by
  `#[ignore]`d network tests in `mod.rs` (`cargo test -p shared --features
  all -- --ignored sdk_lnreader`) plus one non-ignored synthetic hang test
  (`hung_worker_times_out_and_respawns`, hand-written JS, not vendored).

### 3.2 `backend/lnreader_worker/` (standalone binary, needs the `lnreader` feature on `shared`)

`main.rs`: spawns `sdk_lnreader::worker::run()` on a dedicated 64 MiB-stack
thread (headroom for native Rust recursion — `htmlparser2::walk()`, chained
`boa_engine` Promise continuations — the two crash signatures from
`FINDINGS.md` §1.1/§1.4), inside a multi-threaded Tokio
runtime floored at `MIN_WORKER_THREADS = 2` regardless of detected core count
(so `net.rs`'s `futures::executor::block_on` never starves Tokio's own
reactor thread on a single/dual-core e-reader SoC).

### 3.3 `backend/lnreader_packager/` (standalone binary, needs the `lnreader` feature on `shared`)

- **`main.rs`** — CLI entry (`clap`): `package`/`index` from Phase 3, plus
  `fetch` (Phase 3.5 follow-up, see §5) — downloads and packages every plugin
  listed in the upstream `plugins.min.json` index in one pass.
  `package_plugin_js` is the packaging core shared by `package` (one local
  file) and `fetch` (one downloaded file per index entry, best-effort — one
  plugin failing doesn't abort the batch, see §5.3).
- **`plugins_index.rs`** — `fetch_index`/`fetch_plugin_js`: the upstream
  index client (§5). Its own minimal `reqwest::blocking::Client` (this crate
  has no Tokio runtime, unlike `shared`/`lnreader_worker` — `shared::tls`'s
  `client_builder()` returns an async `reqwest::ClientBuilder`, not directly
  usable here), reusing only the shared `DEFAULT_USER_AGENT` string constant
  rather than `shared::tls`'s full TLS/proxy configuration — see §5.4 for why
  that's an accepted tradeoff for a maintainer-run offline tool, not an
  inconsistency to fix.
- **`metadata.rs`** — thin wrapper calling
  `shared::source::lnreader_extract_plugin_metadata` (the one punched-through
  entry point into `sdk_lnreader`).
- **`settings.rs`** — `filters`/`pluginSettings` → `SettingDefinition`
  translation (Switch/Picker/CheckboxGroup/TextInput 1:1;
  `ExcludableCheckboxGroup` still the Phase 3 "lossy" single-`MultiSelect`
  mapping — the two-`MultiSelect` include/exclude replacement is Phase 4
  scope, not touched this session).
- **`package.rs`** — assembles `Payload/main.js` + `Payload/source.json` +
  `Payload/settings.json` into a `.aix`-shaped zip.
- **`index.rs`** — `build_index`: rebuilds an `index.json` entry per
  packaged `.aix` by re-reading each archive's own `Payload/source.json`
  (never trusting whatever the `package` invocation was told), so the index
  can't drift from what's actually on disk.

---

## 4. `docs/lnreader/` folder cleanup (§3.5.9/2.14, two passes)

See the top-level `README.md` in this directory for the current index.
Cleanup happened in two passes within Phase 3.5:

- **First pass**: removed handoffs whose instructions were already fully
  executed and superseded (`PHASE2_HANDOFF{,_FINAL}.md`, `PHASE3_HANDOFF.md`,
  `REVALIDATION_HANDOFF.md`, the two `boa_engine`-investigation handoffs, the
  two efficiency/complexity-investigation handoffs, `PHASE3_5_HANDOFF.md`
  itself once its job was done) and the pre-integration standalone PoC
  (`poc-reference-{main.rs,Cargo.toml}`).
- **Second pass** (same phase, follow-up request): merged the remaining
  *cross-cutting knowledge* documents — which had stayed as separate
  per-research-session files even after the first pass — into single
  reference documents per topic, while explicitly leaving the **phase
  handoffs alone** (those are meant to stay one file per phase, not merged):
  - `FEASIBILITY.md` — the four feasibility reports, condensed to
    conclusions + the commit that implemented each one, exploratory
    reasoning that only mattered pre-decision removed.
  - `FINDINGS.md` — the four investigation findings reports, reorganized by
    topic instead of by research session.
  - `ENV_SETUP.md` — the environment-setup postmortem, rewritten from a
    session-specific incident report into a generic, project-agnostic guide
    for Rust/WASM dev environments on atomic/immutable Fedora-based distros
    (Aurora, Bazzite, other uBlue spins).

`REFERENCE.md` (this file) remains the one day-to-day reference for current
state; `PHASE4_HANDOFF.md` is the one live to-do left in the directory.

---

## 5. LNReader's own upstream plugin discovery index

The Aidoku-side equivalent of this is already in
`server/assets/default-settings.json`'s `source_lists`: URLs like
`https://aidoku-community.github.io/sources/index.min.json`, fetched at
runtime by `usecases::list_available_sources` to show installable,
**already-packaged** sources. LNReader has its own, structurally different
upstream index — confirmed live and fetched during this session:

```
https://raw.githubusercontent.com/LNReader/lnreader-plugins/plugins/v3.0.0/.dist/plugins.min.json
```

### 5.1 End-to-end discovery: `plugins.min.json` lives in `source_lists` too

**Revised in a second same-phase follow-up.** The original version of this
section concluded `plugins.min.json` couldn't go directly into
`source_lists` because it's a **raw, unpackaged** index (points at `.js`
files) rather than Rakuyomi's `.aix`-ready shape, and routed it instead
through a separate, manually-run `lnreader_packager fetch` CLI step. That
was correct as far as it went, but it meant LNReader discovery didn't
actually behave like Aidoku's from the user's side — an extra manual step,
a second tool to know about. This section now describes the fix: the
gap is closed by making `list_available_sources`/`install_source`
understand *both* index shapes directly, so a user adds the
`plugins.min.json` URL to the exact same `source_lists` array they already
use for Aidoku's `index.min.json` URLs, and the rest — listing, installing
— behaves identically regardless of which shape backs a given entry.

**Aidoku's actual fetch lifecycle, confirmed by reading the code (not
assumed)**: there is no cache and no background refresh anywhere in this
path. `list_available_sources` (`shared/src/usecases/list_available_sources.rs`)
does a plain `client.get(source_list.clone()).send()` per URL in
`settings.source_lists`, inside the `async` closure run for every element
of `stream::iter(source_lists)` — i.e. once per `GET /available-sources`
HTTP call, synchronously, no memoization. `install_source` does the exact
same thing independently (its own `client.get(source_list.clone())` call)
once per install attempt. Grepping `source_manager.rs` for
`source_lists`/`cache` turns up nothing — `SourceManager` only manages
already-*installed* `.aix` files read from local disk at startup
(`load_all_sources`), which is a completely different concern from the
*available*-sources index fetched from the network. The one background
`tokio::spawn` in `server/src/app.rs` is `run_manga_cron` (periodic
chapter-update checking for already-tracked manga) — unrelated to
`source_lists` entirely. So Aidoku's lifecycle today is: **fetch fresh,
on demand, every single call, no cache, no startup preload, no periodic
refresh.**

This matters for LNReader because it means there was no separate lifecycle
to reproduce — `plugins.min.json` entries are parsed out of the *same*
per-request HTTP response as any Aidoku entries in the same list (both
shapes can even appear in one `source_lists` document), through the
identical `client.get(source_list.clone())` call in both
`list_available_sources` and `install_source`. There's no second fetch,
no separate cache, no separate refresh schedule to keep in sync — an
LNReader index URL is exactly as fresh, and exactly as un-cached, as an
Aidoku one, because it's fetched by the literal same line of code.

**How the two shapes are told apart**: purely from the JSON itself, no new
`Settings` field, no "kind" tag the user has to specify. Fetch either URL,
normalize the top-level container as before (a bare array, or a
`{"sources": [...]}` wrapper), then look at each entry:

- Has `downloadURL` (Aidoku's own field name) or `file` (the alias
  `SourceListItem` already accepted) → **Aidoku shape**, already a
  packaged `.aix` — handled exactly as before this session.
- Otherwise has `url` (LNReader's `plugins.min.json` field name), and
  neither of the above → **LNReader shape**, a raw, unpackaged `.js`.

This lives in `shared::usecases::list_available_sources::looks_like_lnreader_entry`
and the parallel match in `install_source`'s `SourceListItem` (a
`#[serde(untagged)]` enum with a `Packaged`/`LnReaderRaw` variant — serde
tries `Packaged` first, only falls through to `LnReaderRaw` when `file`/
`downloadURL` aren't present). Two call sites, same detection rule, kept
in sync by both reading off the same field names rather than sharing a
helper across an async/sync boundary that doesn't otherwise exist between
the two functions.

**What happens for each shape**:

| | Aidoku shape | LNReader shape |
|---|---|---|
| **`/available-sources` (listing)** | Parsed straight into `SourceInformation` (unchanged). | Parsed into `packaging::UpstreamIndexEntry`, then converted to `SourceInformation` using the index's own `id`/`name`/`version` *as given* — no execution, no packaging, just for display. See "Trust boundary" below. |
| **Install** | Same as before: download the `.aix` `file`/`downloadURL` field's bytes, hand them to `SourceManager::install_source`. | Download the raw `.js` at `url`, run it through `packaging::package_plugin_js` (the same function `lnreader_packager` uses, moved into `shared` this session — see §3.3) to get `.aix` bytes **in-process**, then hand *those* bytes to `SourceManager::install_source`. No subprocess, no temp files, no separate CLI invocation. |

Both gates from §2 still apply to the install path: if `lnreader_mode_enabled()`
(the new `shared::source::lnreader_mode_enabled` helper — see below) is
false, an LNReader-shaped entry is skipped (with a `log::warn!`) at listing
time and rejected with a clear error at install time — exactly the
"skip gracefully at list/load time, fail loudly at explicit-action time"
split already established in §2.2 for installed archives.

**`lnreader_mode_enabled` helper**: the `cfg!(feature = "lnreader") &&
settings.lnreader_enabled` check used to be duplicated ad hoc in
`SourceManager::load_all_sources` only; it's now
`shared::source::lnreader_mode_enabled(bool) -> bool`, reused by
`load_all_sources`, `list_available_sources`, and `install_source` — three
call sites for the same rule is exactly the point where "just inline it"
stops being the simpler option.

**Trust boundary at listing time vs. install time (deliberate, not a
gap)**: `list_available_sources` trusts `plugins.min.json`'s own `id`/
`name`/`version` fields for **display only** — it does not execute any of
the ~261 listed plugins just to render a catalog (that would mean running
boa_engine 261 times on every `/available-sources` call, which is not
viable). At **install** time, exactly one plugin gets executed
(`packaging::package_plugin_js`), and its own declared `id`/`name`/
`version`/`lang` — read off the live executed object, not the index's
claims — are what actually end up in the installed `.aix`'s
`Payload/source.json`. If a plugin's self-reported metadata ever disagrees
with what the index said (stale index entry, a plugin author changing
`this.id` without updating the index), the installed source reflects the
plugin's own truth, matching how `lnreader_packager package`/`fetch`
already behaved before this session.

**No hardcoded URL, at either layer**: the server never bakes in a
`plugins.min.json` URL anywhere — it only ever sees whatever URL the user
put in `settings.json`'s `source_lists`, fetched through the exact same
code path as any Aidoku `source_lists` entry. `lnreader_packager`'s
standalone `fetch` subcommand (still useful for pre-packaging a static
mirror, or for the kind of bulk corpus validation done in §5.3) had its
`DEFAULT_INDEX_URL` constant removed this same pass — `--index-url` is now
a required flag with no fallback, so there is exactly zero hardcoded
upstream-repo URL anywhere in this codebase, CLI or server.

**Known characteristic, not a bug**: installing an LNReader source takes
measurably longer than installing an Aidoku one — a boa_engine execution
plus a zip write happen synchronously inside the install HTTP request,
versus a plain byte download for Aidoku. Every plugin in the 259/261
validated batch (§5.3) packaged in well under the kind of time that would
threaten an HTTP request timeout, so this is a UX note, not a functional
problem — flagged here in case a future session sees an install stall and
wonders whether something's wrong (it likely isn't; check the boa_engine
crash/hang findings in `FINDINGS.md` §1.1 first if an install actually
hangs, don't assume it's this).

**Verified with a real, complete install — not just unit tests, not just
`cargo check`.** Everything above (shape detection, the `SourceListItem`
enum, `lnreader_mode_enabled`, `package_plugin_js` being called in-process)
was exercised in isolation by unit tests already, but none of those prove
the *whole* HTTP path actually works when a real user follows the real
steps — a genuinely separate risk (wrong route wiring, a state-lock
ordering bug, a real network/`boa_engine` failure against a live site) that
only a real run can catch. So one was run, end to end, no shortcuts:

1. A local, gitignored `docs/lnreader/test_home/settings.json` (covered by
   the existing `docs/lnreader/` `.gitignore` pattern — no new entry
   needed) was written with **both** index URLs in the same `source_lists`
   array — Aidoku's `https://aidoku-community.github.io/sources/index.min.json`
   and LNReader's `https://raw.githubusercontent.com/LNReader/lnreader-plugins/plugins/v3.0.0/.dist/plugins.min.json`
   — plus `"lnreader_enabled": true`. No `lnreader_packager` CLI was run at
   any point during this test.
2. `cargo build -p server` (default features, `lnreader` on), then the
   resulting binary run directly against that home directory —
   `./target/debug/server docs/lnreader/test_home` — exactly the same
   entry point a real deployment uses, not a test harness.
3. `GET /available-sources` returned **389 entries** — 128 from Aidoku's
   index + 261 from LNReader's, confirmed by cross-checking the count
   against each index fetched independently. `novelbuddy` (LNReader,
   `version: 2001003` — the encoded `"2.1.3"`) and `en.novelbuddy` (Aidoku,
   `version: 4`) both appeared in the same flat list, sorted together by
   name, structurally identical from the response's point of view.
4. `POST /available-sources/novelbuddy/install` with body `"raw.githubusercontent.com"`
   (the real `source_of_source` value for that entry) returned `200`. The
   resulting `docs/lnreader/test_home/sources/novelbuddy.aix` was unzipped
   and inspected directly: `Payload/source.json` contained
   `{"id":"novelbuddy","lang":"en","name":"NovelBuddy","version":2001003,"url":"https://novelbuddy.me/",...}`
   — the plugin's own declared `lang`/`site`, not a guess, and the
   URL-folder fallback wasn't even needed here since `novelbuddy.js`
   declares `this.lang` itself.
5. `GET /mangas?q=sword` (Rakuyomi's normal cross-source search, no
   source-specific code path) returned 24 real results with
   `"source":{"id":"novelbuddy",...}`, fetched live from novelbuddy.me
   through the installed plugin's actual `searchNovels` implementation
   running in `boa_engine`.
6. `POST .../refresh-chapters` then `GET .../chapters` on one of those
   results returned **858 real chapters** scraped from the live site.
7. `POST .../chapters/{id}/download` returned `200` with an output path and
   an empty error list; the file at that path was a genuine, valid EPUB
   (`file` identified it as "EPUB document", `unzip -l` showed a normal
   `mimetype`/`META-INF`/`OEBPS` structure, and `OEBPS/pages/page_1.xhtml`
   contained ~15 KB of the chapter's real prose).

**Result: it worked exactly as designed, with no missing bridge.** The
concern this test was meant to rule out — that `install_source` might
still be treating the downloaded bytes as a ready-made `.aix` binary
instead of routing raw `.js` through the packager — does not hold. Here is
precisely which part of the shared path performs the `.js` → `.aix`
transformation, and when: inside `usecases::install_source`
(`backend/shared/src/usecases/install_source.rs`), in the
`SourceListItem::LnReaderRaw` match arm (see excerpt further up this
section) — *after* the matching source-list entry has been found by
`source_id`, but *before* `source_manager.install_source(...)` is ever
called. Concretely, this happens synchronously within the single
`POST /available-sources/{id}/install` HTTP request:
`client.get(url.as_str()).send().await?.text().await?` downloads the raw
`.js`, then `packaging::package_plugin_js(&main_js, Some(&url))` executes
the plugin (`boa_engine`), reads its metadata, and zips the `.aix` bytes
in memory. Only the resulting `Vec<u8>` — indistinguishable in shape from
an Aidoku download's bytes — is handed to
`source_manager.install_source(&source_id, aix_content, ...)`.
`SourceManager` itself contains no LNReader-specific logic at all and
never learns which shape the entry originally had; the entire adaptation
lives in this one match arm, one layer above it.

### 5.2 Real shape (fetched and inspected, not assumed from docs)

A flat JSON array, 261 entries as of this session. Each entry:

```json
{
  "id": "arnovel",
  "name": "ArNovel",
  "site": "https://ar-no.com/",
  "lang": "‎العربية",
  "version": "2.2.0",
  "url": "https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/.js/src/plugins/arabic/ArNovel[madara].js",
  "iconUrl": "https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/public/static/multisrc/madara/arnovel/icon.png"
}
```

**Field coverage against what `lnreader_packager`/`sdk_lnreader` need:**

| Field | Covers | Notes |
|---|---|---|
| `id`, `name`, `site`, `version` | Identification, progress reporting, and (since the §5.1 follow-up) the `/available-sources` listing display | Not trusted for the actual **install** — the packaging pipeline re-derives all of these by *executing* the downloaded plugin (`RawMetadata`), which is more authoritative (handles constructor-parameterized plugins like `ranobes.js` that don't have these as literal source-text properties). Index copies are used for CLI progress/error messages and, now, for what a user sees in the app's source list *before* installing — see §5.1's "Trust boundary" note for why that split is deliberate. |
| `url` | **The one load-bearing field** | Direct link to the plugin's own compiled `.js` — this is what turns "manually locate a file on disk" into "fetch the index, download by URL." Without this field there would be nothing to automate. |
| `iconUrl` | Not covered anywhere | Neither `SourceInfo`/`SourceManifest` (`shared::source`) nor `SourceInformation` (`shared::model`) models a source icon at all — not an LNReader-specific gap, Aidoku sources have no icon field either. Parsed and kept on `PluginIndexEntry` for completeness, `#[allow(dead_code)]`'d, not read by anything. Would need a schema change on the Rakuyomi side (both source kinds) to ever use it. |
| `filters`/`pluginSettings` | **Not present in the index at all** | Expected, not a gap — these can only be known by executing the plugin (`__lnreader_plugin.filters`), which `lnreader_packager` already does for every plugin regardless of the index. |

**Verdict: the index covers what it needs to** — full generation of an
`.aix` still runs the exact same execute-and-extract pipeline as the
single-file `package` command; the index only replaces manual file discovery.

### 5.3 Real-world validation: `lnreader_packager fetch` against the live index

Ran to completion against the real, live 261-entry index during this
session (not a subset, not a dry run):

**259/261 packaged successfully (99.2%), zero manual intervention.** The 2
failures:
- `readfrom`: `ReferenceError: Headers is not defined` — the plugin
  constructs a `Headers` object (Fetch API), which `js_runtime.rs`'s prelude
  doesn't currently polyfill (only `Response`/`FormData`/`URLSearchParams`
  are — see §3.1).
- `RLIB`: `ReferenceError: Intl is not defined` — the plugin uses JS's
  `Intl` (internationalization) global, not something `boa_engine` provides
  and nothing in `sdk_lnreader` polyfills.

Both are small, well-isolated, easy future shim additions (a `Headers`
polyfill mirroring the existing `FormData`/`URLSearchParams` ones; either a
minimal `Intl.NumberFormat`/`DateTimeFormat` native shim or a loud stub if
the specific plugin's usage turns out marginal) — **not fixed this session**,
logged here as the next concrete, evidence-backed shim gaps for whoever picks
up broader corpus coverage next, in the same spirit as the already-documented
NovelUpdates/FreeWebNovel exceptions from Phase 2.

**Secondary finding from this run, fixed in a same-phase follow-up (see
below)**: among the 259 packaged sources, only 3 ended up with a non-`null`
`lang` field in their generated `Payload/source.json` (`hotnovelpub` →
`"en"`, `lightnoveldaily` → `"es"`, `thnovels` → `"th"`) — the other 256
packaged with `lang: null`, even though the upstream index effectively
knows every one of their languages. Most LNReader plugins simply never set
`this.lang` on their own plugin instance (the property `RawMetadata`/
`sdk_lnreader::metadata::extract` reads), so there was genuinely nothing to
extract for those sources at the time.

**Fix implemented in a same-phase follow-up**: the naive approach — passing
`UpstreamIndexEntry::lang` (the index's free-text display name, "English",
"中文, 汉语, 漢語", "‎العربية" with a stray Unicode direction-mark) straight
through as a fallback — was correctly rejected in the original version of
this section, since that field is not a stable, exact-match-safe key. The
fix instead uses a different field that *is* a stable key: **the language
folder segment already embedded in every entry's own `url`**. Every
`plugins.min.json` `url` has the shape
`.../src/plugins/<folder>/PluginName[...].js`, and `<folder>` is one of a
small, closed set of English folder names `lnreader-plugins` itself
organizes its source tree by — confirmed by fetching the live index and
extracting every distinct folder actually in use:

| Folder | Entries (of 261) | ISO-639-1 |
|---|---|---|
| english | 140 | `en` |
| russian | 21 | `ru` |
| arabic | 16 | `ar` |
| spanish | 16 | `es` |
| french | 14 | `fr` |
| turkish | 14 | `tr` |
| portuguese | 10 | `pt` |
| indonesian | 9 | `id` |
| chinese | 7 | `zh` |
| vietnamese | 4 | `vi` |
| japanese | 2 | `ja` |
| korean | 2 | `ko` |
| thai | 2 | `th` |
| ukrainian | 2 | `uk` |
| multi | 1 (`komga`) | *(none — see below)* |
| polish | 1 | `pl` |

All 261 entries fall into one of these 16 folders — no unmapped folder was
found. `multi` (currently just `komga`, a genuinely multi-language plugin
by nature) deliberately has **no** ISO code in the table: forcing one would
be actively wrong, so a `multi`-folder plugin is left with no language
fallback rather than a guessed one, same principle as leaving a truly
unknown value alone.

**Not invented — checked against how Aidoku's own multi-language sources
already behave in this exact codebase, and matched exactly.** Rather than
decide the `multi`/no-identifiable-language treatment in isolation, the
real Aidoku corpus (128 sources, live index fetched and inspected) was
checked directly:

- Aidoku's own multi-language sources (5 of them tagged with the literal
  string `"multi"`, e.g. `multi.suwayomi`/`multi.cubari`/`multi.lanraragi`;
  plus 3 more aggregators like `multi.mangadex` that instead enumerate
  every language they cover, `"All"` included) all express this through a
  **plural** `languages: [...]` array in their own `Payload/source.json` —
  confirmed by downloading and unzipping a real one
  (`multi.suwayomi-v4.aix`):
  ```json
  { "info": { "id": "multi.suwayomi", "name": "Suwayomi", "version": 4,
      "url": "https://github.com/Suwayomi", "contentRating": 0,
      "languages": ["multi"] } }
  ```
  A **non**-multi Aidoku source (`en.aquamanga`, downloaded and unzipped
  the same way) uses the exact same plural shape — `"languages": ["en"]`
  — never a singular `"lang"` key. This is universal across the corpus:
  every real Aidoku `.aix` populates `languages`, none populate `lang`.
- Rakuyomi's own `SourceInfo` struct (`shared/src/source/mod.rs`) has a
  singular `lang: Option<String>` field always compiled in, and a plural
  `languages: Option<Vec<String>>` field that only exists
  `#[cfg(not(feature = "all"))]` — i.e. **not present at all** in any real
  server build, since `server`'s `Cargo.toml` always builds `shared` with
  `features = ["all"]`. So for every installed Aidoku source in a real
  Rakuyomi server, single-language or multi-language alike, parsing its
  `Payload/source.json` finds no `lang` key to fill the singular field
  (Aidoku only ever wrote `languages`) and silently drops the `languages`
  key entirely (the field isn't even compiled in) — leaving
  **`SourceInfo.lang == None` for literally every Aidoku source that
  exists today**, not just the multi-language ones.

Given that, "how does Aidoku treat a multi-language source" and "how does
Aidoku treat a source with no identifiable language" turn out to be **the
same question with the same answer**: `lang: None`, universally, because
the field this whole section is about isn't one Aidoku's own tooling ever
populates in the first place. Reproducing that behavior for LNReader's
`multi` folder and for any URL shape `lang_from_index_url` can't map means
doing exactly what this module already does — return `None`, don't guess —
which is what was implemented from the start, not a new special case added
to match this finding. The one place LNReader-origin sources end up ahead
of Aidoku-origin ones is precisely because LNReader plugins declare `lang`
as a genuine singular string property on the executed object (`this.lang`)
in a way Aidoku's own `.aix` format has no equivalent for — an asymmetry
worth naming so a future session doesn't read "0 Aidoku sources have a
non-null `lang`" as evidence of a bug.

Implementation: `shared::source::sdk_lnreader::packaging::lang_from_index_url`
extracts the segment right after `.../src/plugins/` and looks it up in a
`const LANG_FOLDERS: &[(&str, &str)]` table (the 15 mapped rows above).
`package_plugin_js` calls it as a **fallback only** — if the executed
plugin already sets its own `this.lang` (still true for `hotnovelpub`/
`lightnoveldaily`/`thnovels`), that value wins unchanged; the URL-derived
code only fills in when the plugin's own `lang` is absent or empty. This
preserves the "executed plugin's own truth is authoritative" principle from
§5.1 while finally giving the other 256 sources a real, correctly-formatted
`SourceInfo.lang` instead of `null`.

`UpstreamIndexEntry::lang` (the free-text display name) is still kept on
the struct and still not used as a mapping key anywhere — it remains
available for a possible future UI use (showing a human-readable language
name somewhere) but plays no role in packaging or filtering.

No extra network call was needed for any of this — the folder segment
comes from the exact same `url` field already fetched as part of the
single `plugins.min.json`/per-plugin `.js` requests the packaging pipeline
was already making; see §5.1's "no hardcoded URL, at either layer" note,
which this respects unchanged (the folder table is a static, offline
mapping, not a call to any upstream API).

**One thing this does *not* claim to fix**: verifying where
`SourceInfo.lang` is actually consumed downstream turned up that, as of
this session, **nothing in the current codebase filters the source list by
language** — `Settings.languages` is exposed to WASM/Aidoku plugins as a
global setting so *they* can filter their own chapter results, and
`chapter.lang` is what the frontend (`filterChaptersByLang.lua`,
`ChapterListing.lua`) actually filters/displays by, both per-chapter, not
per-source. `SourceInfo.lang` (what this fix populates) isn't wired into
either `SourceInformation` (the API-facing shape for `/available-sources`/
`/installed-sources`, which only carries `id`/`name`/`version`) or any
visible UI filter today. **The previous version of this section's claim
that `Settings.languages` matches against `SourceInfo.lang` was incorrect**
— corrected here rather than left to mislead a future session. This fix is
still worth having (a correctly-populated `lang` in `Payload/source.json`
is a real correctness improvement in its own right, and is a prerequisite
for any future source-level language filter), but it does not, by itself,
make any UI or filtering behavior different today.

### 5.4 Tag/branch pinning: a known, accepted maintenance point

`plugins/v3.0.0`, wherever a user puts it (their own `settings.json`
`source_lists` entry, or as an argument to `lnreader_packager fetch
--index-url`), is a **branch name**, not a release tag pinned to one
snapshot — confirmed still receiving new plugins on an ongoing basis (261
entries fetched this session; Phase 2's original count, months earlier, was
lower). It will go stale whenever `lnreader-plugins` eventually cuts a new
major-version branch (`plugins/v4.0.0` or similar). Unlike the two Aidoku
`index.min.json` URLs, which *are* hardcoded as editable defaults in
`server/assets/default-settings.json`, **this codebase carries no default
LNReader index URL anywhere** (§5.1) — so there's no shipped default to go
stale; the pinning risk lives entirely in whatever URL each individual user
chose to type into their own `settings.json` or CLI invocation, and updating
it is exactly as easy as editing that one line, same as updating any other
`source_lists` entry.

**No tag-following mechanism exists** (e.g. querying GitHub's API for the
current default/latest `plugins/*` branch and using that automatically) —
**deliberately not built**, per the explicit instruction to document rather
than necessarily implement this. If it's ever worth building: the `GET
/repos/LNReader/lnreader-plugins/branches` GitHub API endpoint would list
current branches, from which a `plugins/vN.*` pattern match could pick the
highest version automatically — a genuinely separate small feature (network
call + version-string parsing + a decision on how often to re-check), not a
one-line change anywhere in this codebase (there being no default URL left
to change). Low urgency today: the branch has been live and actively
maintained since Phase 2, and nothing about the current design blocks
anyone from updating their own `source_lists` entry by hand if it ever goes
stale.

## 6. Full lifecycle parity audit: Aidoku vs LNReader, step by step

Guiding principle for this pass, stated up front because it shaped every
verdict below: the goal is making LNReader converge on Aidoku's existing
behavior, not the other way around. Aidoku/shared code was only touched
where a bug was real and shared by both paths, with the smallest possible
diff; everything else confined to `sdk_lnreader`.

**Method.** The real step list below was built by reading
`backend/shared/src/usecases/` and every `backend/server/src/*/routes.rs`,
not assumed in advance. For each source-dependent step, the same action was
run against two real, freshly-installed sources in the same gitignored
`docs/lnreader/test_home/` from §5.1 — `en.asurascans` (Aidoku, legacy WASM
ABI; `en.aquamanga`, the source used for §5.1/§5.3's `.aix`-inspection, was
tried first but returned zero results for every query with no error, an
unrelated live-site issue with that one scraper, not a WASM/Aidoku pipeline
problem — asurascans was substituted and worked normally) and `novelbuddy`
(LNReader, already validated end-to-end in §5.1) — comparing observed HTTP
responses, not just reading the code. Steps that turned out to be
source-agnostic (library, tracking, notifications, viewer/scanlator
preferences — anything keyed purely by `MangaId`/`ChapterId` strings with no
`Source` call in its usecase) were confirmed by reading every usecase they
route through, then spot-checked live rather than exhaustively re-tested,
since there is no per-source code path in them to diverge in the first
place.

| Step | Aidoku behavior | LNReader behavior | Verdict | Why |
|---|---|---|---|---|
| List available sources + install | `list_available_sources`/`install_source` treat both index shapes identically; install downloads `.aix` bytes directly | Raw `.js` downloaded and packaged in-process via `packaging::package_plugin_js` before reaching `SourceManager` | **Parity** | Already verified end-to-end in §5.1; not re-run here |
| Installed-sources listing: `source_of_source` | `WasmBlockingSource::from_aix_file` reads the `.{stem}.source` sidecar file and sets `manifest.source_of_source` | **Was always `null`** — `LnReaderSource::from_aix_file` parsed `Payload/source.json` but never read the sidecar file `write_meta_file` had written at install time | **Fixed** | Real bug, confirmed live (`/installed-sources` showed `"source_of_source":null` for `novelbuddy` right after a successful install with a real domain). Fixed by adding the exact same sidecar read Aidoku already does, in `LnReaderSource::from_aix_file` (`backend/shared/src/source/sdk_lnreader/mod.rs`) only — zero lines changed in the Aidoku path. Confirmed fixed live: reinstalling `novelbuddy` after the fix now reports `"source_of_source":"raw.githubusercontent.com"` |
| Uninstall | `SourceManager::uninstall_source`: delete the `.aix`, drop from `sources_by_id` | Identical — no source-kind branching in `uninstall_source` at all | **Parity** | Tested live for both; both disappear from `/installed-sources` after `DELETE` |
| Setting definitions (filters/settings screen) | `BlockingSource::setting_definitions` dispatch, real per-source `Payload/settings.json`/filter list | Same dispatch; `novelbuddy` returned 8 real definitions (multi-select genre/demo, text, select) derived from its own `filters` object | **Parity** | Tested live: both return well-formed `SettingDefinition` arrays over the same endpoint; `en.aquamanga` happened to declare none, which is a property of that one source, not a mode difference |
| Stored settings get/set | `HashMap<String, SourceSettingValue>`, source-agnostic persistence | Identical | **Parity** | Tested live: set `genre: ["fantasy"]` on `novelbuddy`, read back unchanged; same endpoint/shape Aidoku sources use |
| Search by query | `search_mangas_by_filters_inner(vec![Filter::Title(query)])` (legacy) or `get_search_manga_list_next` (next ABI) | `worker::Operation::SearchMangas` roundtrips through the plugin's own `searchNovels` | **Parity** | Tested live: `en.asurascans` (10/34 results for "solo") and `novelbuddy` (24 results for "sword", cross-checked in §5.1) both return real, live-scraped results with no errors |
| Search pagination (`page` > 1) | **Legacy** WASM ABI hardcodes `if page > 1 { return Ok((Vec::new(), false)) }` (`WasmBlockingSource::search_mangas`) — only the "next" ABI (`min_app_version >= 0.7.0`) actually paginates | `LnReaderSource::search_mangas` always forwards `page` to the plugin and returns its real `has_next_page` | **Legitimate difference — not a gap** | Tested live: `en.asurascans` (a legacy-ABI source) returned 0 results on page 2; `novelbuddy` returned a full page. This tracks Aidoku's *own* two-tier ABI split (legacy vs "next"), not an Aidoku/LNReader split — LNReader's pagination already matches what Aidoku's own "next"-ABI sources do. Nothing to change: making LNReader regress to the legacy cap would be converging on the *worse* of Aidoku's two own behaviors |
| Manga details (incl. cover) | `get_manga_details`/`get_manga_update_next`; poster cached to `downloads/.posters/` and exposed as a `file://` URL | `worker::Operation::GetMangaDetails` → `MangaDto::into_manga`; same poster caching path (source-agnostic, in `search_mangas.rs`/`chapter_storage`) | **Parity** | Tested live: both `en.asurascans` and `novelbuddy` returned full `tags`/`status`/`description`/cached `cover_url` after `refresh-details` + `GET details`; identical response shape |
| Chapter list | `get_chapter_list`/`get_manga_update_next` | `worker::Operation::GetChapterList` → `ChapterDto::into_chapter` | **Parity** | Tested live: 9 chapters for `en.asurascans`, 858 for `novelbuddy`, both well-formed |
| Chapter number (`chapter_num`) | Populated when the source's own scraper sets it; frequently absent (Rakuyomi's own `mark_chapters_as_read::parse_chapter_ranges` already falls back to positional index for exactly this reason — pre-existing, Aidoku-authored code) | `novelbuddy`'s own `parseNovel()` (fetched and read directly from the live plugin `.js`) builds chapter objects as `{name, path, releaseTime}` — no chapter-number field at all, by the plugin author's own choice | **Legitimate difference — not a gap** | Confirmed live (`en.asurascans` chapters had `"chapter_num":9.0`; every one of `novelbuddy`'s 858 chapters had `"chapter_num":null`) and confirmed in the plugin's own source that this isn't an extraction bug — `sdk_lnreader::convert` does read `chapterNumber` when a plugin sets it (see §5.3's `hotnovelpub`/`lightnoveldaily`/`thnovels` for `lang` set the same way), `novelbuddy` simply never does. Exactly the same "optional field some sources skip" shape as `lang`/`SourceInfo.languages` in §5.3, and the existing positional-index fallback already handles it — nothing LNReader-specific to add |
| Chapter content: text vs image, and CBZ vs EPUB | `get_page_list` returns `Page`s with an image URL, `Page.text: None` | `get_page_list` returns one `Page` with `Page.text: Some(html)`, converted from the fetched chapter HTML | **Structural — must stay different** | Not source-kind branching: `chapter_downloader.rs`'s `download_chapter_pages` decides the output format with one line, `let is_novel = pages.first().and_then(\|p\| p.text.as_ref()).is_some();`, then calls `download_chapter_novel_as_epub` or `download_chapter_pages_as_cbz` — the exact same generic function either mode goes through. Verified live: `novelbuddy` produced a real, valid EPUB with prose in `OEBPS/pages/page_1.xhtml` (§5.1); `en.asurascans` produced a real, valid CBZ (`file`-identified Zip archive, `ComicInfo.xml` + real `.webp` pages) |
| Revoke a downloaded chapter | `usecases::revoke_manga_chapter` — `ChapterId`/`ChapterStorage` only | Identical | **Parity** | Tested live for both; both return `true` |
| Mark chapter(s) as read, update-last-read, preferred scanlator, viewer, add/remove library, tracking (search/link/unlink/sync/dates), notifications (count/list/delete/clear), storage stats, orphan-file cleanup, database sync | All of these usecases take only `MangaId`/`ChapterId` strings and `Database`/`ChapterStorage` — none call into `Source` at all | Identical, by construction | **Parity (source-agnostic)** | Confirmed by reading every route/usecase in this group — no `SourceExtractor`, no `Source`/`BlockingSource` call exists in any of them. Spot-checked live anyway: `add-to-library` and `mark-as-read` both succeeded for `en.asurascans` and `novelbuddy` and showed up correctly in `GET /library` |
| Source-specific notification hook (`handle_source_notification`) | `WasmBlockingSource::handle_notification_next` — a real Aidoku "next"-ABI hook (login/deep-link flows) | `LnReaderSource::handle_notification_next` is a deliberate no-op — "no LNReader equivalent of Aidoku's notification hook" (see the doc comment already on that function) | **Structural — must stay different** | Not live-tested (needs a source that actually registers a notification, which neither test source does) — verdict taken directly from `sdk_lnreader/mod.rs`'s own doc comment, written when the function was implemented, and left unchanged here since it's already correct: a no-op is the right behavior for a mode with nothing to hook into, not a gap |
| Aidoku "next"-ABI-only entry points (`get_search_manga_list_next`, `get_manga_update_next`, `get_page_list_next`, `get_image_request_next`, `get_manga_list_next`) | Only reachable when `next_sdk()` is `true` (Aidoku's own newer WASM ABI, filters/pagination-aware) | `BlockingSourceKind::LnReader` arms all `bail!("... is not supported for LNReader sources")`; `next_sdk()` is hardcoded `false` for `LnReader`, so the base 7-method dispatch is always used instead and none of these are ever actually reached from a usecase | **Structural — must stay different** | Confirmed by reading the dispatch table in `backend/shared/src/source/mod.rs` (lines ~652-753): these 5 methods exist only because `Source`'s generic macro-based facade needs *something* to call for every mode, not because LNReader has an equivalent ABI to converge on |
| Error handling: request for a nonexistent manga | `en.asurascans`'s `get_manga_details` returned `200` with an all-empty placeholder `Manga` (title `""`, every optional field `null`) instead of an error, for a manga id that doesn't exist | `novelbuddy`'s worker correctly detected the missing manga and returned an error, which surfaced as the generic `AppError::NotFound` → `404 {"message":"Requested item was not found"}` from `get_cached_manga_details`'s route handler | **Legitimate difference — not fixed, and not an Aidoku/LNReader split** | Tested live on both. This is *not* a shared-code bug (the 404 path is the same generic `AppError::NotFound` any missing cache entry hits) and not architectural — it is specifically `en.asurascans`'s own WASM binary silently accepting a 404 page as if it were valid content instead of raising an error, which is that one compiled source's own scraping robustness, not something this codebase's shared code or the LNReader adapter has any hand in. Out of scope per this session's rule against touching Aidoku/shared code without a real, shared bug — and there's nothing to "fix" on the LNReader side either, since failing loudly here is already the more correct behavior, not a bug to converge away from |
| Install-time cost | Plain byte download | Synchronous `boa_engine` execution + zip write inside the install request | **Structural — already documented** | See §5.1's "Known characteristic, not a bug" |

**UI/Lua scope note.** This audit's method (real HTTP requests against the
server, not the KOReader frontend) surfaced nothing new for
`AvailableSourcesListing.lua`/`InstalledSourcesListing.lua` beyond what
`PHASE4_HANDOFF.md` already tracks (the available-sources language/second-axis
filter, and the `XCheckboxGroup` include/exclude mapping) — both stay
exactly as already scoped for Phase 4, untouched here.

### 6.1 Upgrade path for the `source_of_source` fix: a restart is enough, no reinstall needed

The fix (§6's table, row 2) only changed *reading* the `.{stem}.source`
sidecar file — `write_meta_file` already wrote it correctly for every
LNReader source ever installed, pre-fix or post-fix. That predicts a plain
binary restart should be enough to self-heal already-installed sources,
with no reinstall. Tested for real rather than assumed:

1. Built two real binaries from this exact codebase: one from *before* the
   fix (`LnReaderSource::from_aix_file` with the sidecar-read block
   removed) and one from *after* it — same source tree otherwise, swapped
   by temporarily reverting just that block, building, copying the binary
   aside, then restoring the block and rebuilding.
2. Started the **pre-fix** binary against a fresh `docs/lnreader/test_home`
   and installed `lnori` (a real LNReader source, not previously
   installed). `GET /installed-sources` showed
   `"source_of_source":null`, as expected. The sidecar file itself,
   `sources/.lnori.source`, already contained
   `{"from":"raw.githubusercontent.com","is_next_sdk":null}` on disk at
   this point — confirming the write path was never the problem, only the
   read.
3. Stopped the pre-fix binary. Started the **post-fix** binary against the
   *same* `test_home` directory, with **no reinstall, no CLI step, no
   touching the sidecar file** — a plain restart.
4. `GET /installed-sources` immediately showed
   `"source_of_source":"raw.githubusercontent.com"` for the same `lnori`
   install.

**Conclusion, stated explicitly so no one assumes a reinstall is
needed**: a normal Rakuyomi restart onto a build containing this fix is
**sufficient** to correct `source_of_source` for every LNReader source a
user already has installed. No migration step, no reinstall, no manual
edit of any file — the sidecar data was always there, waiting to be read.

### 6.2 Cross-source pass: LNori, Ranobes, NovelUpdates, FreeWebNovel

§6's table was built entirely against one LNReader source (`novelbuddy`).
Per the Phase 2 (commit `4761ca5d`) and Phase 3 (commit `9400a1d`)
fixture lists — recovered from `FEASIBILITY.md`/`FINDINGS.md`/commit
messages rather than guessed: **NovelBuddy, LNori, Ranobes, NovelUpdates,
FreeWebNovel** — the same lifecycle steps were re-run against the other
four, freshly installed in the same `test_home` alongside the sources
already covered by §6's table, specifically looking for anything a single
site wouldn't reveal.

**NovelUpdates and FreeWebNovel (`FWN.com` in the live index — distinct
from the unrelated Aidoku source `en.freewebnovel`) are still broken for
exactly the pre-existing, unrelated reasons** (§1.2): `novelupdates`
installs fine and searches with **no error**, but returns 0 results for
every query tried (`love`, `king`, `system`) — a site-side search issue,
not a regression. `FWN.com` installs fine but every search fails with a
clean, well-formed `SearchError` — `"searchNovels rejected: Error: Could
not reach site ('403')..."` — surfaced through the exact same
`SearchError`/`errors` array Aidoku sources use for their own failures
(confirmed structurally in §6's table already; this is the same mechanism
producing a real error for a real anti-bot block, not a new path). Neither
finding is new, but both were worth re-confirming live rather than assumed
still true.

**LNori and Ranobes both worked end-to-end** (search → details → refresh
chapters → download), and surfaced two genuinely new data points, plus one
non-finding that took real digging to rule out:

- **Manga/chapter ID diversity, handled correctly.** `novelbuddy`'s IDs
  are flat slugs; LNori's manga id is a `series/<id>/<slug>` path (e.g.
  `series/9635/chitose-is-in-the-ramune-bottle`) and its chapter ids
  contain a literal `#` (e.g.
  `book/26487/chitose-is-in-the-ramune-bottle-vol-1#page01`); Ranobes'
  manga id starts with a literal `/` and ends in `.html`, with numeric
  chapter ids under a completely different path
  (`/complete-martial-arts-attributes-v812312-935355/3238621.html`). All
  three shapes round-tripped correctly through every route tested (details,
  chapters, download) once percent-encoded as a single path segment —
  **parity**, and confirms `MangaId`/`ChapterId` really are treated as
  opaque strings everywhere in this codebase, with no assumption about
  their shape baked in anywhere (Aidoku source ids are just as free-form,
  e.g. numeric-string chapter ids like `en.asurascans`'s `"9"` from §6's
  table).
- **`chapter_num` availability is genuinely per-plugin, confirmed a third
  way.** LNori's chapters *do* set a real `chapter_num` (`1.0` for the
  first chapter) — unlike `novelbuddy` (always `null`, §6's table) — while
  Ranobes, like `novelbuddy`, leaves it `null` even though the chapter
  *title* contains a real number ("Chapter 4642: ..."). Three sources, three
  different choices, none of them wrong: this is the same
  "optional field some plugins skip" shape as `lang` (§5.3) and the first
  `chapter_num` finding (§6's table), now confirmed to vary
  source-by-source within LNReader itself, not just LNReader-vs-Aidoku —
  reinforces that the existing positional-index fallback in
  `mark_chapters_as_read::parse_chapter_ranges` is the right general
  solution, not a special case for one source.
- **LNori's `tags` come back tripled — investigated in depth, confirmed
  *not* a bug.** `GET details` for a real LNori novel returned its 5
  genres repeated exactly 3 times (15 entries). Rather than assume a
  cheerio-shim or `dom_query` defect, this was root-caused with an
  isolated reproduction: a throwaway Rust binary depending on nothing but
  the exact same `dom_query = "0.28.0"` this codebase already uses, fed the
  real, live-fetched LNori page HTML directly (bypassing `sdk_lnreader`
  entirely). It reproduced the same 15 matches for the plugin's own
  selector (`nav.tags-box.desktop a, nav.tags-box a`) — and walking each
  match's ancestor chain showed **three genuinely distinct, real DOM
  subtrees** matching `nav.tags-box`: one inside the article's `<header>`
  (a breadcrumb-style echo of the genre links) and two more at the same
  depth under `<main>` (a desktop variant and a mobile variant — exactly
  the `tags-box desktop`/`tags-box mobile mob-mq` pair a plain-text grep of
  the page had already found). The page really does list the same 5 genres
  three times; the plugin's own scraper joins whatever the selector
  matches with no de-duplication (`p.join(", ")`, no `Set()`). A real
  cheerio/jQuery engine run against this exact page would match the exact
  same 15 nodes — comma-selector lists don't dedupe by *content*, only by
  *node identity*, and these are three different nodes. **Verdict:
  legitimate site/plugin quirk, faithfully reproduced — not a gap, and
  "fixing" it (e.g. de-duplicating tag text ourselves) would make our
  output diverge from what the real, official LNReader app would show for
  the same source**, which is exactly the outcome this session's guiding
  principle warns against.
- **Ranobes' `description` contains raw CSS text — same reasoning, same
  verdict.** The scraped description opens with a literal
  `.r-desription .cont-text{max-height: 300px; ...}` style block before the
  real prose. Checked `dom_query`'s `format_text` (backing `.text()` in
  `cheerio.rs`'s `native_text`): it has no special-casing for `<style>`/
  `<script>` elements, collecting all descendant text nodes — which is the
  **correct**, spec-matching behavior for `.text()`/`textContent` (browsers
  only *hide* style/script content visually; the DOM text API has always
  included it, and real cheerio/jQuery inherit that same behavior, not a
  browser rendering rule). The plugin's own description selector on this
  particular Ranobes page happens to sweep up an inline `<style>` tag
  alongside the real text. Not an `sdk_lnreader` bug, not a `dom_query`
  deviation from cheerio — the real app would show the same leading CSS
  noise for this same page.

### 6.3 Uninstall, compared precisely — disk, listing, and library state

Not covered by the earlier pass's summary in enough detail to confirm on
its own: uninstall *was* exercised earlier (both `en.asurascans` and
`novelbuddy` were `DELETE`d and confirmed gone from `/installed-sources`),
but only at the API-listing level — disk-level cleanup and library/reading
state were never actually checked. Redone properly this time: a real
Aidoku source (`en.asurascans`) and a real LNReader source (`ranobes`)
were both installed, added to the library, had a chapter downloaded, then
both uninstalled, comparing every layer:

| Layer | Aidoku (`en.asurascans`) | LNReader (`ranobes`) | Verdict |
|---|---|---|---|
| `.aix` file on disk | Removed | Removed | **Parity** |
| `.{stem}.source` sidecar file on disk | **Left behind** (`.en.asurascans.source` still present after uninstall) | **Left behind** (`.ranobes.source` still present after uninstall) | **Parity (shared, pre-existing quirk)** — `SourceManager::uninstall_source` (`backend/shared/src/source_manager.rs`) only ever calls `fs::remove_file` on the `.aix` path itself; it has never touched the sidecar, for either mode. Identical leftover behavior for both, confirmed live, not something LNReader introduced — out of scope to fix here (harmless orphaned JSON, and touching this shared function isn't warranted by anything this session found broken) |
| Downloaded chapter file (`.cbz`/`.epub`) on disk | Left behind | Left behind | **Parity** — uninstall never touches `chapter_storage` for either mode |
| `/installed-sources` | Entry removed | Entry removed | **Parity** |
| `GET /library` | Manga silently disappears from the list | Manga silently disappears from the list | **Parity** — traced to `Database::get_manga_library_with_read_count` (`backend/shared/src/database.rs`), which calls `source_collection.get_by_id(...)?` per row and lets the `?` drop any row whose source is no longer installed; identical, generic, no source-kind branching |
| `GET .../chapters` (cached, DB-only) | Still returns the full cached chapter list, `200` | Still returns the full cached chapter list, `200` | **Parity** — reads only `chapter_informations`, never touches `Source` |
| `POST .../refresh-chapters` (needs a live `Source`) | `404 {"message":"Source was not found"}` | `404 {"message":"Source was not found"}` | **Parity** — same `SourceExtractor` failure path, same generic error, for both |

Every dimension checked comes back identical between the two modes —
uninstall has zero LNReader-specific code at all, and behaves exactly like
Aidoku's own uninstall because it *is* Aidoku's own uninstall path, unchanged.

### 6.4 Sources chosen for rare, specific traits — not the already-validated set

Method, decided and written down *before* any of these were installed or
tested, per the explicit instruction not to pick sources after seeing how
they behave: fetched a fresh `plugins.min.json` (2026-08-05, same session,
confirmed 261 entries) and downloaded all 261 compiled `.js` sources
directly (not sampled), then grepped the real corpus for specific, rare
traits — rather than guessing which sources might be interesting from
their names. Five picked, each for a distinct, stated reason:

| Source | Folder (language) | Why chosen | 
|---|---|---|
| `bakainua` | `ukrainian` (2/261 — one of the rarest folders) | Uses `.siblings()`/`.closest()`, both flagged in §1.2 as cheerio methods none of the 3 originally-validated fixtures exercised; uses `FilterTypes.Switch`, the single rarest filter type in the corpus (7/261 sources, vs. 156 for `Picker`); validates the untested `ukrainian → uk` row of §5.3's folder mapping |
| `komga` | `multi` (1/261 — the *only* entry) | The one real installed test of §5.3's "multi folder gets no `lang`" finding, previously only reasoned about by analogy to Aidoku, never actually installed; uses `.closest()`; its `pluginSettings` includes a `password`-type field and a self-hosted `url` field — a completely different settings shape (credentials + user-provided server) from every other source tested so far |
| `novelki.pl` | `polish` (1/261 — the *only* entry) | Validates the untested `polish → pl` row of the folder mapping on the one plugin that can exercise it |
| `agit.xyz` | `korean` (2/261) | Underrepresented language folder; its `searchNovels` builds a `method:"POST"` request — every other source tested so far (Aidoku and LNReader alike) searches over plain `GET` |
| `kisswood` | `french` (well-represented — chosen for the trait, not the language) | Also uses `.siblings()`; has a 375-chapter catalog (relevant to the already-documented large-catalog `boa_engine` crash risk in `FINDINGS.md` §1) |

**Results — three real, confined `sdk_lnreader` bugs found and fixed,**
all through sources this specific selection method surfaced (none of them
would have been reached by re-testing the already-validated set):

| Step | Aidoku | LNReader | Verdict | Justification |
|---|---|---|---|---|
| Install, `lang` detection | n/a | All 5 installed cleanly; `.aix` inspection confirmed `lang` exactly matching each folder's mapped ISO code: `bakainua→"uk"`, `novelki.pl→"pl"`, `agit.xyz→"ko"`, `kisswood→"fr"`, `komga→null` | **Parity / mapping confirmed** | First real, installed confirmation of the `ukrainian`/`polish`/`korean`/`multi` rows of §5.3's table — previously derived only from folder-name analysis, never exercised end to end |
| Settings mapping: `Switch` filter type | n/a (Aidoku has no equivalent concept) | `bakainua`'s 3 `FilterTypes.Switch` filters (Ukrainian-labelled, e.g. "Новинки") correctly produced 3 `SettingDefinition::Switch` entries via `/setting-definitions`, Cyrillic text intact end to end | **Parity (working as designed)** | Confirms `packaging.rs`'s `filter_to_setting`'s `"Switch" => SettingDefinition::Switch` arm, previously implemented but never exercised against a real live source |
| Settings mapping: fields with no `type` | n/a | `komga`'s own `pluginSettings.password`/`.url` entries have **no `type` key at all** in the compiled plugin (confirmed by reading the raw `.js`) — `packaging.rs`'s `filter_to_setting` requires `obj.get("type")?`, so both are silently skipped | **Fixed (visibility, not behavior)** | The skip itself is correct, deliberate, already-documented behavior (`docs` comment: "unrecognized filter `type`s are skipped, not guessed at") — matches the standalone `lnreader_packager` CLI, which already `eprintln!`s a warning for exactly this. But the **server's on-demand install path silently dropped the same information** (`install_source.rs` only ever read `.bytes` off `package_plugin_js`'s result) — a real user installing from the app, unlike someone using the CLI, had no way to learn a source's settings were incomplete. Fixed with a 12-line, confined addition to `backend/shared/src/usecases/install_source.rs`: log the same warning the CLI already prints. Confirmed live: installing `komga` now logs `[WARN] komga: unrecognized filter/setting type(s), skipped: password, url` |
| Missing JS global: `URL` | n/a | `bakainua.js`'s `parseNovel` calls `new URL(...)` and threw `ReferenceError: URL is not defined` — `sdk_lnreader`'s runtime prelude polyfills `URLSearchParams`, `Response`, `FormData`, but never a plain `URL` constructor | **Fixed** | Confirmed via direct NDJSON request to `lnreader_worker` (bypassing HTTP), isolating the exact error. Grepped the full 261-source corpus: **37 sources (14%) call `new URL(...)`** — a real, corpus-wide gap, not a one-off. Fixed with a minimal `URL` polyfill in `backend/shared/src/source/sdk_lnreader/js_runtime.rs` (protocol/host/pathname/hash parsing, `searchParams` backed by the existing `URLSearchParams`, live `search`/`href` getters) — confined entirely to that one file. Also fixed `URLSearchParams`'s own constructor in the same pass: it only ever supported a plain-object `init` (`for...in`), not a real query-string `init` (`"?foo=bar"`), which the new `URL` polyfill's own `searchParams` needs to parse `match[4]` correctly, and which real code also calls directly (e.g. `new URLSearchParams(location.search)`) |
| Missing cheerio method: `.prev(selector)` | n/a | Past the `URL` fix, `bakainua.js`'s `parseNovel` still failed: `.prev("div.text-2xl")` (used to read a status label from a preceding sibling) doesn't exist on `CheerioSelection` at all — `.next(selector)`, `.nextSibling()`, `.prevSibling()` (the *unfiltered* raw sibling) all exist, but not the selector-filtered previous-sibling form | **Fixed** | `.prev` was already flagged in §1.2 as one of a handful of standard cheerio methods none of the 3 originally-validated fixtures exercised — not a deliberate omission, just never hit until now. Added `CheerioSelection.prototype.prev(selector)` in `js_runtime.rs`, mirroring `.next(selector)`'s exact existing logic (built on the already-native `__native_prev_sibling`) |
| Foundational bug: `.get()` on a chained array doesn't return a plain array | Not applicable — Aidoku has no cheerio-style chaining at all | `kisswood.js`'s `parseChapter` failed with `TypeError: not a callable function` even after the two fixes above. Root-caused with an isolated reproduction against the real, live-fetched chapter page: `n = $(...).contents().map(cheerioCb).get()` correctly returns a plain array of HTML strings — but that array had gone through this module's own `toChain()` helper, whose `.get()` (no-arg form) returned `this` **unchanged**, still carrying every chain override. When the plugin's own next line called `n.map((element, index) => ...)` — genuine, spec-correct native `Array.prototype.map` convention, since `n` is no longer cheerio elements — it silently got `toChain`'s over­ridden `.map`, which uses the *opposite*, cheerio-style `(index, element)` order. The swapped-in "element" parameter was actually a plain number (the index), which has no `.includes()` — hence "not a callable function" | **Fixed — likely the highest-impact bug found this whole session** | `toChain`'s own doc comment already said `.get()` should mirror "real cheerio converting a collection to a plain JS array" — the implementation just didn't do that. Fixed with a one-line change (`return this;` → `return this.slice();`) in `backend/shared/src/source/sdk_lnreader/js_runtime.rs`: `.slice()` is genuine native `Array.prototype.slice` (still present — `toChain` only ever adds properties, never removes native ones), so its result was never itself passed through `toChain` and carries none of the overrides. `$(...).map(cb).get()` followed by plain-array methods is a common real-world pattern (cheerio-chain manipulation, then plain JS on the extracted results) likely to recur across the wider corpus, not unique to `kisswood` |
| `novelki.pl` — live functional test | n/a | Install + `lang` succeeded; every search returned 0 results, no error | **Structural — site issue, root-caused, not fixed** | Direct fetch of the real search page showed a nearly-empty `<div id="app"></div>` shell plus a Cloudflare challenge-injection script (`window.__CF$cv$params`, `/cdn-cgi/challenge-platform/...`) — the site has migrated to a client-rendered SPA behind Cloudflare bot management since this plugin was written. Cheerio-based scraping fundamentally cannot execute the client-side JS that would render real content; the real, official LNReader app would fail identically against this same page for the same reason. Same category as the already-documented NovelUpdates/FreeWebNovel breakage, now with a precisely identified cause rather than "0 results, unclear why" |
| `agit.xyz` — live functional test | n/a | Every search failed with `TypeError: fetch failed for https://agit664.xyz: error sending request` | **Structural — site unreachable, not fixed** | Independently confirmed via a direct `curl` to `https://agit664.xyz/` (outside the app entirely): connection failure, HTTP code `000`. The site is currently down or has moved; not a code issue on either side |
| Regression check after the three fixes | — | Re-ran search on every previously-validated source (`novelbuddy`, `lnori`, `ranobes`) post-fix: identical result counts, zero new errors | **Confirmed safe** | The `.get()` fix in particular touches a foundational helper used by every LNReader source's compiled JS, not just `kisswood`'s — re-checked deliberately rather than assumed safe. Full workspace test suite also re-run clean (157 passed, 0 failed) |

**Net new code from §6.3/§6.4**: one visibility fix
(`install_source.rs`, ~12 lines) and three runtime fixes
(`js_runtime.rs`: a `URL` polyfill, a `URLSearchParams` string-parsing
fix, a `.prev(selector)` method, and the one-line `.get()` fix) — all four
confined to `sdk_lnreader`. Zero lines changed in the Aidoku/WASM path.
§6.3 found no code issues at all (uninstall already behaves identically by
sharing the exact same code path).

**Net diff from the whole §6 pass across both sessions**: two real bugs
fixed in the prior round (`source_of_source`), four more in this round
(the settings-warning visibility gap, `URL`, `.prev()`, and the `.get()`
chain-poisoning bug) — seven fixes total, every one confined to
`backend/shared/src/source/sdk_lnreader/`, zero lines changed anywhere in
the Aidoku/WASM path at any point across this whole audit.

## 7. Écart avec upstream (`tachibana-shin/rakuyomi`)

**Method, and proof this came from a real fetch, not recall.** `git remote
-v` already had `upstream` pointed at
`https://github.com/tachibana-shin/rakuyomi.git` from an earlier session.
Re-ran `git fetch upstream` fresh for this pass (not reused from before):

```
$ date -u +%Y-%m-%dT%H:%M:%SZ
2026-08-05T22:55:55Z
$ git fetch upstream
$ git rev-parse upstream/main
0ef01d0bab2ab90a436f4884fd3192f821d4a996
$ date -u +%Y-%m-%dT%H:%M:%SZ
2026-08-05T22:55:57Z
```

Cross-checked independently, over a completely different network path (the
GitHub REST API rather than git's own protocol), against the same SHA:
`curl -s https://api.github.com/repos/tachibana-shin/rakuyomi/commits/main`
returned `"sha": "0ef01d0bab2ab90a436f4884fd3192f821d4a996"`, same commit
date (`2026-07-31T14:22:00Z`) and message (`chore(release): 1.39.6`) as
`git log` reports locally. Both the CLI fetch and the API call happened
live, minutes before this section was written, with real network access to
github.com — not recited from training data. This SHA is identical to the
one used for the original §7 pass earlier in this same session (upstream's
`main` hadn't moved in the interim), so the diff below is unchanged from
before, now with its provenance made explicit rather than assumed.

Then `git diff upstream/main` against this fork's actual current state —
working tree included, not just the last commit, using `git add -N` on the
two untracked new files (`lnreader_packager/src/plugins_index.rs`,
`sdk_lnreader/packaging.rs`) so they show up as additions instead of being
invisible to `git diff`. `merge-base upstream/main HEAD` is exactly
upstream's current tip (`0ef01d0`), so this is a clean "everything this
fork has added since diverging" diff, not muddied by unrelated upstream
drift in either direction. Total: 34 files, +6976/-187, zero changes
outside `backend/` and three root config files (no `frontend/` diff at
all — confirms the "no new Lua screens/widgets" claim from every phase's
commit messages holds up against upstream too, not just against this
fork's own history).

### 7.1 Confirmed necessary for LNReader support

| File(s) | What changed | Why it's necessary |
|---|---|---|
| `backend/shared/src/source/sdk_lnreader/**`, `backend/lnreader_worker/**`, `backend/lnreader_packager/**` | Entirely new files/crates | The LNReader execution engine, worker subprocess, and packaging pipeline themselves — nothing to compare against, upstream has no equivalent |
| `backend/shared/src/source/mod.rs` | `BlockingSourceKind::{Wasm, LnReader}` dispatch enum, `is_lnreader_archive`, `lnreader_mode_enabled` | The actual integration point: routes every `Source` operation to the right execution mode. `WasmBlockingSource` itself is a pure rename with no behavior change (stated in commit `4761ca5` and re-verified structurally in §6's table — every Aidoku dispatch arm just forwards to the exact same pre-existing method) |
| `backend/shared/src/source_manager.rs` | Skip-gracefully-when-disabled logic in `load_all_sources` | So an installed LNReader source doesn't abort loading every other (Aidoku) source when LNReader support is off — see §2.2 |
| `backend/shared/src/usecases/install_source.rs`, `list_available_sources.rs` | `SourceListItem::LnReaderRaw` untagged variant, LNReader-shape detection | The install/listing bridge documented in full in §5.1 |
| `backend/server/src/source/routes.rs` | Threads `settings.lnreader_enabled` into the install/list-sources handlers | Needed for the two usecases above to see the runtime toggle |
| `backend/shared/src/settings/schema.rs` | `lnreader_enabled: bool` config toggle | The runtime half of the two-gate isolation design (§2.2) |
| `backend/shared/src/source/source_settings.rs` | `SourceSettings::snapshot()` | The out-of-process `lnreader_worker` has no live handle to `SourceManager`, so its whole settings map has to be handed over upfront instead of fetched key-by-key |
| `backend/server/src/main.rs` | `thread_stack_size(16 MiB)` on the blocking runtime | `boa_engine`'s tree-walking interpreter needs more stack than the 2 MiB default for LNReader's full plugin execution. Technically also benefits Aidoku-"next" sources' embedded JS (same `boa_engine`, per the comment in the diff itself) — a shared-file change, but justified by one line with a clear, honest comment, not hidden |
| `backend/shared/Cargo.toml`, `backend/server/Cargo.toml`, `backend/Cargo.toml` | `lnreader` Cargo feature (default-on) on both crates, `boa_gc` optional dep, `lnreader_worker`/`lnreader_packager` workspace members, `reqwest` gains `gzip`/`deflate`/`multipart`/`brotli` | Feature-gating per §2.1; `server`'s `shared = { default-features = false, features = ["all"] }` restructuring specifically so `server` can opt out of `lnreader` while keeping `all` (`cargo build -p server --no-default-features --features use_nix`) — the mechanical detail behind §5.3's `SourceInfo.languages` feature-gating finding. `gzip`/`deflate` are genuinely sent in `sdk_lnreader/js_runtime.rs`'s default `Accept-Encoding` header; `multipart` backs real `fetchApi` multipart bodies in `sdk_lnreader/net.rs` (both confirmed by grep, not assumed); `brotli` is defensive rather than directly requested — see §7.2's retraction note for why that's still necessary, not dead weight |
| `backend/Cargo.lock` | ~20 new transitive entries | Checked line by line: every changed line is a pure addition (`+name = "..."`), **zero existing dependency was bumped, replaced, or removed** — the whole diff is additive, driven by `boa_gc` + the new `reqwest` features + the two new crates themselves |
| `.gitignore` | Two new patterns (`docs/lnreader/`, `sdk_lnreader/test_fixtures/`) | Keeps this fork's own planning docs and locally-vendored test fixtures out of git history — same principle upstream already applies to Aidoku fixtures (`tls.rs`) |
| `AGENTS.md` | Documents the two new modules | Keeps the repo-structure overview accurate |

### 7.2 Should be isolated before any upstream PR

| File(s) / commit | What it is | Why it doesn't belong in an LNReader PR |
|---|---|---|
| `devenv.lock`, `devenv.nix`, `devenv.yaml` (commit `ef60f8e`, "Fix devenv environment for current nixpkgs/Aurora (atomic distro)") | Pins a stale `cargo-debugger` hash, swaps `python313Full` for `python313`, drops a nonexistent `freetype-sys` package | **Zero connection to LNReader.** A personal dev-machine fix (this fork's own maintainer runs Fedora Aurora, an atomic/immutable distro with its own Nix quirks) that happened to land on the same branch lineage. Should ship as its own PR, if upstream even wants it — bundling it with LNReader would make reviewers evaluate an unrelated Nix change alongside the actual feature |
| `backend/shared/src/chapter_storage.rs` (bundled into commit `4761ca5`, not mentioned in that commit's message) | Removes an unused `make_storage()` test helper and its now-dead `size::Size`/`tempfile::tempdir` imports from `#[cfg(test)] mod tests` | Harmless dead-code removal, but it drifted into the LNReader commit silently — the commit message only mentions one unrelated cleanup (an `eprintln!` → `anyhow::Context` swap in `source/mod.rs`), not this one. Should be its own tiny commit, or at least called out in the commit message, before any upstream submission |

**Retracted from this list on review: `reqwest`'s `brotli` feature.** An
earlier version of this section classified `brotli` as "possibly unused,
flag for removal" on the strength of one observation — none of the 5
LNReader call sites grepped requested it via `Accept-Encoding`. That
reasoning doesn't hold up and the row is gone. Two things checked instead
of assumed:

- **Whether decompression is even gated by what `Accept-Encoding` a caller
  sent.** It isn't. `reqwest`'s own doc comments (checked directly in the
  vendored crate source, `async_impl/client.rs`) are explicit: brotli
  auto-decompression triggers "if [the response] headers contain a
  `Content-Encoding` value of `br`" — a check against what the *server*
  actually sent back, completely independent of what the client asked for.
  `sdk_lnreader/js_runtime.rs` manually sets `Accept-Encoding: gzip,
  deflate` (overriding `reqwest`'s own default header), but that only
  changes what's *requested* — it doesn't stop `reqwest` from correctly
  decompressing a response that arrives brotli-encoded anyway, which real
  CDNs and anti-bot layers (exactly the kind of infrastructure sitting in
  front of several corpus sites already documented as fragile — FreeWebNovel's
  403, this session's Cloudflare-fronted `novelki.pl` in §6.3) are known to
  do regardless of the client's stated preference. Without the `brotli`
  feature compiled in, that specific real-world scenario would silently
  hand a plugin raw compressed bytes instead of text.
- **Whether a specific Phase 2 "gzip/brotli decompression fix" commit
  exists, as raised when reviewing this section.** Searched thoroughly —
  `git log --all -S"brotli"`/`-S"gzip"`/`-S"decompress"` across the entire
  history, every string in every currently-existing `docs/lnreader/*.md`
  file, and every commit message — and found no such fix, and no "six real
  bugs" list, anywhere in this repository or its history. What *does* exist
  is commit `4761ca5` (Phase 2) adding `brotli`/`gzip`/`deflate`/`multipart`
  to `reqwest` all together, upfront, in the same `Cargo.toml` hunk that
  stood up the whole LNReader networking stack — not a targeted bug-fix
  commit. Either this refers to something outside this repository (a
  separate note, a different working copy) or the recollection doesn't
  match what's actually here; reporting that plainly rather than
  constructing a matching story.

Net effect: `brotli` cannot be confirmed unused by the code's own logic
(quite the opposite — the mechanism it exists for is real and
response-driven, not request-driven), so per the standing rule for this
section, it stays out of the "isolate" list. It remains necessary,
defensive infrastructure for the wider ~274-source corpus this session's
5-source sample doesn't cover, not a candidate for trimming.

**Nothing found in the "superfluous/forgotten" category this pass** — no
stray debug file, no unresolved `TODO`/`FIXME`/`XXX` marker, no leftover
`println!`/`dbg!` outside `lnreader_packager`'s own CLI output (checked by
grepping every added line across the full diff). Nothing here is being
removed or changed as part of this audit — per the brief, these three
items are listed for a future, explicit decision, not acted on now.
