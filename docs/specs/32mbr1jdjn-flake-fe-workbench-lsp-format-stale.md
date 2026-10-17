---
task_uid: "01m4vzwkhwfv12xq32mbr1jdjn"
task: "T712"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "stale_missing_and_slow passes 20 of 20 runs while the workspace's heavy tests run in parallel"
    required: true
    manual: false
  - id: "C2"
    text: "A real hang still fails the test within a bound"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench and clippy -D warnings pass"
    required: true
    manual: false
---

# Flake: fe-workbench lsp_format stale_missing_and_slow times out on its 10 s reply wait under load

## Problem

`crates/fe-workbench/src/lsp_format_tests.rs:158`, `stale_missing_and_slow`, failed T668's gate (run
01m4vwvhnezmp7wafx8xcsmvks, 2026-10-14 ~00:36Z, load 49-84) after 10.1 s: "timed out waiting for the reply",
raised by the shared `until` helper's 10 s clock (`crates/fe-workbench/src/tests/lsp.rs:470`). It failed the
same way in 5 other gate runs on 2026-10-13/14 on unrelated commits (0e213bda, 31b4cdb7, 93c2e208, f042d2ef,
a2a171f7). Alone it passes in 0.76 s. The wait is for the fake server to record the 300 ms-delayed
formatting request; under heavy load the fake LSP process round trip outlasts 10 s.

## Change

Make the wait robust under load without hiding a real hang: wait on the event the test needs (the request
recorded) with a budget that scales under load, or raise the `until` budget for fake-server round trips in
`crates/fe-workbench/src/tests/lsp.rs` only, keeping a bound that still fails a real hang. Don't weaken what
the test checks: edits from a reply that arrives after typing are dropped with nothing shown.

## Tests

Test-only fix: prove it with 20 runs while the workspace's heavy tests run in parallel.

## Implementation notes

The 10 s wait was not only load. With the budget raised to 60 s, 3 of 80 loaded runs still hung for the full 60 s while
the rest took about 2 s: the test typed `x` straight after `format_document`, so a loaded machine could run the key
ahead of the request and cancel the format before it was sent. Waiting for the request and a longer wall-clock delay
did not order the reply against the key either.

The test now orders them explicitly. `Lsp::with_fake_clock` (tests/lsp.rs) puts the fakes' reply delays on the
manager's `ManualClock`. The test starts `format_buffer` (the code `lsp.format_document` runs), sees the request
recorded, `wait_for_sleep`s until the fake's 300 ms delay is armed, types, `advance`s the clock to release the reply,
and awaits the format's own outcome (`FormatOutcome::Unchanged`, bounded by `Lsp::until`'s 60 s) before asserting the
text is unchanged and no toast is shown. No sleep stands in for completion. `Lsp::until` keeps a 60 s bound for
fake-server round trips on a loaded machine, so a real hang still fails.

Load evidence (2026-10-16 UTC). The tree was clean at the commit below; the test binary was built from it with
`cargo test -p fe-workbench --lib --no-run`; `cargo test --workspace --no-fail-fast` was then started and, once its
first test binary was running, the fe-workbench binary was run 30 times with `stale_missing_and_slow`. `test-processes`
counts live `target/debug/deps/<crate>-<hash>` processes at the end of each run (the workspace's test binaries plus
this one); the last line shows the workspace run still going after run 30. Raw log:

```
HEAD=ee6683894e5180e1d90eec38a78ffac0a33497b0 crates-tree=c984ffced6f88ea1d4e5e60a8e9436a57d2db370 clean=       0
binary=target/debug/deps/fe_workbench-e26c324ed2892284 sha=c8f808d543ac built=18:26:52
run 1 start=22:28:53 end=22:28:54 rc=0 finished in 0.57s test-processes=4
run 2 start=22:28:54 end=22:28:55 rc=0 finished in 0.37s test-processes=3
run 3 start=22:28:55 end=22:28:55 rc=0 finished in 0.46s test-processes=7
run 4 start=22:28:55 end=22:28:56 rc=0 finished in 0.30s test-processes=5
run 5 start=22:28:56 end=22:28:56 rc=0 finished in 0.23s test-processes=3
run 6 start=22:28:56 end=22:28:57 rc=0 finished in 0.35s test-processes=5
run 7 start=22:28:57 end=22:28:57 rc=0 finished in 0.28s test-processes=3
run 8 start=22:28:57 end=22:28:58 rc=0 finished in 0.36s test-processes=5
run 9 start=22:28:58 end=22:28:58 rc=0 finished in 0.20s test-processes=3
run 10 start=22:28:58 end=22:28:58 rc=0 finished in 0.19s test-processes=3
run 11 start=22:28:58 end=22:28:59 rc=0 finished in 0.27s test-processes=3
run 12 start=22:28:59 end=22:29:00 rc=0 finished in 0.53s test-processes=7
run 13 start=22:29:00 end=22:29:00 rc=0 finished in 0.38s test-processes=5
run 14 start=22:29:00 end=22:29:01 rc=0 finished in 0.23s test-processes=3
run 15 start=22:29:01 end=22:29:01 rc=0 finished in 0.45s test-processes=7
run 16 start=22:29:01 end=22:29:02 rc=0 finished in 0.47s test-processes=6
run 17 start=22:29:02 end=22:29:02 rc=0 finished in 0.23s test-processes=3
run 18 start=22:29:02 end=22:29:03 rc=0 finished in 0.39s test-processes=6
run 19 start=22:29:03 end=22:29:04 rc=0 finished in 0.46s test-processes=6
run 20 start=22:29:04 end=22:29:04 rc=0 finished in 0.24s test-processes=3
run 21 start=22:29:04 end=22:29:05 rc=0 finished in 0.51s test-processes=6
run 22 start=22:29:05 end=22:29:05 rc=0 finished in 0.47s test-processes=3
run 23 start=22:29:05 end=22:29:06 rc=0 finished in 0.38s test-processes=5
run 24 start=22:29:06 end=22:29:06 rc=0 finished in 0.44s test-processes=7
run 25 start=22:29:06 end=22:29:07 rc=0 finished in 0.31s test-processes=3
run 26 start=22:29:07 end=22:29:07 rc=0 finished in 0.26s test-processes=5
run 27 start=22:29:07 end=22:29:08 rc=0 finished in 0.51s test-processes=9
run 28 start=22:29:08 end=22:29:09 rc=0 finished in 0.27s test-processes=3
run 29 start=22:29:09 end=22:29:09 rc=0 finished in 0.26s test-processes=5
run 30 start=22:29:09 end=22:29:10 rc=0 finished in 0.29s test-processes=3
workspace-tests-still-running=       2 running-lines=11 ok-tests-so-far=89 at 22:29:10
```
