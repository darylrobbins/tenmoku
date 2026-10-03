---
task_uid: "01m417z4dgn0pbt6dy9b0pbvqn"
task: "T24"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax: with an over-limit snapshot the driver schedules window parses through the Spawner and never parses on the caller's thread (test driver::window_tests::window_parses_never_run_on_the_caller)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax: viewport changes during a window job start exactly one follow-up, for the latest viewport, and the old window keeps highlighting until it lands (test driver::window_tests::scrolling_coalesces_to_the_latest_viewport)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui: scrolling to the middle of a 9 MB Rust buffer shows keyword highlights in the visible rows once work settles (test view::window_tests::a_huge_file_highlights_around_the_viewport)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-syntax: after jumping the viewport across an over-limit Rust snapshot many times, the driver retains a bounded number of windows and their syntax trees' live bytes, counted with a counting allocator (tree-sitter's set_allocator or a global one), stay under 0.5x the snapshot size (test driver::window_tests::window_memory_stays_bounded)"
    required: true
    manual: false
---

# Highlight around the viewport of files over the parse limit, parsing off the UI thread

Source: Paddock T-16 (`c9403034-4af6-4293-9707-993a834ed9c9`), P2, draft (needs the user's approval).
Split from T-16 along the risk line: this is the stateful half. The window parse itself is T-16a.

## Problem

T-16a lets `Syntax` parse and highlight a 32 KB window of a file over `MAX_PARSE_BYTES`. Nothing asks
for one yet. The window has to follow the viewport while scrolling without ever parsing on the UI
thread: PERF.md shows jump-scrolling a 16 MB plain file at 2.2 / 3.6 ms prepaint (p50 / p99) with 7
missed vsyncs in 600 frames, and a highlighted 16 MB file at 6.5 / 11.6 ms with 14 missed, so there's
little room.

## Change

Build on T-05's `SyntaxDriver` (`crates/fe-syntax/src/driver.rs`), whose `Spawner` already runs
`ParseJob`s, so a window job needs no new seam. With an over-limit snapshot:

- The driver takes the visible byte range on each sync. If `Syntax::window()` doesn't cover it, or
  the window's tree predates the snapshot, it schedules `Syntax::window_job` through the `Spawner`.
  It never runs a window job on the caller's thread, whatever the budget.
- Single flight, coalesced: while a window job runs, viewport changes only record the latest
  viewport. When the job lands, the driver installs it with `Syntax::finish` and, if the latest
  viewport isn't covered, starts exactly one job for it. No timers, so tests stay deterministic.
- The last window's spans keep showing until the new window lands (T-16a keeps the old window).
- A full-tree job and a window job never run at once for the same buffer; below the limit nothing
  changes.
- `SyntaxDriver::sync` takes the visible byte range. `EditorView::sync_syntax`
  (`crates/fe-gpui/src/view.rs`) passes it from the last frame's layout (`self.layout`); T-05's
  `Spawner` adapter already notifies the view when a job lands. `crates/fe-gpui/src/element.rs`
  doesn't change: it highlights through `SyntaxDriver::syntax()` and `Syntax::highlights`.

Driver tests use T-05's `ManualSpawner` (make it `pub(super)` in `driver/tests.rs`), in a new module
declared from `driver.rs` as `#[cfg(test)] mod window_tests;` (`crates/fe-syntax/src/driver/window_tests.rs`). The GPUI test goes in a new module declared from `view.rs` as
`#[cfg(test)] mod window_tests;` (`crates/fe-gpui/src/view/window_tests.rs`), on the pattern of
`tests::highlighting_follows_edits` in `crates/fe-gpui/src/tests.rs`.

Don't consult Zed's GPL crates for viewport highlighting (CLEAN_ROOM.md).

## Implementation notes

- **The driver keeps the latest viewport.** Every `sync` records its visible range, including
  while a job runs. `finish` takes the spawner. After installing a result, it starts one window
  parse for the recorded viewport, if that isn't covered. The view's adapter passes its own
  spawner, so a follow-up doesn't wait for another render.
  `scrolling_coalesces_to_the_latest_viewport` checks that the follow-up starts from `finish`
  alone.
- **The driver tracks freshness, not `Syntax`.** The driver records the version and viewport each
  window job was spawned for. A window counts as current when its version matches the snapshot. It
  counts as covering the viewport when it contains the visible range, or when it was parsed for
  exactly that range. The second case stops a viewport wider than 32 KB from respawning forever.
  `syntax.rs` is unchanged.
- **Re-sync after paint.** `render` syncs before prepaint, so it sees the previous frame's rows. A
  scroll that settles would never get its window. For files over the limit, `render` therefore
  also runs `cx.defer_in(window, sync_syntax)`, which fires after the frame is painted. It spawns
  only when the new rows aren't covered. Without it, `a_huge_file_highlights_around_the_viewport`
  fails.
- **Perf context, 2026-10-03.** These runs were for the first spec's manual C4, which the respec
  replaced. They are context only. The release binary came from the shared cargo target, which
  other lanes were building into at the same time. So it may have linked another lane's crates,
  which would also explain why the no-window variant measured higher. Release build, `fastedit --perf` on a 16.8 MB Rust file (the crate sources
  repeated) against the same bytes as `.txt`. Four runs of each, on a loaded machine (load
  average 4.6–7), so absolute numbers are above PERF.md's. Highlighted vs plain:
  - jump prepaint p50 2.3–2.9 ms vs 2.3–3.1 ms;
  - jump prepaint p99 5.4–7.0 ms vs 6.4–8.6 ms;
  - missed vsyncs in the jump phase 20–40 vs 26–45;
  - footprint after scrolling 314–321 MB vs 306–312 MB.

  Prepaint and missed vsyncs are the same within noise. The footprint difference is 7–9 MB
  (0.42–0.56× the file size). That is at the 0.5× bound, not clearly under it. At file shown the
  difference is 2 MB, the compiled Rust query; the rest builds up while scrolling.

  **Attribution, also 2026-10-03, load average 6–8.** After the fold fix, the real build was run
  against a variant whose window parses never spawn. That variant highlights nothing, but it loads
  and compiles the Rust query.
  - Real build: `.txt` 311 / 311 MB, `.rs` 313 / 314 MB, so 2–3 MB over plain.
  - No-window variant: `.rs` 319 / 319 MB.

  Doing less work came out higher, so run-to-run footprint noise (±5 MB) is as large as the
  0.5× bound of 8.4 MB. The window parses are not shown to cost anything measurable. Getting a
  clean number would take paired runs on a quiet machine. Later attempts that afternoon timed out
  at load average 15 with other lanes running.
- **C4, window memory.** `window_memory_stays_bounded` counts tree-sitter's live bytes through
  `tree_sitter::set_allocator`, using an allocator that forwards to libc and sizes blocks with
  `malloc_size` (`malloc_usable_size` off macOS). tree-sitter allows replacing its allocator only
  before its first use, and not while other threads use it. The test therefore re-runs its own
  test binary, filtered to just itself (`--exact`). That fresh process installs the allocator
  before anything touches tree-sitter. This is the same under `cargo test` and nextest. Results
  on the 9.4 MB snapshot:
  - one window's tree: 1.94 MB live, 0.21× the file;
  - after 64 jumps across the file: the peak stays at 1.94 MB.

  As a mutation check, `Syntax::finish` was changed to leak each replaced window. Live bytes then
  reached 126 MB and the test failed.
