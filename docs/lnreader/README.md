# `docs/lnreader/` — index

This directory accumulated across many sessions (feasibility research, Phase 2
implementation, several follow-up investigations into a `boa_engine` crash, Phase 3
packaging, Phase 3.5 cleanup and a same-phase follow-up). This index exists so a new
session doesn't have to read everything to find its footing.

**Not committed to git** (`docs/.gitignore` excludes this whole directory) — these are
working notes for Claude Code sessions, not shipped documentation.

**Cleaned up in Phase 3.5, two passes** (`REFERENCE.md` §4 has the full detail):
1. Removed handoffs whose instructions were already fully carried out and superseded —
   `PHASE2_HANDOFF{,_FINAL}.md`, `PHASE3_HANDOFF.md`, `REVALIDATION_HANDOFF.md`, the two
   `boa_engine`-investigation handoffs, the two efficiency/complexity-investigation
   handoffs, `PHASE3_5_HANDOFF.md` itself, and the pre-integration standalone PoC
   (`poc-reference-{main.rs,Cargo.toml}`).
2. Merged the remaining cross-cutting knowledge documents (which stayed as separate
   per-research-session files even after pass 1) into three topic references —
   `FEASIBILITY.md`, `FINDINGS.md`, `ENV_SETUP.md` — while leaving phase handoffs as
   one file per phase, deliberately not merged (those stay specific to a phase, by
   design).

If you're looking for a document by a name that isn't listed below, it's gone for one
of the reasons above — its content lives in whichever of the four files below absorbed
it, not lost.

## The four files in this directory, and when to read each

- **`REFERENCE.md`** — **read this one first.** The day-to-day reference for the
  LNReader mode as it stands right now: the dead-code audit conclusion (§1), the
  `lnreader` Cargo feature + `lnreader_enabled` config toggle that isolate the whole
  mode — including a **deliberate, documented default-flip you should not "fix"**
  (§2.2) — a file-by-file reference of what's active in `sdk_lnreader`/
  `lnreader_packager` (§3), this cleanup's own history (§4), and LNReader's upstream
  plugin discovery index (`plugins.min.json`) with a real-world validation result (§5).
- **`FEASIBILITY.md`** — *why* the architecture is shaped the way it is: four
  approaches evaluated (new project / adapt Rakuyomi directly — the one built / an
  automated JS→Rust converter / a prior-art survey), each condensed to its conclusion
  and the commit that implemented it, exploratory reasoning stripped once the decision
  was made. Read this when you need to understand a design choice's rationale, not
  what's currently built (that's `REFERENCE.md`).
- **`FINDINGS.md`** — investigation results: the multi-session `boa_engine`
  crash/hang investigation (what's confirmed, what's still open, every
  resource-limiting decision made and why), Aidoku-vs-LNReader efficiency comparisons,
  and the HTML-parse O(n²) complexity investigation. Organized by topic, not by
  research session.
- **`ENV_SETUP.md`** — generic, project-agnostic guide for setting up a Rust/WASM dev
  environment (Nix/devenv/direnv) on Aurora, Bazzite, or any other atomic/immutable
  Fedora-based (uBlue-family) distro. Useful even outside this project.
- **`PHASE4_HANDOFF.md`** — the next phase to actually resume (UI-facing
  `XCheckboxGroup` include/exclude mapping + end-to-end device testing). Not yet
  executed — read it fresh, it's the one real to-do list left in this directory.

## Things easy to lose track of — check `REFERENCE.md` for the full context on each

- **`lnreader_enabled` currently defaults to `true`**, reversing the original Phase 3.5
  spec ("off by default"). This is deliberate — LNReader is in an active real-world
  testing period — not an oversight. See `REFERENCE.md` §2.2 before changing it back.
- **The `boa_engine`/`boa_gc` original SIGSEGV crash signature (Phase 2) is still not
  root-caused** — only contained by worker process isolation + respawn. See
  `FINDINGS.md` §1.1.
- **Worker native stack size (64 MiB) has only ~2x margin at its target**, not the
  usual 4-8x — accepted because a breach fails as a clean `SIGABRT`, not corruption.
  Revisit only with real-device evidence. See `FINDINGS.md` §1.4.
- **LNReader discovery now works exactly like Aidoku's, end to end** — a
  `plugins.min.json` URL goes into the same `settings.json` `source_lists`
  array as any Aidoku index URL; the server tells the two shapes apart from
  the JSON itself and packages an LNReader plugin on demand at install time
  (no separate CLI step required for a user). `lnreader_packager fetch`
  still exists as an optional offline pre-packaging/bulk-validation tool.
  See `REFERENCE.md` §5.1.
- **The 256/259 language-tagging gap found during validation is fixed** —
  packaging now falls back to a language derived from the plugin's own
  index URL folder (`.../src/plugins/<folder>/...`) when the plugin itself
  doesn't declare a `lang`. Note this by itself doesn't change any UI/
  filtering behavior yet — nothing in the app currently filters the source
  list by language (only per-chapter `lang` is used today). See
  `REFERENCE.md` §5.3, including the correction to what an earlier version
  of this doc claimed about `Settings.languages`.
