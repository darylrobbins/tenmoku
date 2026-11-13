---
task_uid: "01m7ap10j8ghkavgqchd53jbb2"
task: "T612"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in cargo nextest run -p fe-runlog --test history pass: last8_keeps_the_newest_eight_oldest_first, flaky_needs_a_pass_and_a_fail_in_the_window, a_test_missing_from_a_run_keeps_its_window, save_is_atomic_and_load_round_trips, a_corrupt_file_loads_empty, the_cap_of_20000_drops_the_least_recently_run"
    required: true
    manual: false
  - id: "C2"
    text: "No test reads HOME or the real state directory (the directory is passed in)"
    required: true
    manual: false
---

# Test history: the last 8 results per test, flaky detection, persisted atomically in the state directory (Mac T1574)

# Test history: the last 8 results per test, and flaky detection

Spec: `docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`
("each test's last 8 results as small bars, so flaky tests stand out"; "failed
3 of its last 8 runs, likely flaky").

## Change (in `crates/fe-runlog`, after the parsers)

- `History` (`src/history.rs`): per project root, `record(run: &[(TestId,
  Outcome)])`, `last8(&TestId) -> Vec<Outcome>` (oldest first, at most 8),
  `flaky(&TestId) -> Option<Flaky { failed, of }>`: flaky when the window holds
  both a pass and a fail. A test missing from a run keeps its window. One run
  records a test once.
- Persistence: `<state_dir>/test-history/<hash of the root>.json`, written
  atomically (temp file then rename) after each recorded run; a missing or
  corrupt file loads empty and logs once; capped at 20,000 tests (the least
  recently run dropped).
- The state directory is passed in, so tests use a temp dir; nothing reads
  `$HOME`.

## Not in this card

Feeding it from a run (a later card), drawing it (a later card).

Platform: **Linux-OK**.
