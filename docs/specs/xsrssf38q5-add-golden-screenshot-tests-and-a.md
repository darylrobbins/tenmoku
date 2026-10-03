---
task_uid: "01m417z3xg3xxsgxxsrssf38q5"
task: "T11"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui example 'screenshots' is built as a test (test = true); its tests compare::tests::identical_images_match, compare::tests::pixel_beyond_tolerance_fails and compare::tests::size_mismatch_fails pass under nextest"
    required: true
    manual: false
  - id: "C2"
    text: "Gate stages build and vet compile and lint the example, and the gate's test stage renders no scene"
    required: true
    manual: false
  - id: "C3"
    text: "cargo run -p fe-gpui --example screenshots passes against committed goldens for six scenes (highlighted Rust, multi-line selection, multi-cursor, tabs, cut-off long line, three-digit gutter); a change beyond the tolerance (a channel off by more than 2/255 in more than 0.1% of pixels) or a size mismatch exits nonzero and writes both .actual.png and .diff.png under target/screenshots/; UPDATE_SCREENSHOTS=1 rewrites the goldens (manual: run log)"
    required: true
    manual: false
  - id: "C4"
    text: "'screenshots -- render out.png \u003cfile\u003e' writes a PNG from an agent shell with no visible window and no screen-recording permission (manual)"
    required: true
    manual: false
  - id: "C5"
    text: "TESTING.md documents the commands, tolerances, regeneration, and the pinned font and its licence"
    required: true
    manual: false
---

# Add golden screenshot tests and a headless screenshot command for the editor view

## Problem

Agents working on fastedit cannot check UI changes: macOS screen capture needs
screen-recording permission, which the agent shell lacks, so every visual
check so far has needed a person (tasks/README.md: T-06 "lets agents check UI
without screen-recording permission").

gpui-pre 0.3.7 (Apache-2.0, allowed by CLEAN_ROOM.md) ships
`gpui::VisualTestAppContext` (`src/app/visual_test_context.rs`). It renders
with the real Metal platform into an off-screen window (at -10000,-10000) on the
deterministic `TestDispatcher`, and `capture_screenshot(window)` reads the
frame's texture through `Window::render_to_image`, so no screen-recording
permission is involved. Two constraints shape the design:

- `Window::render_to_image` and `VisualTestAppContext::capture_screenshot` are
  `#[cfg(any(test, feature = "test-support"))]` (gpui `src/window.rs:2703`),
  so they can't go in the shipping `fastedit` binary without compiling GPUI's
  test support into it.
- AppKit must run on the process's main thread. gpui-pre-platform's own visual
  tests are `#[ignore]` for this reason ("Standard Rust tests run on worker
  threads, which causes SIGABRT"). Under nextest that would need a custom
  harness (a new `libtest-mimic` dependency). A cargo example's `main` already
  runs on the main thread.

## Change

Add a cargo example to fe-gpui, `crates/fe-gpui/examples/screenshots/`, with
three modes:

1. `cargo run -p fe-gpui --example screenshots` renders each scene below into
   a fixed-size window (e.g. 800×480 logical px, scale 2) through
   `VisualTestAppContext::new(gpui_platform::current_platform(false))`. It runs
   `run_until_parked` so the background parse lands and highlights show, then
   compares each capture with `examples/screenshots/golden/<scene>.png`. A
   pixel matches when every channel is within a small tolerance (start with
   ±2/255). A scene passes when at most a small number of pixels differ (start
   with 0.1%). On failure it writes `<scene>.actual.png` and `<scene>.diff.png`
   (mismatched pixels in magenta) to `target/screenshots/`, prints their paths,
   and exits nonzero.
2. `UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots` rewrites
   the goldens.
3. `cargo run -p fe-gpui --example screenshots -- render <out.png> <file>
   [--size WxH]` opens `<file>` in an `EditorView` with the same pinned theme
   and saves one frame. This is the dev command that lets people and agents
   review UI changes without a screen.

Scenes, built from `EditorModel::new(Buffer::new(..), Some("scene.<ext>"))` plus
`set_selections`, with the editor focused:
- highlighted Rust (about 30 lines);
- a multi-line selection;
- multi-cursor (three carets);
- a line indented with tabs;
- a long line cut off at the right edge;
- the gutter with the active line beyond row 99, so line numbers have three digits.

Pin the theme: build an `EditorTheme` from `EditorTheme::default_dark` and then
set `font_family` explicitly, so goldens don't depend on what
`default_dark`'s installed-font lookup finds. Which font to use is the open
question below.

Put the pixel comparison in `examples/screenshots/compare.rs` with unit
tests, and set `[[example]] name = "screenshots", test = true` in
`crates/fe-gpui/Cargo.toml`. The gate's test stage then runs the comparison
tests but never renders a scene. Rendering stays out of plain `cargo test`.

Dependencies. A person must approve these, because Cargo.lock is a human path:
- `image` (MIT OR Apache-2.0, already in the lock at 0.25 through gpui) as a
  `[workspace.dependencies]` entry with `default-features = false,
  features = ["png"]`, and as a fe-gpui dev-dependency, to decode goldens and
  encode diffs;
- `gpui_platform` as a fe-gpui dev-dependency, for `current_platform`.

No new crates enter the lock, only new edges.

Add a `screenshots` step to the macOS `check` job in
`.github/workflows/ci.yml` that runs the example. This is a human path too.

Document the commands, the tolerances, how to regenerate goldens and the font
choice in TESTING.md (new row in the layer table plus a "Screenshots" section).

## Notes

- Goldens are rendered on one macOS version. CI's `macos-15` may render text
  differently from the local machine. If it does, generate goldens on CI
  (upload `target/screenshots/` as an artifact) rather than raising the
  tolerance.
- T-05 (Spawner seam) changes how parses are scheduled. If it lands first,
  drive parsing through its seam instead of `run_until_parked`. The two cards
  share no files.
- Rendering changes in T-17 and T-28 will need `UPDATE_SCREENSHOTS=1`. Land
  this first so those cards can use the goldens.

## Open questions

- **Font (from the source card):** JetBrains Mono is OFL-1.1, which isn't on
  the `deny.toml` allowlist. Bundling it also needs a THIRD_PARTY.md row. The
  alternative is Menlo, which ships with macOS and needs no licence decision
  but can change between macOS versions. Which one should the goldens use?
- **Dev command form:** the source card asked for `fastedit --screenshot
  out.png <file>`. Because `render_to_image` is gated behind GPUI's
  `test-support`, this card ships it as the fe-gpui example's `render` mode,
  which shows the editor view without the workbench's placeholder project
  panel. Is that acceptable, or should `fastedit` get an off-by-default
  `screenshot` cargo feature? The gate would not build that feature.

## Implementation notes

- **Font:** Menlo. It needs no licence change (system font, not bundled), and
  regenerating goldens is cheap if a macOS update shifts it. Switching to
  JetBrains Mono later means bundling it, a `deny.toml` OFL-1.1 entry and a
  THIRD_PARTY.md row, then `UPDATE_SCREENSHOTS=1`.
- **Dev command:** shipped as the example's `render` mode, as the card
  proposes; `fastedit` gets no screenshot feature.
- `capture` draws the window twice with `run_until_parked` between, so the
  first frame's visible range drives the parse and the second paints its
  highlights; `capture_screenshot` reads the last drawn frame.
- `gpui_platform` needs its `test-support` feature as a dev-dependency:
  `MacWindow::render_to_image` is gated on it too. That feature adds a
  `gpui-pre-platform → gpui-pre-wgpu` edge to the lock (no new crates).
- Captures are 1600×960 (scale 2 on this Mac) and byte-identical across runs.
  The 0.1% allowance lets up to 1,536 pixels differ.
- Every failure, a size mismatch included, writes both `.actual.png` and
  `.diff.png` (`compare::save_failure`). A size mismatch's diff covers both
  images: the shared area is compared pixel by pixel, and the area only one
  image has is magenta.
- CI uploads `target/screenshots/` as an artifact when the step fails.
