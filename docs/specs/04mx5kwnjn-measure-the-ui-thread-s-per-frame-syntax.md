---
task_uid: "01m55bpxz2xd7fvr04mx5kwnjn"
task: "T146"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo bench -p fe-syntax --bench driver runs the cases rust/10k lines, rust/7 MiB and rust/20 MiB, each 600 frames paced at 8.33 ms with a real background-thread Spawner, and prints frames, frames with a parse in flight, parses landed, and p50, p99 and max of the per-frame SyntaxDriver time in ms"
    required: true
    manual: false
  - id: "C2"
    text: "A frame counts as having a parse in flight only if a background parse job has started executing on its worker thread and its result has not yet been delivered (not merely spawned or queued, and not completed). In rust/7 MiB a parse is in flight at the start of at least half the frames. rust/20 MiB, whose viewport-window parses take about 1 ms, reports the same count with no minimum, and PERF.md's subsection says that this case measures the windowed path, where a parse rarely overlaps a frame start."
    required: true
    manual: false
  - id: "C3"
    text: "PERF.md has a 'Typing during a background parse' subsection at the end of Syntax with one run's table, machine, commit, date and load, and states whether p99 \u003c= 2 ms and max \u003c= 8 ms hold; the M2 table's 'UI never blocks on a parse' row cites it; a miss names the costly call and a follow-up in the decision file"
    required: true
    manual: false
  - id: "C4"
    text: "Cargo.lock gains no [[package]]; crates/fe-syntax/Cargo.toml only gains the [[bench]] entry; cargo bench -p fe-syntax --no-run builds every bench"
    required: true
    manual: false
---

# Measure the UI thread's per-frame syntax work while a background reparse really runs: a fe-syntax driver bench at 120 Hz on 7 MiB and 20 MiB of Rust (Mac T814)

## Problem

M2's done-when includes "UI thread never blocks on a parse". PERF.md's M2 table marks it "by design" (a
frame tries at most 1 ms of reparse, then moves it to a background thread), and the Typing latency section
says the keystroke bench's `type/during-parse` case does not show it: under GPUI's deterministic test
dispatcher the pending parse never runs concurrently with the timed keystroke, so "a measurement with real
concurrent execution" is still needed (PERF.md, end of "Typing latency"). Nothing measures how long the
UI thread actually spends in `SyntaxDriver` per frame while a background parse of a large file really runs
on another thread, including `finish` installing a landed tree and the catch-up reparse after it.

## Change

New bench `crates/fe-syntax/benches/driver.rs`, `harness = false` (a `[[bench]] name = "driver"` entry in
`crates/fe-syntax/Cargo.toml`), a plain `main` with no criterion, like the keystroke bench's percentile
printing. No new dependency.

- A `Spawner` (`crates/fe-syntax/src/driver.rs`) whose `spawn` runs the `ParseJob` on a `std::thread` and
  sends the `Parsed` back over an `mpsc` channel; its `Task` is the thread's `JoinHandle` (`ParseJob` has no cancel
  flag, so a dropped task just detaches).
- The bench's main thread plays the UI thread, at 120 Hz: every 8.33 ms frame (sleep to the frame's
  deadline) it types one letter inside a random word (`Buffer::edit`, `Origin::User`), calls
  `SyntaxDriver::sync(&snapshot, visible, &mut spawner)` with the budget fe-gpui uses (1 ms,
  `SYNC_PARSE_BUDGET` in `crates/fe-gpui/src/view.rs`), then `finish` for any `Parsed` waiting on the
  channel (`try_recv`), then `sync` again if one landed, as the view does. Time the frame's driver calls
  (sync + finish + sync) together with `Instant`.
- Cases, 600 frames each (about 5 s): `rust/7 MiB` (generated Rust as in `benches/syntax.rs`, just under
  Rust's 7.5 MiB parse limit, so every reparse goes to the background) and `rust/20 MiB` (over the limit:
  window parses around a `visible` range that moves 60 rows every 10 frames), and `rust/10k lines` as the
  in-frame control.
- Print per case: frames, frames with a background parse in flight at their start, parses landed, and p50,
  p99 and max of the per-frame driver time in ms.

Budgets (the plan's typing and M2 targets): per-frame driver time p99 at most 2 ms and max at most 8 ms
(one 120 Hz frame) in `rust/7 MiB` and `rust/20 MiB`, with a parse in flight in at least half their frames
(else the case isn't exercising concurrency and must be fixed). If a budget is missed, find which call
costs it (likely `finish` editing a landed tree forward, or the catch-up `sync`), record it, and name a
follow-up card; don't fix it here.

PERF.md: a new "### Typing during a background parse (`cargo bench -p fe-syntax --bench driver`)"
subsection at the end of the "Syntax" section (before "Flat files"), with one run's table, machine, commit,
date and load, and update the M2 table's "UI never blocks on a parse" row to cite it.
