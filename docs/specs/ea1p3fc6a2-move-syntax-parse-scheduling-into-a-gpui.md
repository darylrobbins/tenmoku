---
task_uid: "01m417waewdct797ea1p3fc6a2"
task: "T1"
rev: 1
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax test driver::tests::sync_never_parses_past_its_budget passes: with a fake clock, SyntaxDriver::sync stops parsing on the calling thread once the budget elapses and hands the parse to the Spawner"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test driver::tests::one_background_parse_at_a_time passes: syncs while a job is in flight spawn nothing"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test driver::tests::a_late_result_is_edited_forward_and_used passes"
    required: true
    manual: false
  - id: "C4"
    text: "fe-syntax test driver::tests::typing_during_a_parse_starts_one_follow_up passes: edits during a background parse cause exactly one follow-up parse"
    required: true
    manual: false
  - id: "C5"
    text: "fe-syntax test driver::tests::dropping_the_driver_or_switching_buffers_cancels_its_parse passes"
    required: true
    manual: false
  - id: "C6"
    text: "fe-gpui test tests::highlighting_follows_edits passes with EditorView driving syntax through SyntaxDriver::sync and a GPUI Spawner adapter"
    required: true
    manual: false
---

# Move syntax parse scheduling into a GPUI-free SyntaxDriver behind a Spawner seam

## Problem

The rules for when a reparse runs live in `EditorView::sync_syntax` (`crates/fe-gpui/src/view.rs:273`) and lean on GPUI's executor: parse in-frame within `SYNC_PARSE_BUDGET` (1 ms), otherwise start one background `ParseJob` via `cx.background_spawn`, hold it in `syntax_task`, then `Syntax::finish` the late result and `cx.notify()` to reparse what changed meanwhile. Only the headless GPUI test `tests::highlighting_follows_edits` (`crates/fe-gpui/src/tests.rs:573`) covers them, so the budget, single-flight and late-result rules are untested on their own, and anything else that wants to drive a `Syntax` (the AI backend, benches, T-27's "skip doomed sync reparses") has to copy them. The plan already lists a `Spawner` seam for this.

## Change

Move the state machine into fe-syntax as `SyntaxDriver` in a new `crates/fe-syntax/src/driver.rs`, exported from `crates/fe-syntax/src/lib.rs` with the `Spawner` trait.

- `Spawner` has an associated `Task` type (dropping it cancels the job) and `fn spawn(&mut self, job: ParseJob) -> Self::Task`.
- `SyntaxDriver<T>` owns the `Syntax`, the budget, an injectable clock (`with_clock(impl Fn() -> Instant)`; defaults to `Instant::now`) and the task in flight. `sync(&mut self, snapshot, &mut impl Spawner<Task = T>)` edits the tree, returns if a job is in flight, else reparses on the calling thread until the clock passes the budget, else spawns exactly one job. `finish(Parsed)` installs a result (edited forward by `Syntax::finish`) and clears the task; a result for another buffer is ignored. Switching to another buffer drops the job in flight. `syntax()` gives `&Syntax` to readers.
- To make the budget testable, `Syntax` gains `parse_until(out_of_time: impl FnMut() -> bool) -> bool` in `crates/fe-syntax/src/syntax.rs`; `parse_within` becomes a wrapper over it (benches still call `parse_within`) and `ParseJob`'s private `parse`/`parse_tree` take the callback instead of `Option<Duration>`, keeping T-08's `catch_unwind` handling intact.
- fe-gpui only adapts: `EditorView.syntax` becomes `Option<SyntaxDriver<gpui::Task<()>>>`, `syntax_task` is renamed `language_task` (language loading only), and a small `Spawner` impl in `view.rs` runs the job with `background_spawn`, then calls `SyntaxDriver::finish` and `cx.notify()` on the view. `element.rs` and `tests.rs` read the tree through `SyntaxDriver::syntax`.
- fe-syntax gains no dependency; GPUI stays out of it.

Tests go in `crates/fe-syntax/src/driver/tests.rs` (`#[cfg(test)] mod tests;` in `driver.rs`), using a manual spawner that queues jobs and runs them only when the test says, and a clock that advances a fixed tick per read.

## Prior attempt

Two uncommitted Paddock attempts exist; both are based on `76c28ef`, before T-08 (`14a80ca`) rewrote `ParseJob::parse` with `catch_unwind`, so their `syntax.rs` hunks need redoing by hand. Copy from them; don't modify those worktrees.

- **Use this one:** `/Users/daryl/code/fastedit.worktrees/paddock-task-4dc05e55-1` (about 230 lines). It has the design above nearly complete: `crates/fe-syntax/src/driver.rs` (untracked, 94 lines), the `parse_until` change, the fe-gpui `ParseSpawner` adapter in `view.rs`, the one-line `element.rs`/`tests.rs` changes, and six driver tests (`quick_parses_stay_on_the_calling_thread`, `sync_never_parses_past_its_budget`, `one_background_parse_at_a_time`, `a_late_result_is_edited_forward_and_used`, `typing_during_a_parse_starts_one_follow_up`, `dropping_the_driver_or_switching_buffers_cancels_its_parse`) with a `ManualSpawner`. They sit in the shared `crates/fe-syntax/src/tests.rs`; move them to `driver/tests.rs`. Run `git -C <worktree> diff` and read `driver.rs` there.
- `/Users/daryl/code/fastedit.worktrees/paddock-task-4b798fc3-1-sub-5ab5dd` tried a different shape (the driver owns the spawner, results come back over an mpsc channel polled on `sync`, plus `crates/fe-gpui/src/spawner.rs`) with four inline tests. Don't follow it; it's only there if a piece helps.
