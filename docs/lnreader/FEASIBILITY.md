# Feasibility — decisions and why (merged, Phase 3.5)

Merged from four separate feasibility reports written 29–30 July 2026, before
and during early implementation. **Exploratory reasoning that only mattered
before the decision was made has been cut** — this document keeps the
conclusion reached, why it beat the alternatives, and the commit that
actually implemented it. For "what's built and how it works today," see
`REFERENCE.md` instead — this document is about *why* the architecture is
shaped the way it is, not a description of its current state (which drifts;
this doesn't).

Read order if you want the full original write-ups instead of this summary:
they're gone — merged and superseded by this file, per the Phase 3.5 cleanup
decision. What's below is the complete distilled content, not a teaser.

---

## The question: how do you run ~274 third-party JS scraper plugins (LNReader/
`lnreader-plugins`) inside a KOReader plugin (Rakuyomi) written in Rust + Lua?

Four options were evaluated, in this order, over two days:

## Option 1 — New standalone project, forked from Rakuyomi's design (29 Jul)

**Verdict: feasible, but not chosen.** Rakuyomi already has, in its own
codebase, nearly every low-level piece a fresh LNReader-focused project would
need to build from scratch: an embedded JS engine (`boa_engine`, already used
for Aidoku-"next" sources' small anti-bot scripts), EPUB generation
(`epub_builder`, `chapter_downloader.rs` already distinguishes a "novel
chapter" from a "manga chapter" by `page.text.is_some()`), SQLite-backed
library management, a proven Lua↔Rust IPC pattern, and e-reader
cross-compilation already solved. A key advantage over the manga case: KOReader
reads EPUB natively (CREngine) — no custom reader UI needed, unlike the manga
side which needed `MangaReader.lua`/`CbzDocument.lua`.

Real technical core identified: don't port cheerio (JS) — Rakuyomi already has
a complete DOM/CSS-selector engine in Rust (`wasm_imports/html.rs`, backed by
`dom_query`) built for Aidoku sources. Of the cheerio methods actually used
across the LNReader corpus (`.text()` 1185 occurrences, `.find()` 694,
`.attr()` 613, `.each()` 279, ...), most already have a direct Rust
equivalent — the real work is a **binding** (a JS-side chainable API that
delegates to the existing native engine), not a **port** (reimplementing an
HTML parser + CSS selector engine in JS).

**Not chosen because**: Option 2 (below), found the same day, gets the same
technical core with far less new code, by extending Rakuyomi's own
already-polymorphic `Source` abstraction instead of rebuilding library/UI/DB
management from scratch in a new project.

**Fallback noted, never built**: a small self-hosted Node/Deno service running
the real, unmodified plugins, with the KOReader plugin as a thin HTTP client —
kept as an escape hatch if the embedded-JS-engine approach proved too costly
to maintain solo. Never needed.

## Option 2 — Adapt Rakuyomi directly, third `Source` execution mode (29 Jul, updated 1 Aug) — **the option built**

**Key discovery that made this the obvious choice**: `Source` in
`backend/shared/src/source/mod.rs` was already polymorphic — it picks between
Aidoku "legacy" and Aidoku "next" WASM ABIs at install time, with an automatic
fallback if the guessed mode fails. Adding a **third** strategy (LNReader/JS)
extends an existing, already-proven pattern rather than grafting on something
foreign. The 13 functions a `Source` must expose
(`get_manga_list`/`search_mangas`/`get_manga_details`/`get_chapter_list`/
`get_page_list`/`get_image_request`/`process_page_image` + 6 `_next` variants)
were already the well-bounded extension point.

**Mapping decided** (LNReader plugin method → `Source` function):

| `Source` function | LNReader method | Notes |
|---|---|---|
| `search_mangas(query, page)` | `searchNovels(searchTerm, pageNo)` | direct |
| `get_manga_list(listing)` | delegates to `search_mangas("", 1)` | Rakuyomi has no browse/popular UI at all — verified by listing every function `Backend.lua` exposes, confirmed for every source kind, not LNReader-specific |
| `get_manga_details(manga_id)` | `parseNovel(novelPath)` | maps `SourceNovel`'s meta fields directly |
| `get_chapter_list(manga_id)` | `parseNovel(novelPath).chapters` | same call as above, reuses the result |
| `get_page_list(manga_id, chapter_id)` | `parseChapter(chapterPath)` | returns exactly **one** `Page` with `.text` set — "1 LNReader chapter = 1 Rakuyomi Page," not the coarser "1 chapter = 1 volume" pattern the reference Aidoku "Light Novel" source (`vi.hakovn`) uses. Chosen because it keeps per-chapter read progress/download granularity instead of collapsing it to per-volume, and the code needed is *simpler* than `vi.hakovn`'s, not more complex |
| settings (`SettingDefinition`) | `filters`/`pluginSettings` | near 1:1 (Switch/Select/CheckboxGroup/Text ↔ Switch/Picker/CheckboxGroup/TextInput) |

**Architecture actually built** (differs from the original one-line-sketch in
a good way): `BlockingSource` was wrapped in an enum
(`BlockingSourceKind::{Wasm, LnReader}`) rather than adding a branch inside the
existing WASM code — the pre-existing struct was renamed `WasmBlockingSource`,
body untouched. This kept every WASM/Aidoku code path completely unchanged
(confirmed by grep: `BlockingSource` had ~37 call sites, all now one-line
dispatches).

**Non-negotiable principles set during this phase, still in force**:
- All-native-Rust: no real JS library (`dayjs`, `htmlparser2`, `lodash-es`)
  ever gets bundled and interpreted — `boa_engine` has no JIT, so interpreted
  JS is too slow for anything touching every page/date. Native Rust
  (`chrono`, a `dom_query`-backed replay) does the real work; only the
  JS-visible *shape* of these libraries is polyfilled.
- No source ever hardcoded in the backend — `source_lists` (`Vec<Url>`) is
  the only legitimate discovery mechanism, same as Aidoku.
- No new Lua UI/widget, ever — LNReader sources use the exact same
  library/search/settings/chapter-list screens as Aidoku manga sources.
- Rakuyomi's own functionality bar applies ("same as what the project already
  does for manga, no more"), not the theoretical ceiling of either SDK.

**Implemented in**: Phase 2 (commit `4761ca5d` — `BlockingSourceKind::LnReader`,
end-to-end against real sources) → Phase 3 (commit `9400a1d` —
`lnreader_packager` packaging pipeline, source-list index generation) → Phase
3.5 (this branch, uncommitted at merge time — Cargo feature flag +
`lnreader_enabled` config toggle, dead-code audit, upstream plugin index
support; see `REFERENCE.md`).

## Option 3 — Automated JS→Rust/WASM converter (29 Jul)

**Verdict: feasible, structurally cheaper than it first looks, but not
chosen for regular use.** The ~274 sources aren't a flat pile — they split
into three tiers: ~153 "multisrc" sources (12 shared JS templates, config-only
per site — and Aidoku *already has* equivalent native templates for the one
template that overlaps, Madara, so 73 sites become pure config mapping);
~15 bespoke sources with no cheerio at all (pure JSON API, direct field
mapping); and ~106 bespoke cheerio sources (the genuinely hard tier, where CSS
selectors themselves port near-verbatim between cheerio and `dom_query`, but
surrounding control flow/idioms need real translation, realistically
AI-assisted with a mandatory compile-and-verify-against-the-real-site loop).

**Structural advantage over Options 1/2**: zero changes to Rakuyomi/KOReader —
output is a standard `.aix`, installed exactly like any Aidoku source. Real
disadvantages: update latency (a converted source only catches up to an
upstream site change after a generate+verify cycle, vs. instantly for
Options 1/2 which just re-read the upstream `.js`), and a much bigger
per-source-family engineering investment for the ~106-source cheerio tier.

**Decision recorded 29 July, still in force**: no automated pipeline. Handle
sources one at a time, on demand, classified into the tier above only when a
specific source is actually wanted — the tiering logic stays useful as manual
guidance even without a batch pipeline behind it. Not revisited since; Option
2's embedded-JS-runtime approach became the actual path taken instead, making
this comparison moot in practice (Option 2 already gives every source, not
just the ones worth hand-converting).

## Option 4 (implicit) — Prior-art survey confirming the direction (30 Jul)

A systematic review of `wotaku.wiki`'s software/extension listings found **at
least six independent projects** that already solved some version of this
problem: Mangayomi (Dart + QuickJS + a hand-written cheerio shim, the closest
direct analog — its `js_cheerio.dart` was kept as the most transposable
reference for the Rust/Boa binding), IReader (embedded JS + a
metadata-only/scaffold-only JS→Kotlin generator, confirming the "mechanical
conversion only goes so far, scraping logic stays manual" finding from Option
3), Shosetsu and NoveLA (independently converged on "Lua + native
jsoup/DOM binding" — validates that exposing a native DOM engine directly to
a lightweight script host, exactly Option 2's shape, is an established
pattern, not a Rakuyomi-specific oddity), Tsundoku (a compatibility bridge
running Shosetsu/IReader sources inside a different host — confirms
cross-runtime source-compatibility bridges are the norm across this whole
class of app, not unique to Rakuyomi's legacy/next/LNReader trifurcation),
and Legado (a declarative CSS/XPath/JSONPath rule format with a `<js>...</js>`
escape hatch for the genuinely hard cases).

**Outcome**: confirmed Option 2 as the right default, and surfaced a
**future optimization idea, not pursued**: a declarative rule format (à la
Legado) for the sources that don't actually need real programming logic
(most of Option 3's tiers A/A'/B), reserving `boa_engine` + the full cheerio
shim only for genuinely complex bespoke sources. Explicitly deferred — "not
prioritized now, a simplification path to keep in mind if the full-JS shim's
maintenance cost ever becomes a real problem for the simple sources," per the
original report. No code exists for this; it would be a new, separate
execution mode if ever pursued.

---

## Net result

Options 1, 3, and the Option-4 declarative-rules idea were all seriously
evaluated and are still valid fallback paths if Option 2 (the one built) ever
turns out not to scale — they're recorded here specifically so a future
session facing a real problem with the current approach doesn't have to
re-derive them from scratch. But as of Phase 3.5, only Option 2 has any code
behind it, and it's the one to keep extending.
