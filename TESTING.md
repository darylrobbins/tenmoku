# Testing the core

The core crates (`fe-text`, `fe-buffer`, `fe-display`, `fe-editor`,
`fe-syntax`) and the editor view (`fe-gpui`) are tested in layers. Each layer catches things the others can't.

| Layer | Where | What it proves |
|---|---|---|
| Unit tests | `src/tests/mod.rs` in each crate | Specific behaviour, written in before/after form (`fe-editor` uses `\|` for carets and `[`…`]` for selections) |
| String differential | `fe-text/src/tests/large.rs`, `fe-buffer` `random_edits_match_string_model` | Every query and edit matches a plain `String`. The `fe-text` texts reach 400 KB so the rope's tree, chunk boundaries and line indexes are exercised |
| Reference model | `fe-buffer/src/tests/ideal.rs` | Undo/redo across user and AI origins matches a slow, obviously correct model (characters are never removed, only hidden). Inserted characters are unique, so nothing can match by coincidence |
| Invariants | `fe-buffer/src/tests/invariants.rs`, `fe-editor/src/tests/laws.rs` | Laws that hold for any input: anchors come home after undo, events never skip a version, movement round-trips, multi-cursor typing matches a string, random command streams keep the model well formed |
| Syntax | `fe-syntax/src/tests.rs` | One snippet per language with spans it must highlight; random batched edits, with parses finishing late, leave the same tree and highlights as a parse from scratch; a visible range highlights exactly like the same slice of the whole file; random nested and overlapping captures flatten to the spans a byte-by-byte model paints; a parse or query that panics (injected with `FAULT`) leaves plain text |
| Golden highlights | `fe-syntax/src/tests/golden.rs`, `fe-syntax/tests/golden/` | A realistic sample per language, highlighted whole, matches the spans saved in `<sample>.golden`, so grammar or query bumps and precedence changes show up as a line diff. `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax golden` rewrites them |
| Display map | `fe-display/src/{lib,wrap_map,fold_map}.rs`, `display_map/tests.rs` | One property per layer (tabs, soft wrap, folds), then all three under random edits: every buffer offset maps to the display row showing it (or a fold's header) and back, rows tile the text, and a map kept up to date edit by edit matches one built from scratch |
| Frame layout | `fe-display/src/layout/tests.rs` | Scrolling, autoscroll, visible rows, selection and cursor rectangles, line numbers, fold markers, style runs and hit testing, with soft-wrapped and folded rows, laid out against a fake monospace measurer (tabs, wide chars), with no window |
| Key bindings | `fe-gpui/src/tests.rs` | Real keystrokes go through the real keymap into a focused editor in a headless GPUI window. One table row per binding, plus typing, input-method composition, clipboard and save. `bindings_are_all_tested` fails if a binding has no row |
| Keystroke latency harness | `crates/fe-gpui/tests/keystroke_latency.rs` | The `keystroke` bench's harness (`fe-gpui/benches/keystroke/harness.rs`), run with no text system or renderer on the small corpus: typing and moving each give a sample whose instants are in order and whose stages add up to the total, `reset` restores the text, selections and scroll position, and a keystroke that changes nothing gives no frame instead of hanging. Nothing is timed against a threshold; the numbers are in PERF.md |
| Screenshots | `fe-gpui/examples/screenshots/`, `golden/` | The editor view, rendered off screen by the real Metal renderer, matches a golden PNG per scene: highlighted Rust, a multi-line selection, three carets, tab indentation, a long line cut off at the right edge, a three-digit gutter, Markdown with a Python fence, 400 KB of C, a gutter annotation column, the minimap beside 300 rows of Rust scrolled to the middle with its slider always shown, drawn as blocks (`render_characters = false`), and sticky scroll's band scrolled into a long method: its impl's and its own header rows pinned at the top with their line numbers, over a shadow, and the minimap's marks (`minimap_marks`): a multi-row selection, find matches, an error and a warning under its code, and an added, a modified and a deleted row in its git lane; the minimap drawing its characters, the default (`minimap_characters`); and the marked minimap on the left (`minimap_left`), the gutter and text after it and its git lane at its inner edge; and, from `fe-workbench/examples/screenshots/`, the workbench: tree, tabs and editor. See [Screenshots](#screenshots) |

Mutation testing (`cargo mutants -p <crate>`) is worth running by hand before a
milestone, to find code the tests don't actually check. It isn't part of CI.
Each crate takes a few minutes with `-j 6`. The known survivors are equivalent
mutants (bookkeeping that only saves work, guards a later check repeats),
trivial accessors, and `fe-gpui`'s paint geometry. Some `fe-editor` mutants
are only caught by `fe-gpui`'s tests; add `--test-workspace true` to count them.

## Running

```zsh
cargo nextest run --workspace     # what CI runs on every push; each test in its own process
```

**macOS app bundle.** `scripts/bundle-macos.sh` turns a built `target/release/tenmoku` into an
ad-hoc-signed `target/bundle/Tenmoku.app` (it does not build; Developer ID signing and
notarization are person steps in `docs/release/macos-signing.md`). `scripts/test-bundle-macos.sh`
checks it in about a second, bundling a copy of `/usr/bin/true` instead of the real binary.

**Slow tests.** A test belongs in the `profile.ci` `default-filter` slow set
(`.config/nextest.toml`) when it takes over 30 s on the gate machine in a
nextest run, or over 10 s and is not on a path the gate exists to protect. Add it
by editing that filterset in the same task that adds the test, and first try to
make it faster or smaller (see the imara Histogram and large-text notes). A test
that a task criterion or `--regression-test` names must not be in the slow set:
the gate must run it, and Regatta refuses a criterion whose test did not run in
the gate. The gate and `rgt ci run --fast` skip the slow set; `post_merge` runs
it (stage `test_all`). By hand: `cargo nextest run --profile ci
--ignore-default-filter`, or the unfiltered default profile.

**Affected packages.** The gate and `rgt ci run --fast` test only the packages
the change touches and the workspace members that depend on them
(`scripts/ci-test-affected.sh`); a workspace-wide change (`Cargo.lock`, the root
`Cargo.toml`, `scripts/`, `.config/`, `regatta.toml` and the like) runs the
whole workspace, and so does `post_merge`. So a test that a task criterion or
`--regression-test` names must be in a crate the task changes, or in one of
that crate's reverse dependencies: Regatta refuses a criterion whose test did
not run in the gate.

```zsh
scripts/soak.sh                   # 50k cases per property, release (a few minutes);
                                  # run after changing undo/redo or anchor code
scripts/check-syntax-features.sh  # fe-syntax with no languages, then each alone (CI runs it)
scripts/lsp-real.sh               # fe-lsp against real language servers; see Real language servers
scripts/check-framework-standalone.sh    # kiln/ builds and tests outside this repo; see Kiln on its own
scripts/prune-build-cache.sh --dry-run   # stale incremental dirs of this lane; see Disk
scripts/test-prune-build-cache.sh        # fixture test for the prune script
scripts/ci-test-affected.sh       # the gate's test stage: changed packages and their reverse deps
scripts/test-ci-test-affected.sh  # self-test for the affected-packages script
cargo mutants -p fe-buffer -j 4   # by hand, before milestones
cargo llvm-cov -p fe-buffer --summary-only
```

Fuzzing is separate and also by hand; see [Fuzzing](#fuzzing).

Benchmarks and the `tenmoku --perf` frame-timing run are described in
[PERF.md](PERF.md), with the latest numbers. Release binaries carry no symbols
(`strip = "symbols"`); to profile, use the bench profile (`cargo bench`), which keeps them.

`PROPTEST_CASES` sets the case count for every property test, except the
large-text rope tests, which run a hundredth as many (at least 8). Note that
`proptest!` lets the variable override `ProptestConfig::with_cases`, so a test
that needs its own count has to drive `TestRunner` itself, as `large.rs` does.
CI's `soak` workflow runs `scripts/soak.sh` on demand.

## Real language servers

fe-lsp's gate tests run against an in-process fake server. The slow group in
`crates/fe-lsp/tests/real_server.rs` runs the real servers instead:
rust-analyzer, pyright, typescript-language-server and typescript-go
(docs/decisions/lsp-client.md §6.2, §6.3).

```zsh
scripts/lsp-real.sh                            # installs the servers, runs the tests
FE_LSP_REQUIRE_SERVERS=1 scripts/lsp-real.sh   # a missing server fails instead of skipping
```

The script does three things:

1. It installs Node from `tools/lsp/mise.toml` (`mise install`) and the npm
   servers from `tools/lsp/package-lock.json` (`npm ci --prefix tools/lsp`).
   They are pinned there for these tests only. Tenmoku never installs
   servers for users.
2. It adds the pinned toolchain's rust-analyzer (`rustup component add
   rust-analyzer`). The component isn't listed in `rust-toolchain.toml`, so
   everyday builds don't download it.
3. It runs `cargo nextest run -p fe-lsp --run-ignored only -E
   'test(/real_server_/)'` on tools/lsp's Node.

Each `real_server_*` test is `#[ignore]`d, so `cargo nextest run --workspace`
runs none of them. Each one copies its project from
`crates/fe-lsp/tests/fixtures/real/` into a temporary directory. It finds its
server's binary in this order:

- `FE_LSP_RUST_ANALYZER`, `FE_LSP_PYRIGHT`, `FE_LSP_TYPESCRIPT_LANGUAGE_SERVER`
  or `FE_LSP_TYPESCRIPT_GO`
- `tools/lsp/node_modules`
- `rustup which`

A missing server prints `skipped: <bin> not found (run scripts/lsp-real.sh)`
and the test passes, unless `FE_LSP_REQUIRE_SERVERS=1`.

The tests check:

- launch to `Ready` within 60 s
- §4.4's emoji cases 1, 3 and 7 at the columns the probe measured:
  - rust-analyzer under utf-8, and under utf-16 (offered alone)
  - pyright and typescript-language-server under utf-16
  - typescript-go under utf-8
- definition and references through `LspNavigation`
- a clean stop (typescript-go's exit code 1 counts as one)
- rust-analyzer's `cargo check` diagnostics after saving a type error, and
  after saving its fix
- whether each server's `publishDiagnostics` carries a version

A warm run takes about 35 s.

`.github/workflows/lsp.yml` runs the script on `workflow_dispatch` only, on
`macos-15`, with `FE_LSP_REQUIRE_SERVERS=1`: never on PRs and not on a
schedule. Run it from the Actions tab or with `gh workflow run lsp.yml`.

**M4's manual check: go-to-definition in the app.** The script's
`LspNavigation` definition checks through rust-analyzer, pyright and
typescript-language-server cover it headlessly. A person runs these steps
for card T323, and again after changing fe-lsp, `LspNavigation` or the
workbench's LSP wiring. Point the app at
tools/lsp's servers, on tools/lsp's Node, with a scratch config directory.
rust-analyzer comes from rustup's proxy on your `PATH`.

```zsh
scripts/lsp-real.sh    # installs tools/lsp's servers
lsp_path=$(mise exec -C tools/lsp -- printenv PATH)
mkdir -p /tmp/fe-lsp-check && cat > /tmp/fe-lsp-check/settings.toml <<EOF
[lsp.servers.pyright]
command = "$PWD/tools/lsp/node_modules/.bin/pyright-langserver"
env = { PATH = "$lsp_path" }
[lsp.servers.typescript-language-server]
command = "$PWD/tools/lsp/node_modules/.bin/typescript-language-server"
env = { PATH = "$lsp_path" }
EOF
cp -R crates/fe-lsp/tests/fixtures/real /tmp/fe-lsp-real
# The tests write the Rust fixture's manifest into their copy; so does this.
printf '[package]\nname = "real"\nversion = "0.1.0"\nedition = "2024"\n\n[workspace]\n' \
  > /tmp/fe-lsp-real/rust/Cargo.toml
for project in rust python typescript; do
  TENMOKU_CONFIG_DIR=/tmp/fe-lsp-check cargo run -p fastedit -- /tmp/fe-lsp-real/$project
done
```

In each project, open `src/main.rs`, `main.py` or `main.ts`, and wait for the
status bar to show the server ready. Then:

- Press F12 on `e1`, `e3` and `e7` in the row below each declaration. The
  caret must land on the declared name, after the emoji, the ZWJ family and
  the combining accent.
- Press F12 on the call of `hello`. It must open the greet file with the caret
  on `hello`.

## Kiln on its own

`scripts/check-no-gpui.sh` checks that kiln/ names nothing of fastedit's. It
can't show that kiln builds without our root `Cargo.toml`, `Cargo.lock`,
patches and `vendor/`, which it must before it is split out
(docs/decisions/app-framework.md, card K6.2).
`scripts/check-framework-standalone.sh` builds it the way it will be split out:

```zsh
scripts/check-framework-standalone.sh            # copy, build, test, then delete the copy
scripts/check-framework-standalone.sh --keep     # keep the scratch workspace after a pass
scripts/check-framework-standalone.sh --dir /tmp/kiln-ws --target-dir /tmp/kiln-target
scripts/check-framework-standalone.sh --online   # let cargo fetch what the local cache lacks
```

1. It copies kiln/'s tracked and new files (not ignored ones) into a new
   directory under `$TMPDIR`, with `rust-toolchain.toml`. The repo has no
   `.cargo/config.toml` for it to copy.
2. `scripts/kiln_standalone.py` writes the scratch `Cargo.toml` from ours,
   keeping only the `[workspace.package]` keys, `[workspace.dependencies]`
   entries, lints, `[patch.crates-io]` entries and profile overrides that the
   kiln crates use. A patch that points into `vendor/` is copied with it and
   listed. kiln needs none today: `tree-sitter-yaml` is the only patch, and
   it is fastedit's.
3. It copies our `Cargo.lock` in and lets cargo drop the packages kiln
   doesn't use. The helper then checks that every package left is at the
   version and source our lock pins.
4. It runs `cargo build --locked --offline --workspace --all-targets` and
   `cargo test --locked --offline --workspace`, using the local registry
   cache, in a target dir of its own (`$TMPDIR/kiln-standalone-target`,
   reused between runs; our `CARGO_TARGET_DIR` is ignored).
5. It prints what kiln still takes from this repo: the inherited manifest
   entries, the toolchain pin, the nextest retries, `deny.toml`, the repo
   paths its comments cite, and its `THIRD_PARTY.md` rows and missing
   licence files.

On a failure it names the step, keeps the scratch workspace and prints the
command to rerun there. A cold run took 71 s with `CARGO_BUILD_JOBS=4` and left
a 2.2 GiB target dir; a warm one takes a few seconds. It isn't in CI yet.

## Disk

Each regatta lane builds in its own `target/`, and so does each gate snapshot
(`.worktrees/.verify/<id>`), cold. `scripts/ci-env.sh` also defaults `CARGO_BUILD_JOBS` to 6
(a caller's value wins) so concurrent lanes and gates don't oversubscribe the cores.
`scripts/ci-env.sh` makes snapshots light: `CARGO_INCREMENTAL=0` and
`CARGO_PROFILE_DEV_DEBUG=line-tables-only` (panics keep file:line; the workspace
`[profile.dev]` sets the same, so lane and local builds are light too; for a debugger
session run `CARGO_PROFILE_DEV_DEBUG=limited cargo build`), and it exits 75
with a one-line message when the volume has under `FASTEDIT_MIN_FREE_GIB` (default 10)
free. Lanes are never checked. A cold snapshot build (`cargo build --workspace
--all-targets` plus `nextest run --profile ci --no-run`) took 144 s and left a 4.1 GiB
`target/`, down from about 6 GiB.

### Compiler cache

Every lane and snapshot has its own target dir, so each would compile GPUI and the rest of
the dependency tree itself. `scripts/ci-env.sh` therefore sets `RUSTC_WRAPPER=sccache` when
`sccache` is on `PATH` and `RUSTC_WRAPPER` is unset (`brew bundle` installs it), and all
lanes and gates share one compiler cache. The wrapper is `scripts/sccache-wrap.sh`: sccache
keys on every `CARGO_*` variable, and each lane and gate has its own `CARGO_TARGET_DIR`,
so it is unset for the compile. It lives in `~/Library/Caches/fastedit-sccache`
(`SCCACHE_DIR`; Time Machine skips `~/Library/Caches`) and is capped at 8G
(`SCCACHE_CACHE_SIZE`); set either in the environment to override. Turn it off with
`RUSTC_WRAPPER=` (empty). `sccache --show-stats` shows hits. The sccache server fixes its
directory when it starts, so after changing `SCCACHE_DIR` or the size run
`sccache --stop-server`. Lanes build incrementally, which sccache skips, so it
only serves their dependencies; snapshots (`CARGO_INCREMENTAL=0`) get their own entries.

`scripts/prune-build-cache.sh [--dry-run] [--older-than MIN] [--if-below GIB]` deletes
incremental session dirs older than `--older-than` minutes (default 180) from the
lane's own `target/debug/incremental` (`$CARGO_TARGET_DIR` if set), always keeping each crate's newest, and prints the GiB
freed. `--if-below` does nothing while that much is free.

## Screenshots

```zsh
cargo run -p fe-gpui --example screenshots                       # compare every scene with its golden
UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots  # rewrite the goldens
cargo run -p fe-gpui --example screenshots -- render out.png path/to/file [--size 800x480]

cargo run -p fe-workbench --example screenshots                  # the workbench scene
UPDATE_SCREENSHOTS=1 cargo run -p fe-workbench --example screenshots
cargo run -p fe-workbench --example screenshots -- render out.png path/to/dir [--size 1000x600]

cargo run -p kiln --example gallery-screenshots                  # kiln's gallery pages
UPDATE_SCREENSHOTS=1 cargo run -p kiln --example gallery-screenshots
cargo run -p kiln --example gallery-screenshots -- render out.png <page> [--size 1000x640]
```

Both examples use kiln's harness, `kiln_platform::screenshots` (behind
kiln-platform's `test-support` feature, which `kiln/test-support` forwards to):
the settling loop, the comparison, its tolerances, the golden updates and the
failure output below are shared, and their unit tests run in kiln-platform's
(`screenshots::…::tests`). Each example keeps its scenes and goldens.

The gallery's scenes are its Controls, Lists and Theme pages, dark and light,
each in a 1000×640 window inside `kiln::Root` as the gallery runs:
`gallery_<page>_<theme>`, with goldens in
`kiln/crates/kiln/examples/gallery-screenshots/golden/`. Only macOS compares
with them; elsewhere the run prints `skipped` and exits 0. `render` draws any
page (`controls`, `text`, `lists`, …) in the theme `KILN_GALLERY_THEME` names
(dark unless `light`), and works on Linux too: there the harness uses GPUI's
headless context, drawn by wgpu (it needs a Vulkan device, such as Mesa's
lavapipe) in Linux's fonts, which is for looking at a page, not for goldens.

The workbench scene lives in `crates/fe-workbench/examples/screenshots/`, with
its golden in `golden/workbench.png`. The scene is
`workbench`, 1000×600 logical pixels: a demo project written into
`<tempdir>/demo` with `src/` expanded, `README.md` and `src/main.rs` open
(`main.rs` active, a caret at its start, Rust highlighted, the minimap on as by default), `main.rs` selected
in the Project dock, the unified title bar on and the editor font pinned to
Menlo. The harness registers `fe_ui::Assets` so tree icons and chevrons draw;
AppKit's traffic lights aren't part of an off-screen render. Its failures write
`target/screenshots/workbench.actual.png` and `workbench.diff.png`, which CI
uploads with fe-gpui's. `render` shows a folder in the workbench with nothing
opened. A change to the default layout's look regenerates this golden in the
same change.
A change to the gutter or text layout (`gutter_width`, `layout_frame*`) regenerates the
goldens in the same change; made off the Mac, it needs a Mac follow-up to do that, as
147b9f1a and ef074cdf did in 2026-10.

Scenes render through GPUI's `VisualTestAppContext`: a window at
(-10000, -10000), 800×480 logical pixels, drawn by Metal and read back from
its texture. Nothing appears on screen and no screen-recording permission is
needed, so agents can run all three commands. A scene is drawn until a
frame and the background work it starts (loading languages, parsing) leave
the view without a notification, then that frame is captured. After ten
frames the run fails instead. The last two scenes are cold: they must be the
first to load their languages, and their highlights must land from a second
round of background work, an injected language or a parse too long for a
frame. The run fails if either check doesn't hold, so they keep covering
highlights that land late. AppKit has to run on the main thread,
so scenes render only under `cargo run`; `cargo test` and the gate run the
harness's unit tests but never render. Off macOS the examples print `skipped`
and exit 0.

A pixel matches when each RGBA channel is within ±2/255 of the golden's. A
scene passes when at most 0.1% of its pixels differ. On failure the run
prints the mismatch, writes `target/screenshots/<scene>.actual.png` and
`<scene>.diff.png` (the capture dimmed, with differing pixels in magenta), and
exits nonzero. A size mismatch fails outright and writes the same two files;
its diff covers both images, with the area only one of them has in magenta.
It usually means the display's scale factor differs from the goldens' 2×.
GitHub's `macos-15` runners have a 1× display, so CI sets
`SCREENSHOTS_ALLOW_OTHER_SCALE`: a capture that is a golden at another scale
(another size, same aspect ratio) prints `skipped` instead of being compared.
CI still renders every scene and fails one that doesn't settle or a cold
scene whose highlights don't land late, but it compares pixels only with
goldens at its own scale, which it has none of yet.

After an intended rendering change, regenerate with `UPDATE_SCREENSHOTS=1`,
look at every changed PNG, and commit them with the change. Rendering is
deterministic on one machine: regenerating without a change leaves the files
byte-identical.

`render` opens a file in the same pinned theme and saves one frame. Use it to
look at a UI change without a screen; the image is 2× the given size.

**Font.** Goldens pin the font to Menlo rather than taking whatever
`EditorTheme::default_dark` finds installed. Menlo ships with macOS under
Apple's licence. It is used from the system, not bundled or redistributed,
so it needs no `deny.toml` or THIRD_PARTY.md entry. A macOS update can
change Menlo's rendering. If goldens drift, or CI's `macos-15` renders text
differently from a local machine, take the goldens from CI rather than
raising the tolerance: a failing run uploads `target/screenshots/` as the
`screenshots` artifact, and its `<scene>.actual.png` files become the new
`golden/<scene>.png`. JetBrains Mono
(OFL-1.1) would be steadier, but bundling it needs the licence added to
the allowlist and a THIRD_PARTY.md row.

## Linux

macOS is the primary platform, but the app, every test and every bench
also build and run on Linux (X11 or Wayland). CI's `linux` job runs clippy
and the tests on Ubuntu 24.04, then launches the app under Xvfb and fails
if it draws nothing. On
Ubuntu 26.04, GPUI needs:

```sh
sudo apt install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev \
  libfontconfig-dev libfreetype-dev libssl-dev libx11-xcb-dev libxcb1-dev \
  libxcb-xkb-dev libasound2-dev libzstd-dev clang cmake mesa-vulkan-drivers
```

Then `cargo run -- <folder or file>`, and `cargo test --workspace` as usual.
Without a display, run under Xvfb (`apt install xvfb`), which renders
through Mesa's software Vulkan driver, and drive it with `xdotool`.

What differs from macOS:

- **Keys** come from `fe-keymap/keymaps/fastedit.toml`, resolved on Linux: ctrl where
  macOS has cmd, and PC keys (ctrl-left by word, home and end, ctrl-home)
  where macOS has its own. A user `keymap.toml` layers over it as on macOS.
  Shifted punctuation is named by what it types: `ctrl-{`, not
  `ctrl-shift-[`.
- **Prompts** such as the trust question are drawn in the window
  (`fe_ui::prompt_renderer`), not as native alerts.
- **The window manager** draws the title bar where it can (X11, KDE). Where
  it leaves that to the app, as GNOME on Wayland does, the menu bar's row
  becomes the title bar, with the folder's name, minimize, maximize and close,
  a drag to move and a double click to maximize, and the window's edges
  resize it (`kiln/crates/kiln-shell/src/window_frame.rs`). The unified title bar is
  macOS's.
- **The menu bar is drawn in the window**, above the panes, from
  `fe-keymap/menus/fastedit.toml`, resolved on Linux: GPUI has no native menu bar on
  Linux. F10 opens it. See `docs/decisions/linux-menu-bar.md`.
- **Screenshots** render with Metal, so their examples only run on macOS;
  `cargo test` still runs their comparison's unit tests.
- **`tenmoku --perf`** times launch from the kernel's process start to
  the nearest clock tick (10 ms), and reports memory as the resident set
  and its peak: Linux has no physical footprint as macOS counts it.

## Fuzzing

The grammars are C, several with hand-written external scanners, and a
memory error or abort in one takes down the editor: fe-syntax's
`catch_unwind` only stops panics. `fuzz/` has two cargo-fuzz targets that
run under AddressSanitizer:

- `syntax_edits`: arbitrary text in every language, edited in batches, must
  end with the same tree and highlights as a fresh parse.
- `buffer_edits`: edit, undo and redo streams over two origins match a
  `String`, and a refused undo or redo changes nothing.

Run both for ten minutes or more by hand before a milestone. They need
nightly and are never part of `cargo test` or the gate: `fuzz/` is its own
cargo workspace, outside the root one. [fuzz/README.md](fuzz/README.md) has
the commands, including the `CFLAGS` that instrument the C code, and what to
do with a crash. Minimized crashing inputs live in
`fuzz/regressions/<target>/`.

## When a property test fails

The reference-model tests shrink a failing session to a few operations and
print them. Add that session to the `regressions` module in
`fe-buffer/src/tests/ideal.rs`, where `trace` prints it step by step, and fix
it. proptest also saves failing seeds under `proptest-regressions/`; commit
those.

## Known limitation: refused undos

Undo and redo map positions through later edits using offsets. Offsets can't
always say in what order text belongs when several undos and redos put text
back at the same spot, or inside text a later edit replaced and then reverted.
In those cases the buffer refuses with `UndoError::Conflict` rather than guess.
A refusal never changes the text.

`avoidable_refusals_stay_rare` tracks how often a refused transaction's text
was actually intact. It is 1.8% of attempts in its adversarial sessions, with
a budget of 2.2%. Plain undo and redo of a single origin are exact and never
refused. Removing the limitation would mean giving every character an identity,
as collaborative editors do.
