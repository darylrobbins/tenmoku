---
task_uid: "01m5wzvwcyzvgn9t4kqbz88jgq"
task: "T1197"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_full_queue_drops_the_oldest_and_reports_lagged loads its plugin with call_deadline 1000 ms (Limits { call_deadline: 1000 ms, ..Limits::default() } or the file's limits()), keeping event_queue at its default so assert_eq!(limits.event_queue, 256) still holds; only kiln/crates/kiln-plugin/tests/events.rs changes"
    required: true
    manual: false
  - id: "C2"
    text: "The test's assertions are unchanged (lagged 1, opened 0, lagged 744, the focus/selection and zero-capacity cases) and src/limits.rs keeps its 100 ms CALL_DEADLINE"
    required: true
    manual: false
  - id: "C3"
    text: "Evidence in the notes: inside kiln/ (its own workspace), with 4 CPU-burning background loads, 'cargo nextest run -p kiln-plugin --test events -E \"test(a_full_queue_drops_the_oldest_and_reports_lagged)\"' run 20 times, all 20 pass, each reporting 1 passed, with the command and per-run results; kiln clippy and fmt clean inside kiln/"
    required: true
    manual: false
---

# Flake: kiln-plugin events a_full_queue_drops_the_oldest_and_reports_lagged fails under load (call ran past its 100 ms deadline at plugin load)

# Flake: kiln-plugin events a_full_queue_drops_the_oldest_and_reports_lagged

## Evidence
Run 01m5wx549gp22f5wv8khvjamxw (first failure; load from a parallel full gate), ~/.local/state/regatta/runs/01m5wx549gp22f5wv8khvjamxw/test.fastedit.log: panicked at kiln/crates/kiln-plugin/tests/events.rs:64 (the `.unwrap()` of `Plugin::load_logging_to` inside `load`) with `Unloaded(Diagnostic { plugin: "events", reason: Timeout, message: "a call ran past its 100ms deadline" })`, after 1.1 s.

## Cause (test setting)
The test calls `load(Duration::ZERO, limits)` with `let limits = Limits::default()`, whose `call_deadline` is `CALL_DEADLINE` = 100 ms (kiln/crates/kiln-plugin/src/limits.rs:12). Loading and instantiating the wasm component (and any init call) runs under that deadline, so a loaded host can exceed 100 ms in a pure wall-clock call. Every other test in the file uses the file's `limits()` helper (call_deadline 1000 ms, "a deadline well past the fixture's slow calls"); this is the one test that uses the production default. It needs the default only for `assert_eq!(limits.event_queue, 256)`.

## Fix
Build the test's limits as `Limits { call_deadline: Duration::from_millis(1000), ..Limits::default() }` (or `limits()`), keeping `event_queue` at its default so the 256 assert stays. Test-only.

## Do not touch
src/limits.rs (the 100 ms production deadline), the events.wat fixture, and the other tests in events.rs. Do not loosen the `started.elapsed() < 1 s` push-speed assert beyond what the failure shows. kiln/ is its own cargo workspace: run clippy and fmt inside it. Overlap note: T1119 (blocked, kiln-plugin process test) is a different file.

## Implementation notes

- `a_full_queue_drops_the_oldest_and_reports_lagged` now uses the file's `limits()` (call_deadline 1000 ms, event_queue still the default 256). Only `kiln/crates/kiln-plugin/tests/events.rs` line 384 changed.
- Evidence (inside kiln/, 4 background `while :; do :; done` CPU burners): `cargo nextest run -p kiln-plugin --test events -E 'test(a_full_queue_drops_the_oldest_and_reports_lagged)'` run 20 times: runs 1-20 all exit 0, each "1 passed".
- `cargo fmt --all --check` and `cargo clippy -p kiln-plugin --tests` clean inside kiln/.
