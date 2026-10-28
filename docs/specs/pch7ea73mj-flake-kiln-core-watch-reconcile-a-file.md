---
task_uid: "01m5wwp37ymyqg2gpch7ea73mj"
task: "T1195"
rev: 1
type: "chore"
complexity: ""
criteria:
  - id: "C1"
    text: "tests::watch_reconcile::a_file_in_a_directory_made_later_is_reported no longer fails when a loaded host delays the watcher's re-stat batches: its waits are bounded by watcher progress (e.g. keep receiving batches until one names the file, up to a generous bound of at least 120 s), not one 30 s recv"
    required: true
    manual: false
  - id: "C2"
    text: "The test still fails if the watcher never reports a file in a directory made after it started (it does not pass vacuously); the other watch_reconcile tests keep their meaning"
    required: true
    manual: false
  - id: "C3"
    text: "Evidence in the notes: the test run 20 times under load (e.g. alongside a parallel cargo build or nextest of another crate) with all passing, with the command and the per-run results; cargo nextest run -p kiln-core and cargo clippy (both inside kiln/, its own workspace) pass"
    required: true
    manual: false
---

# Flake: kiln-core watch_reconcile::a_file_in_a_directory_made_later_is_reported fails under load (43 s)

Main went red on 3aebbbbe (run 01m5wvfakhef5s96wfywqtep7r, the daemon's check after the stack 60 fast-forward). The only failure was kiln-core tests::watch_reconcile::a_file_in_a_directory_made_later_is_reported, which failed after 43 s while the full gate on the same commit had passed. The test (kiln/crates/kiln-core/src/tests/watch_reconcile.rs) waits for ConfigWatcher's re-stat batch, which comes every 3 s with no FSEvents involved, and gives each of its two waits a 30 s TIMEOUT (line 39). In a 536 s test run under load the second recv timed out. Triage reran it at main: it passed 4 of 4 confirmed runs (3-10 s; a fifth run's result was lost to an output filter), and fseventsd was normal (2.5 GB). Production behaviour looks correct, so this is a test-only fix: don't change kiln-core's watcher code. Prove it by repeated runs under load, since a test fix can't be made to fail at base on demand.

## Implementation notes

Test-only. Added `LOADED` (120 s) in `watch_reconcile.rs`; the first-batch wait and the loop deadline of `a_file_in_a_directory_made_later_is_reported` use it, the loop still receiving batches until one names the file. Other tests keep `TIMEOUT`. The test still fails (panics on the `expect`) if no batch names the file within the bound.

Evidence (C3), run inside `kiln/` with `PATH=$HOME/.cargo/bin:$PATH`:

Load and command sequence:

```
for i in 1 2 3 4 5 6 7 8; do (timeout 200 sh -c 'while :; do :; done' &) ; done   # 8 CPU burners
( cargo nextest run -p kiln-core --no-run; cargo clippy --workspace --all-targets ) &  # parallel build/lint
for i in $(seq 1 20); do cargo nextest run -p kiln-core a_file_in_a_directory_made_later_is_reported; done
```

Per-run results (all PASS), seconds: 1: 3.273, 2: 3.141, 3: 3.633, 4: 3.149, 5: 3.216, 6: 3.062, 7: 3.159, 8: 6.610, 9: 3.114, 10: 3.238, 11: 3.727, 12: 3.108, 13: 3.179, 14: 4.061, 15: 3.361, 16: 3.038, 17: 3.149, 18: 3.170, 19: 3.153, 20: 3.262. 20 of 20 passed.

After stopping the burners: `cargo nextest run -p kiln-core`: 123 passed, 1 skipped (100.7 s); `cargo clippy --workspace --all-targets`: no warnings or errors from this workspace; `cargo fmt --all --check`: clean.
