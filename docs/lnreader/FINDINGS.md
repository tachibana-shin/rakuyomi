# Findings — investigation results and decisions (merged, Phase 3.5)

Merged from four separate investigation reports (2 Aug 2026). Organized by
topic rather than by research session — each session's process narrative is
compressed to what was found and decided; the full blow-by-blow (which
debugger commands were run, which hypotheses were tested and rejected in what
order) is not reproduced here. If you need that level of detail for a
specific still-open question, it existed in `BOA_GC_BOUNDARY_FINDINGS.md`/
`BOA_GC_ROOT_CAUSE_FINDINGS.md` before this merge — gone now per the Phase 3.5
cleanup decision, but nothing load-bearing was cut; every decision and its
justification is below.

---

## 1. The `boa_engine` crash/hang investigation

### 1.1 What's confirmed, current state

Two distinct problems were in play, and three sessions of investigation
eventually separated them:

- **Original SIGSEGV / invalid memory read** (Phase 2, large catalog —
  22-volume series): reproduced once via `gdb` during Phase 2, **never
  reproduced again since**, in this or later sessions. **Still not
  root-caused.** Contained today only by process isolation (`lnreader_worker`,
  §1.3) — a crash there takes down one worker, not the whole backend, and it
  respawns transparently on the next call.
- **A hang** (worker stops responding, never crashes): reproduced reliably
  across two later sessions, always against LNori (the one validated source
  whose `parseNovel()` fans out over `Promise.all` — one HTTP request per
  "volume" found, ~22-28 requests in a single call), always around the 5th
  repeated `get_chapter_list()` call on the same large novel via the same
  persistent worker. **Root cause found, with a high but not absolute
  confidence level**: a live `gdb` attach during the hang showed the worker's
  thread blocked entirely inside `reqwest`/`hyper`/`tower` (`oneshot::poll_recv`
  waiting for an HTTP response that never arrives), with Tokio's own runtime
  (I/O driver + thread pool) completely healthy — **no trace of
  `boa_engine`/`boa_gc` anywhere in the blocked call stack**. The leading
  hypothesis is a stale keep-alive connection reused from the worker's single,
  long-lived `reqwest::Client` connection pool, silently dead on the server
  side (no clean FIN/RST) — explaining both why only the multi-request-per-call
  source (LNori) is exposed, and why `NovelBuddy` (a single fetch per call)
  never hangs even on a 10x larger catalog (Shadow Slave, 3136 chapters,
  8/8 repeated calls, no degradation). **Not directly confirmed** (no isolated
  test of the HTTP client alone against a simulated dead connection was run)
  — a plausible, evidence-consistent hypothesis, not a certainty.
- **Ranobes** showed a third, unrelated behavior during the same testing: a
  fast, clean HTTP 429 (rate limiting) on repeated automated requests —
  confirms these sites do react to scripted traffic, but with an ordinary
  catchable rejection, not a hang.

### 1.2 Decision: connection pool tuning (Option G) — not applied, either side

Bounding `pool_idle_timeout`/`pool_max_idle_per_host` on the LNReader worker's
HTTP client was considered (cheap, plausible fix for the hang hypothesis
above). **Verified before deciding**: LNReader's and Aidoku's HTTP clients
share the exact same base (`crate::tls::client_builder()`) with no pool
overrides on either side — applying Option G only to LNReader would have
*created* a new asymmetry, not closed one. **Decision: don't apply it
unilaterally.** If the stale-connection hypothesis is ever confirmed (e.g. via
an isolated `reqwest` test against a simulated dead connection — never done),
the fix belongs in the shared `client_builder()`, not in `sdk_lnreader::net`
alone.

### 1.3 Decision: worker isolation stays; the real reason it exists

`lnreader_worker` (a dedicated subprocess per source, see `REFERENCE.md` §3.1)
is the actual containment for both problems above — a crash or unresponsive
worker is detected (dead pipe, or `WORKER_READ_TIMEOUT` = 120s) and respawned
transparently, at the cost of one failed call. This was investigated
specifically to check whether Aidoku/WASM sources have — and lack — the same
protection: they run **in-process**, no subprocess isolation at all. **This is
the one significant asymmetry found in this whole investigation, and it goes
in LNReader's favor, not the reverse** — a WASM crash takes down the entire
backend, LNReader's doesn't. Nothing to "catch up on" here; if anything, a
similar isolation for Aidoku sources would be the interesting follow-up, but
that's out of scope for LNReader work.

### 1.4 Decision: native worker thread stack size — 64 MiB, kept as-is

Measured directly (a disposable calibration harness, fully removed after
measurement — nothing committed from it): native stack cost is ~1616
bytes/level of HTML nesting depth during `htmlparser2::walk()` (each level
also crosses several `boa_engine` call frames, not just one `walk()` frame).
64 MiB covers ~40,400 nesting levels. Real-world nesting measured on an actual
scraped page (LNori) is 87 levels — three orders of magnitude below the
protected threshold.

**Decision**: keep 64 MiB, targeting 20,000 nesting levels as the "realistic
worst case" (not the 50,000-level stress case used to measure the per-level
rate) — giving only ~2x margin at that target, well under the usual 4-8x rule
of thumb. Accepted anyway because a breach fails as a clean `SIGABRT` (Rust's
own stack guard-page detection), already contained by the worker
crash/respawn mechanism — not a silent-corruption risk. Revisit if real-device
testing (Phase 5) ever shows deeper nesting, or if the per-level rate turns
out higher on ARM.

### 1.5 Inventory: resource-limiting mechanisms available vs. actually used

Checked whether Aidoku/`wasmi` has protections LNReader/`boa_engine` lacks —
the honest answer turned out to be "mostly neither side uses what's
available," not "Aidoku is ahead":

| Protection | Aidoku | LNReader | Verdict |
|---|---|---|---|
| Fuel/instruction budget | Not enabled (`wasmi` has `consume_fuel`, unused) | Not available (`boa_engine` has no equivalent) | Capability asymmetry, not an activation gap |
| Loop iteration limit (JS only) | N/A | Not enabled (`boa_engine::RuntimeLimits::set_loop_iteration_limit` exists, default unlimited) | Recommended, cheap, not applied — see below |
| Max HTTP response size | Not enforced | Not enforced | Identical, shared gap |
| Redirect limit | `reqwest` default (10) | `reqwest` default (10) | Identical |
| Retry/backoff | `reqwest` default | `reqwest` default | Identical |
| Instance memory limit | Not enabled (`wasmi::StoreLimitsBuilder` exists, unused) | Not available (`boa_gc` has no heap cap at all) | Capability asymmetry |
| Process isolation | **No** (in-process) | **Yes** (`lnreader_worker`) | LNReader ahead, see §1.3 |
| HTTP `read_timeout()` | Not configured | Not configured | Identical, shared gap |

**Decision (both `loop_iteration_limit` and `read_timeout()`)**: documented as
available, cheap options — **not applied**. Neither has evidence of being
needed against a real source yet (only a synthetic `while(true){}` test
exercises the loop-limit case; the hang the `read_timeout` would target is
itself only a hypothesis, §1.1). Re-evaluate explicitly during Phase 5
real-device testing rather than adding protection against an unobserved
scenario.

---

## 2. Efficiency findings (Aidoku vs. LNReader, normal usage — not abuse protection)

### 2.1 `parseNovel()` double-call — real LNReader-specific gap, fixed

`get_manga_details` and `get_chapter_list` both called `parseNovel()`
independently — for a source whose `parseNovel()` fans out over the network
per volume (LNori), opening a novel doubled the HTTP/CPU work for one logical
action. **Fixed** (commit `4d89737`): a short-lived (10s), single-use cache on
`JsRuntime` keyed by `manga_id`, holding the *converted* `Manga`/`Vec<Chapter>`
result (never the raw `JsValue` — a `JsValue` can't safely be held across
worker calls). Verified against real LNori: `get_manga_details` 24.4s (cold) →
`get_chapter_list` 20ms (cached) → next `get_manga_details` after TTL expiry
23.0s (cold again, as expected).

### 2.2 Settings-write amplification — shared gap, not LNReader-specific

Writing one `storage.set()` key triggers `SourceManager::update_source_setting`
→ `load_all_sources`, which reloads **every** installed source (re-opens each
`.aix`, rebuilds a fresh `wasmi::Store`/`Instance` for WASM sources or, for
LNReader, at least re-snapshots settings). Multiple keys written in one call
(`storage.set()` called N times) trigger N full reloads of the whole catalog.
**Confirmed identical on both sides** — Aidoku's `wasm_imports/next/defaults.rs`
goes through the exact same `SourceSettings::save` → `update_source_setting`
path. Not fixed (would need broader work: batching the write loop, and
separately, only reloading the one changed source instead of the whole
catalog) — noted as the most direct, lowest-risk place to fix for LNReader
specifically without touching the shared Aidoku path, if ever prioritized.

### 2.3 HTML-parse memory cleanup — LNReader is ahead, not behind

The initial hypothesis ("Aidoku's mature `dom_query` usage is probably more
disciplined than LNReader's newer cheerio shim") **did not hold**. LNReader's
`cheerio::Store::clear()` runs unconditionally after every top-level plugin
call — the actual fix for the historical `Box::leak` crash. Aidoku's
equivalent (`WasmStore.htmls`) only frees via `free_reference_html`, itself
only invoked when the WASM guest code explicitly calls `std.destroy` — no
unconditional sweep between calls at all. **Nothing to adopt from Aidoku
here**; if anything, an unconditional sweep mirroring `Store::clear()` would
be a real improvement for the WASM side, but retrofitting it is riskier there
(some legitimate Aidoku call sequences keep values alive across calls via
`std_descriptors`) — not pursued, no evidence of a real incident on that side
to date.

### 2.4 Download streaming — shared gap, novel path slightly worse

Neither path streams. The image/CBZ path buffers per-page (bounded by
`concurrent_requests_pages × 2`, with an already-acknowledged `// TODO` in the
code about it). The novel/EPUB path is worse: `download_all_images` loads
**every** image referenced across **every** page of a chapter into one
`HashMap` before starting EPUB assembly — more buffering than the image path,
not less. Not LNReader-introduced (`chapter_downloader.rs` is generic,
predates LNReader support) — a generic download-pipeline hardening item if
ever prioritized, not specific to this mode.

---

## 3. HTML parse O(n²) complexity

Found while calibrating the stack size (§1.4): `Document::fragment()` +
`htmlparser2::walk()` doesn't scale linearly with HTML nesting depth (0.45s at
1,000 levels → 71.5s at 20,000 levels). Localized precisely: the cost is
**entirely in `Document::fragment()`** (i.e. `html5ever`'s HTML5 parsing
itself) — confirmed by isolating `walk()` alone (linear, negligible: 22ms→74ms
over the same range) and by testing flat siblings vs. nested elements at the
same element count (33x faster flat). Not `dom_query`'s tree operations
(confirmed O(1) per op by reading `dom_tree/tree.rs`), not the JS binding, not
a bug specific to this project. The exact line inside `html5ever` responsible
was not pinned down (would need a profiler, unavailable in this environment,
or a full tokenizer state-machine review) — confirmed to be **depth**-driven,
not element-count-driven, which was enough to assess the real risk.

**Real-world risk measured negligible**: a real LNori chapter page peaks at 87
nesting levels; extrapolated cost at that depth is ~1.4ms. The threshold where
this becomes perceptible (>100ms) is ~730 levels — nowhere near what a real,
even badly-coded, site produces.

**Decision: accepted as-is, no code change** (Option "do nothing" chosen over
a depth guard-rail before parsing). Justification: the existing
`WORKER_READ_TIMEOUT` (120s) already catches the pathological extreme (a
document north of ~25,000-30,000 levels would blow the parse time budget
alone, well before any stack concern from §1.4 could even be reached) and gets
treated as an ordinary hang with transparent respawn. No evidence in any
tested source of nesting approaching the regime where this would matter.
Revisit only if a real site is ever encountered with nesting far beyond what's
been measured.

---

## 4. Aidoku-vs-LNReader comparison, overall takeaway

Every session in this investigation went in expecting to find Aidoku (the
older, more mature path) ahead of LNReader on some axis, and to bring
LNReader up to parity. That expectation mostly didn't hold: most protections
inventoried are either **identical on both sides** (HTTP pool config,
redirect/retry policy, response size limits, read timeout — none configured
either way) or **capability asymmetries** that can't be fixed by "just
enabling a flag" (`boa_engine` genuinely has no fuel/heap-limit equivalent to
`wasmi`'s). The one real, activated asymmetry found (process isolation) already
favors LNReader. The one genuine LNReader-specific inefficiency found
(`parseNovel()` double-call) was fixed. Keep this pattern in mind before
assuming "Aidoku already solved this" in a future comparison — verify first.
