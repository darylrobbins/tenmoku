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
| Screenshots | `fe-gpui/examples/screenshots/`, `golden/` | The editor view, rendered off screen by the real Metal renderer, matches a golden PNG per scene: highlighted Rust, a multi-line selection, three carets, tab indentation, a long line cut off at the right edge, a three-digit gutter, Markdown with a Python fence, and 400 KB of C; and, from `fe-workbench/examples/screenshots/`, the workbench: tree, tabs and editor. See [Screenshots](#screenshots) |

Mutation testing (`cargo mutants -p <crate>`) is worth running by hand before a
milestone, to find code the tests don't actually check. It isn't part of CI.
Each crate takes a few minutes with `-j 6`. The known survivors are equivalent
mutants (bookkeeping that only saves work, guards a later check repeats),
trivial accessors, and `fe-gpui`'s paint geometry. Some `fe-editor` mutants
are only caught by `fe-gpui`'s tests; add `--test-workspace true` to count them.

## Running

```zsh
cargo nextest run --workspace     # what CI runs on every push; each test in its own process
scripts/soak.sh                   # 50k cases per property, release (a few minutes);
                                  # run after changing undo/redo or anchor code
scripts/check-syntax-features.sh  # fe-syntax with no languages, then each alone (CI runs it)
scripts/prune-build-cache.sh --dry-run   # stale regatta incremental dirs; see Disk
scripts/test-prune-build-cache.sh        # fixture test for the prune script
cargo mutants -p fe-buffer -j 4   # by hand, before milestones
cargo llvm-cov -p fe-buffer --summary-only
```

Fuzzing is separate and also by hand; see [Fuzzing](#fuzzing).

Benchmarks and the `fastedit --perf` frame-timing run are described in
[PERF.md](PERF.md), with the latest numbers.

`PROPTEST_CASES` sets the case count for every property test, except the
large-text rope tests, which run a hundredth as many (at least 8). Note that
`proptest!` lets the variable override `ProptestConfig::with_cases`, so a test
that needs its own count has to drive `TestRunner` itself, as `large.rs` does.
CI's `soak` workflow runs `scripts/soak.sh` on demand.

## Disk

Regatta lanes share one build cache (`~/Library/Caches/fastedit-regatta/target`);
each gate snapshot (`.worktrees/.verify/<id>`) builds cold in its own `target/`.
`scripts/ci-env.sh` makes snapshots light: `CARGO_INCREMENTAL=0` and
`CARGO_PROFILE_DEV_DEBUG=line-tables-only` (panics keep file:line), and it exits 75
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
incremental session dirs older than `--older-than` minutes (default 180) from the shared
cache's `debug/incremental`, always keeping each crate's newest, and prints the GiB
freed. `--if-below` does nothing while that much is free.

## Screenshots

```zsh
cargo run -p fe-gpui --example screenshots                       # compare every scene with its golden
UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots  # rewrite the goldens
cargo run -p fe-gpui --example screenshots -- render out.png path/to/file [--size 800x480]

cargo run -p fe-workbench --example screenshots                  # the workbench scene
UPDATE_SCREENSHOTS=1 cargo run -p fe-workbench --example screenshots
cargo run -p fe-workbench --example screenshots -- render out.png path/to/dir [--size 1000x600]
```

The workbench scene lives in `crates/fe-workbench/examples/screenshots/`, with
its golden in `golden/workbench.png`. It includes fe-gpui's `compare.rs` by
`#[path]`, so the comparison, its tolerances and the failure output below are
shared, and its `compare::tests` run for it as well. The scene is
`workbench`, 1000×600 logical pixels: a demo project written into
`<tempdir>/demo` with `src/` expanded, `README.md` and `src/main.rs` open
(`main.rs` active, a caret at its start, Rust highlighted), `main.rs` selected
in the Project dock, the unified title bar on and the editor font pinned to
Menlo. The harness registers `fe_ui::Assets` so tree icons and chevrons draw;
AppKit's traffic lights aren't part of an off-screen render. Its failures write
`target/screenshots/workbench.actual.png` and `workbench.diff.png`, which CI
uploads with fe-gpui's. `render` shows a folder in the workbench with nothing
opened. A change to the default layout's look regenerates this golden in the
same change.

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
so scenes render only under `cargo run`. The example is built as a test
(`test = true`), so `cargo test` and the gate run the comparison's unit
tests (`compare::tests`) but never render.

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
