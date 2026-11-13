---
task_uid: "01m7ap100d14z9s01wjfvcrvxg"
task: "T611"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::run_process (cargo nextest run -p fe-workbench) pass: streams_lines_in_order_in_batches (at most one delivery per 16 ms), a_nonzero_exit_finishes_the_run_failed_with_its_code, cancel_kills_the_whole_process_group (a child sleep is gone), output_is_capped_at_8_mib_and_marked_truncated, an_untrusted_root_refuses_with_the_task_message"
    required: true
    manual: false
  - id: "C2"
    text: "No test depends on cargo or the network (fixtures are sh -c scripts)"
    required: true
    manual: false
---

# Background runner for Runs: spawn a process off the main thread, stream its lines to a parser, cancel by process group, cap the kept output (Mac T1572)

# A background runner that feeds a Run from a process's output

Spec: the Runs rows for tests and builds need a process whose output the
workbench can read as it arrives. A task in a terminal tab (`task_run.rs`) is a
pty, which gives colour and prompts, not parseable lines; test and build
parsers want plain pipes.

## Change

- `crates/fe-workbench/src/run_process.rs` (new): start a program (argv, cwd,
  env) off the main thread with piped stdout and stderr; deliver lines to a
  `LineSink` (a trait the parsers implement: `fn line(&mut self, stream:
  Stream, text: &str, at: Duration) -> Vec<RunUpdate>`) in batches of at most
  one per 16 ms; apply the updates to the `Run` (T1570: progress, summary).
- Output kept for the Output view: both streams interleaved, capped at 8 MiB
  (the oldest dropped, a `truncated` flag set).
- Cancel: SIGTERM to the process group, SIGKILL two seconds later; the Run
  becomes `Cancelled`. Exit status finishes the Run (zero Passed, else
  Failed { code }).
- Only in a trusted root: an untrusted folder refuses with the same message as
  `task_run::UNTRUSTED`.
- No parsers here; a sink that records lines is enough for the tests.

## Not in this card

Test and cargo parsers, any command that uses it, any UI.

Platform: **Linux-OK** (tests run `sh -c` fixtures).

## Implementation notes

- `RunProcess::start(spec, sink, runs, cx)` resolves the folder off the main
  thread, then checks `kiln_shell::trust::decided(folder) == Some(true)` just
  before the run begins: an undecided, untrusted, missing or symlinked-out
  folder is `Refused::Untrusted(task_run::untrusted_message())`, with no run
  begun and nothing started. It answers an `Entity<RunProcess>` holding the
  kept output (`RunOutput`), the run's id and its end.
- The sink runs on the runner's thread, not the main one (`LineSink: Send`).
  Besides `line`, it has a provided `batch_end`, called once each batch: the
  test uses it to time batches, and a parser may sum up there.
- A batch's updates are applied to the run once a batch. A run whose sink
  never gave a summary ends with `RunSummary::Exit(code)`, as task tabs do.
- Review round 1: the readers feed a bounded queue (64 lines), a batch takes
  in at most `BATCH_CAP` (512 KiB) before reading stops, and handing a batch
  to the main thread waits only while not cancelled; on a cancel the group's
  `SIGKILL` runs on a thread of its own, `GRACE` after the `SIGTERM`. A batch
  that dropped lines to fit the cap replaces the older output, so what's kept
  is always the newest lines without a gap.
- Review round 2: signalling moved to a stopper thread of its own, started
  before the program (none, and nothing starts: `nothing_starts_without_its_threads`
  injects the failure). It sends `SIGTERM` as soon as the run is cancelled and
  `SIGKILL` `GRACE` later, so neither waits for a blocked sink or a stalled
  main thread; the runner's thread reaps the leader only once the stopper is
  done. `cancel(cx)` marks the run `Cancelled` on the main thread at once, and
  a later end from the runner's thread is ignored.
- Lines longer than 64 KiB come as several; the cap counts each line's bytes
  plus a newline. The sink sees every line, whatever the cap drops.
- Dropping the `RunProcess` stops the program as `cancel` does; its run is
  finished `Cancelled` as it's released. Quitting sends each running group `SIGTERM`.
  A sink that panics stops the group, its run `Failed { code: None }`.
- A run ends when its leader has exited and both pipes have closed: a
  background child still holding stdout keeps it running, as a pipe would.
- Scope widened to `lib.rs` and `tests/mod.rs` to declare the modules.
