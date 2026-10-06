---
task_uid: "01m46w3fn0fzs63j5zwvna4vkt"
task: "T83"
rev: 6
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test typing_records_every_stage (integration binary keystroke_latency): in type/small, x gives a Sample whose instants are ordered, whose stages (dispatch, pre_layout, prepaint, paint, draw_rest, present) sum to total, with present zero (no renderer) and the x in the model text; after reset the text, selections and scroll equal the starting ones"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test movement_records_a_frame_without_an_edit (integration binary keystroke_latency): right gives a Sample, moves the caret one character and leaves the text unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test a_keystroke_with_no_effect_gives_no_frame (integration binary keystroke_latency): an unbound keystroke (ctrl-alt-shift-f19) returns NoFrame, not a sample or a hang"
    required: true
    manual: false
  - id: "C4"
    text: "cargo bench -p fe-gpui --bench keystroke -- --test exits 0 and runs type/small, type/10k-rust, type/1m-plain and move/10k-rust once each, using the real macOS text system and gpui_platform::current_headless_renderer; rg 'VisualTestAppContext|open_offscreen_window|gpui_platform::application' crates/fe-gpui/benches/keystroke crates/fe-gpui/tests/keystroke_latency.rs matches nothing (no NSWindow, so a locked screen doesn't matter)"
    required: true
    manual: false
  - id: "C5"
    text: "Scope check: git diff against the base touches no file under crates/fe-gpui/src or crates/fastedit; crates/fe-gpui/Cargo.toml changes only by criterion in [dev-dependencies] and the [[bench]] keystroke entry; Cargo.lock gains no [[package]] entry"
    required: true
    manual: false
  - id: "C6"
    text: "PERF.md's Measuring block lists the bench command, and a Typing latency section replaces the Not measured yet bullet: per case, p50 and p99 of each stage and of total from one bench run (date, commit, machine), whether total fits the 4 ms p50 / 8 ms p99 budget and if not which stage is over, and a note that this is the pre-keymap-engine baseline; TESTING.md has a row for crates/fe-gpui/tests/keystroke_latency.rs"
    required: true
    manual: false
---

# Measure keystroke-to-frame latency by stage in a headless criterion bench

# Measure keystroke-to-frame latency by stage in a headless criterion bench

Comes before the keymap cards (4a `fe-keymap`, 4b the GPUI adapter), so their effect on the key path
can be measured: run the bench before and after them and compare. Competitive research found input
latency is something editors compete on. PERF.md still lists "Typing latency (keystroke to paint;
target p50 < 4 ms, p99 < 8 ms)" under *Not measured yet*.

## Problem

Nothing times the path from a key press to the frame that shows it:

- `fastedit --perf` measures startup, scrolling, memory and idle, not typing. It also needs a real,
  unlocked display: with the screen locked the window gets no frame requests and `--perf` hangs.
- The criterion benches (`fe-buffer`, `fe-display`, `fe-syntax`) time the pieces (an edit, a
  re-wrap, a reparse, a highlight) without the GPUI dispatch, layout, shaping and paint between them.
- `EditorView::frame_timings()` gives the editor element's prepaint and paint, but nothing ties a
  frame to the keystroke that caused it.

So we can't say what a keystroke costs, where the time goes, or whether the keymap engine changes it.

## Change

A criterion bench in `fe-gpui` that dispatches a synthesized keystroke into a focused `EditorView`
and times each stage until the frame showing its effect has been drawn and submitted. Everything runs
headless: GPUI's `HeadlessAppContext` (`test-support`, already a dev-dependency feature) with the
real macOS text system and GPUI's headless Metal renderer. No `NSWindow` is opened, so a locked
screen doesn't matter. **No file under `crates/fe-gpui/src` or `crates/fastedit` changes.** The
harness uses only public API (`fe_gpui::init`, `EditorView::new`, `EditorView::set_language`,
`EditorTheme::default_dark`, `EditorView::frame_timings`/`frame_timings_mut`), so it keeps working
across cards 1b, 3 and 4b.

### The harness: `crates/fe-gpui/benches/keystroke/harness.rs` (new)

- **Setup.** `Harness::new(text_system, renderer_factory, case)` builds a `HeadlessAppContext`,
  calls `fe_gpui::init`, and opens a 1200×800 logical-pixel window (the `--perf` window size) with a
  focused `EditorView` over the case's text, with its language set when the case is highlighted. It
  settles before returning: `run_until_parked` plus frames until the view stops notifying, so
  language loading and the first full parse are done.
  - The bench passes `gpui_platform::current_platform(true).text_system()` (needs the main thread,
    which a `harness = false` bench has) and `gpui_platform::current_headless_renderer`.
  - Tests pass `gpui::NoopTextSystem` and no renderer.
- **One measured keystroke.** `Harness::keystroke(&str) -> Result<Sample, NoFrame>` dispatches
  `Keystroke::parse(…)` through `Window::dispatch_keystroke`, so it goes through the real keymap and
  action or input-handler path, as `crates/fe-gpui/src/tests.rs` does. Then it drives the window's
  frame explicitly: draw if dirty, then `present_if_needed`, which hands the scene to the headless
  renderer. A `Sample` has these instants, in order, and the stage durations between them:

  | Stage | From | To | What it covers |
  |---|---|---|---|
  | `dispatch` | before `dispatch_keystroke` | the view's first notification after it (a `cx.observe` on the view) | GPUI keystroke matching, the action or text-input handler, the model edit |
  | `pre_layout` | notified | the editor's `prepaint_started` | GPUI's layout of the element tree before the editor element |
  | `prepaint` | `FrameTiming::prepaint` | | the editor's layout: scrolling, the in-frame reparse budget, highlighting, shaping visible rows |
  | `paint` | `FrameTiming::paint` | | the editor's paint |
  | `draw_rest` | `painted_at` | the window's draw returns | the rest of GPUI's draw |
  | `present` | | `present_if_needed` returns | scene encoded and committed by the headless Metal renderer (no wait for the GPU); zero with no renderer |

  `total` runs from before dispatch to the end of `present`, and equals the sum of the stages. The
  frame used is the first editor `FrameTiming` whose `prepaint_started` is after the notification.
  GPUI test windows can also draw inside the dispatching update. Use that frame if it qualifies;
  don't count a draw that started before the handler ran.
- **Failures.** If the keystroke causes no notification, or no qualifying frame appears within a
  fixed number of driven frames, `keystroke` returns `NoFrame` rather than a sample or a hang.
- **Reset.** `Harness::reset()` undoes the keystroke's effect untimed: `backspace` after a typed
  character, or the opposite movement. Then it settles background work (a reparse moved off the
  frame) with `run_until_parked`. Every timed keystroke starts from the same text, selections, scroll
  position and highlight state.
- **Cases.** `cases()` returns these, with corpora generated inline (no fixture files):
  - `type/small`: `x` typed inside a word in about 60 lines of highlighted Rust.
  - `type/10k-rust`: the same in 10k lines of highlighted Rust, with the viewport and caret around
    line 5,000.
  - `type/1m-plain`: `x` in 1M lines of plain text (about 33 MB, no language), viewport and caret
    mid-file.
  - `move/10k-rust`: `right` in the 10k-line Rust case, the path with no edit.

### The bench: `crates/fe-gpui/benches/keystroke/main.rs` (new)

- In `crates/fe-gpui/Cargo.toml`, add `criterion.workspace = true` to `[dev-dependencies]` and a
  `[[bench]] name = "keystroke"`, `harness = false`. Nothing else in that file changes. `Cargo.lock`
  gains no `[[package]]`: criterion is already locked.
- The bench includes the harness with `mod harness;`. Each case is one criterion benchmark, in a
  `keystroke` group with a short measurement time. It times `total` with `iter_custom`, calling
  `reset` outside the timed part.
- After each case, the bench prints p50 and p99 of each stage and of `total` over that case's
  measured samples, as the `fe-syntax` benches print tree memory. Criterion's own statistics cover
  `total`.
- `cargo bench -p fe-gpui --bench keystroke` takes about a minute after the build.
  `cargo bench -p fe-gpui --bench keystroke -- --test` runs each case once.
- The module doc states the budget the numbers are read against: the plan's typing target of p50 <
  4 ms and p99 < 8 ms keystroke to paint. It also says what the harness doesn't cover (see *Out of
  scope*). Like the other benches, nothing asserts a threshold.

### PERF.md and TESTING.md

- **PERF.md:**
  - Add the bench command to *Measuring*.
  - Replace the *Not measured yet* bullet with a *Typing latency* section: one table per case with p50
    and p99 of each stage and of `total`, plus the date, commit and machine, and the command used.
  - Say per case whether `total` fits the 4 ms / 8 ms budget, and if not, which stage is over.
  - Note that this is the baseline before the keymap engine (card 4b), and is to be re-run after it.
- **TESTING.md:** one row for the new test file.

### Tests (new file)

`crates/fe-gpui/tests/keystroke_latency.rs`, an integration test binary that includes the harness
with `#[path = "../benches/keystroke/harness.rs"] mod harness;`. It uses `NoopTextSystem` and no
renderer, so it runs on a test thread. It uses small corpora only, and no sleeps or wall-clock
thresholds.

- `typing_records_every_stage`: in the `type/small` case, `x` gives a `Sample`. Its instants are in
  order, the stages add up to `total`, `present` is zero (there's no renderer), and the model text
  contains the `x`. After `reset` the text, selections and scroll position equal the starting ones.
- `movement_records_a_frame_without_an_edit`: `right` gives a `Sample` and moves the caret by one
  character, with the text unchanged.
- `a_keystroke_with_no_effect_gives_no_frame`: an unbound keystroke such as `ctrl-alt-shift-f19`
  returns `NoFrame`, promptly.

## Out of scope

- Real display latency: OS event delivery (`NSEvent` to GPUI), waiting for vsync, the compositor,
  scan-out and GPU execution time. Those need a real screen and a camera or GPU timestamps; the
  harness measures the work fastedit and GPUI do per keystroke.
- A typing phase in `fastedit --perf`, and any GUI timing criteria.
- Changes to `fe-gpui`'s sources, such as a timing hook inside the view. If the stages above prove
  too coarse, that's a follow-up after cards 1b and 4b, which own `view.rs` and `lib.rs`.
- Fixing slow stages. If a case misses the budget, PERF.md names the stage and a follow-up card is
  filed.
- Typing while a background parse is running (contention). Background work is settled between
  keystrokes.

## Decided (lead defaults; the user may revisit)

- **GPUI's own bench support isn't used.** Its `bench`/`profiler` features have `bench_platform`,
  `#[gpui::bench]`, a threaded dispatcher, and dirty-to-draw and foreground-work histograms. They
  would add `hdrhistogram` and `gpui-pre-bench-metrics` to `Cargo.lock` (a `deny`/THIRD_PARTY review)
  and turn on criterion's `html_reports`. Our own `Instant`s plus `FrameTiming` cover the stages we
  need. Revisit if GPUI-internal breakdowns are wanted.
- **The dispatcher is GPUI's deterministic `TestDispatcher`** (via `HeadlessAppContext`), not the
  threaded one, so runs are repeatable. The in-frame 1 ms reparse attempt is still timed. A reparse
  moved to the background lands untimed in `reset`, as colours already lag the text on screen.
- **`present` ends at the Metal commit** and doesn't wait for the GPU, mirroring presentation to a
  real window.
- **Four cases**: small and 10k-line highlighted Rust, 1M lines of plain text, and one movement.
  More (multi-cursor, soft wrap, Markdown) can be added later, as one line in `cases()` each.
- **A criterion bench, no CLI mode.** `cargo bench … -- <filter>` is the reproducible command.
- **The harness lives with the bench**, included by the test via `#[path]`, so neither `lib.rs` nor
  any other `src` file changes. That keeps this card clear of cards 1b, 3 and 4b, which edit
  `fe-gpui/src/lib.rs` and `view.rs`.
- **No hard dependency either way.** Cards 4a and 4b don't depend on this card, and this card
  doesn't depend on 1b. Priority (75, between card 3 at 70 and cards 4a and 4b at 80) puts it ahead
  of the keymap engine. Its shared paths (`Cargo.lock`, `crates/fe-gpui/Cargo.toml`, `PERF.md`) are
  serialised by Regatta's scope check.

## Implementation notes

- For the PERF.md numbers, build with a private `CARGO_TARGET_DIR`. The shared lane target can
  silently link another lane's `fe-gpui` into a release or bench build (seen in T8). Close CPU-heavy
  jobs first, as PERF.md's *Measuring* section says.
