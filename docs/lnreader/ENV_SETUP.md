# Rust/WASM dev environment on atomic/immutable Fedora-based distros (Aurora, Bazzite, other uBlue spins)

Generic, reusable guide — not tied to any specific project or session. Written
after resolving ~15 distinct problems getting a Nix/devenv-based Rust project
running on Aurora Linux from a completely clean machine. If you're setting up
`devenv`/`direnv` + Rust (optionally cross-compiling to WASM or an embedded
target) on Aurora, Bazzite, Bluefin, or any other rpm-ostree/uBlue-family
atomic distro, this should save you rediscovering the same issues.

**What makes atomic distros different here**: they have a **read-only root
filesystem** (rpm-ostree-based, sometimes with `composefs` layered on top).
Most Linux dev tooling — including Nix's own official installer — silently
assumes a writable `/`. That single fact is the source of nearly every
distro-specific problem below; everything else is ordinary Nix/devenv version
management that would bite on any distro.

---

## 1. Installing Nix itself on an atomic distro

This is the step most project READMEs skip — they assume Nix is already
present and jump straight to `devenv`/`direnv` setup. On an atomic distro,
getting Nix installed at all is the first real obstacle.

- **The classic Nix installer (`sh <(curl -L https://nixos.org/nix/install)`)
  will not work as-is** — it assumes a writable `/nix`, which a read-only root
  doesn't allow without extra steps.
- Use the **multi-user, atomic-distro-aware install path** instead (the
  Determinate Systems installer, or the official installer's
  `--daemon`/atomic-specific flags, depending on what's current when you read
  this — check for an explicit "immutable"/"atomic"/"ostree" mention in
  whatever installer docs you're following, don't assume the default path
  covers it).
- **`composefs`** (a relatively recent addition to Fedora Atomic's image
  format) can block even the atomic-aware installer. If installation fails in
  a way that points at the root filesystem or image composition, check
  whether `composefs` is enabled (`rpm-ostree status` / your distro's
  equivalent) and whether the installer version you're using has a known fix
  or requires an `ostree`-level config adjustment. This has historically
  required a config change plus a reboot — don't assume a single retry will
  fix it if the first attempt fails for this reason.
- Reboot after installation before trusting anything — atomic-distro package/
  filesystem changes often only take effect after the next boot layers them
  in.

## 2. devenv + direnv setup (the part most project docs *do* cover)

Once Nix itself works, the actual project setup is usually standard and
already documented by the project you're working on:

```sh
# inside the project directory
cp .envrc.dist .envrc   # or whatever the project's template is called
direnv allow
dev                     # or whatever the project's own validation command is
```

If this fails, don't assume the failure is atomic-distro-specific — read the
actual error first. Most failures past this point are **ordinary Nix version
skew**, covered in §3, which would happen on any Linux distro, atomic or not.

**Don't assume a project's own setup guide covers every dependency it
pulls in.** If a `devenv.nix` builds something substantial as a package
dependency (a whole other application, a large native library), and you hit a
problem specific to *that* dependency rather than to the project's own code,
check that dependency's own build/dev documentation directly rather than
assuming the parent project's guide has already accounted for it. Trusting
that "if devenv built it, it must be fully working" without ever cross-checking
against the dependency's own docs is a real gap, not a safe assumption —
name it explicitly if you skip that check, rather than silently assuming it
away.

## 3. The one root cause behind most Nix/devenv errors: lockfile-vs-tooling skew

**If you hit a cascade of seemingly unrelated Nix errors, check this first
before debugging each one individually.** `devenv.lock` pins the exact
revisions of `devenv` itself, `nixpkgs`, and any other flake inputs a project
uses, as of whenever that lock file was last updated. Nix's reproducibility
guarantee only holds **if you stay strictly within those pinned versions**.
The moment you install "whatever's newest today" for `devenv`/`nix`/etc.
against a lock file that's a year old (or even a few months), you get a
long tail of small, individually-confusing incompatibilities:

- A `devenv`/Nix module option that changed name or shape between the pinned
  revision and today (online docs describe the *current* version, not the one
  actually pinned — this is a generic trap, not specific to any one option).
- A flake input (`git-hooks`, or any other convention introduced by `devenv`
  after the lock was made) simply missing from an old lock file.
- Updating **one** input in isolation (just `devenv`, or just one dependency)
  desyncing it from everything else that's supposed to move together —
  Nix flake inputs are often meant to advance as a set, not individually.
- A separate `nixpkgs-unstable` channel input (common, deliberate pattern:
  pin the bulk of a project on a stable/rolling channel, but pull one
  specific package from `unstable` when a newer version of *that one thing*
  is needed) occasionally having a broken/failing package on a given day —
  an `unstable`-channel timing accident, not a sign the architecture is wrong.
- A dependency pinned to a **floating git ref** (`rev = "master"` or similar,
  rather than a fixed commit) inside the project's own Nix expressions —
  a known anti-pattern that eventually breaks reproducibility for anyone who
  builds it after the referenced branch moves. If you find one while
  debugging, it's worth flagging even if fixing it isn't your job right now.
- A crate/package name mismatch: a Rust dependency name (Cargo crate) is not
  automatically a `nixpkgs` system package name — if a `devenv.nix` lists
  something that looks like a Cargo crate name in its system-packages list,
  double check it's actually meant to be a `nixpkgs` package and not a
  leftover/mistaken entry.

**Practical approach that actually resolves this class of problem**: don't
chase each symptom as an independent bug. Once you've confirmed the pattern
is "old lock, new tooling," the fix is almost always some form of "bring the
whole set back into alignment" — either find/install a version of your
top-level tool (`devenv`, etc.) that's contemporary with the lock file, or run
the project's full update mechanism (e.g. `devenv update`) to advance
everything together, rather than patching inputs one at a time. Isolating
variables one at a time (test a suspect option alone, then in combination) is
still the right debugging technique *within* this process — it just shouldn't
be your strategy for the update itself once you've recognized the root cause.

## 4. Environment-unrelated issues you may hit along the way (don't over-attribute them to Nix)

Not everything that goes wrong during this kind of setup is Nix's fault —
some of what surfaces is:
- Desktop/display-server issues (e.g. an SDL2/Wayland problem under KDE) —
  unrelated to Nix, don't spend Nix-debugging effort on these.
- Pre-existing, unrelated warts in the project itself (an unused-variable
  warning from an old refactor, a stale editor diagnostic that doesn't match
  a `cargo clean` build) — verify against a fully fresh build/tool run before
  assuming a real bug, and check `git blame`/history before assuming it's
  related to whatever you're currently doing.

Keep a running list of what you've ruled out as you go — it's easy to
re-investigate the same red herring twice in a long debugging session.

## 5. Sandboxed dev tools (Claude Code, other Flatpak-packaged tools) on an atomic host

If your coding agent/IDE tooling itself runs inside a **Flatpak sandbox**
(common on these distros — check whether your tool's process tree shows a
separate `/etc`/mount namespace from the real host), be aware:

- Commands run "normally" inside that sandbox execute in a **different
  filesystem/environment than the real host** — a tool like `direnv`
  installed on the real host will not be on `PATH` inside the sandbox, no
  matter how correctly it was installed, because the sandbox simply doesn't
  see that part of the host environment.
- **The reliable invocation pattern**: `flatpak-spawn --host` to escape the
  sandbox back onto the real host, combined with `direnv exec <project-dir>
  <command>` to get the project's actual Nix-provided environment loaded for
  that one command:
  ```sh
  flatpak-spawn --host bash -lc 'direnv exec /path/to/project <command>'
  ```
  This is the pattern to reach for any time a command needs the project's
  real toolchain (`cargo`, `rustc`, a binary the project just built, etc.)
  and a bare invocation fails with a missing-binary or missing-library error
  that doesn't make sense given what you just installed.
- A binary **compiled** inside the sandboxed devenv shell is typically linked
  against Nix-store paths that don't exist in a bare host shell — running
  that exact binary directly via `flatpak-spawn --host <path-to-binary>`
  (skipping `direnv exec`) will often fail with a missing shared library
  error, because the plain host shell doesn't have the Nix-provided
  `LD_LIBRARY_PATH` set up. Prefer running it through `cargo run`/`cargo
  test` (still wrapped in `direnv exec`) over invoking the built binary path
  directly, unless you've separately confirmed the direct path works.

## 6. General debugging discipline that paid off

- **Verify hypotheses instead of guessing** — check a changelog, read the
  actual pinned version's source, or search for the specific error text,
  rather than assuming based on how a *newer* version of the same tool
  behaves. Online documentation defaults to describing the latest release.
- **Isolate variables** — when two config options interact badly, test each
  one alone before assuming which one is at fault, so you don't end up
  changing three things at once and losing track of which change actually
  fixed (or caused) the problem.
- **Keep environment/tooling fixes scoped separately from application code**
  — if you're debugging a dev-environment problem, resist the urge to also
  "clean up" unrelated application code in the same pass. Two different
  categories of change reviewed/committed together make it harder to tell
  later which fix addressed which problem.
