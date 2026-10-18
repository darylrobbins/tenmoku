---
task_uid: "01m57ya9j0hxdkh57sb4zjv6cf"
task: "T202"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim tests::search::counted_search_is_bounded and the two tests in tests::substitute that raise the limit never set or restore the process-wide search time limit outside a shared lock-holding guard, and keep every existing assertion (including ROWS_SEARCHED \u003c= 6)"
    required: true
    manual: false
  - id: "C2"
    text: "Both pass 20 of 20 runs (cargo test -p fe-vim counted_search_is_bounded and substitute with -- --test-threads high, repeated -count style loop) while the workspace's heavy tests run in parallel, load stated per run, evidence in the decision file"
    required: true
    manual: false
  - id: "C3"
    text: "Only the three scoped fe-vim test files change; no production code; cargo test -p fe-vim and clippy -D warnings pass"
    required: true
    manual: false
---

# Flake: fe-vim search counted_search_is_bounded races the substitute tests on the process-wide search time limit; serialise the raise behind one test guard (Mac T980)

## Problem

`fe-vim tests::search::counted_search_is_bounded` (`crates/fe-vim/src/tests/search.rs:254`) failed 7 gate/post-merge
runs in the last 7 days. Its assertions are already counted work (`ROWS_SEARCHED <= 6`), not time; T495 raised the
search time limit to 600 s for it. The remaining flake is a race on the process-wide limit
(`fe_vim::search::set_time_limit`, an `AtomicU64`, `crates/fe-vim/src/search.rs:67`). The unit tests share one process
and run in parallel threads. `counted_search_is_bounded` sets 600 s and its `Restore` guard resets to
`DEFAULT_TIME_LIMIT` (2 s) on drop. The substitute test at `crates/fe-vim/src/tests/substitute.rs:255-281` does the
same set/reset around its 200,000-row `:%s` and its 100,000-match `:s///g`. Whichever test finishes first puts the 2 s
limit back under the other, still running. Under load the 40,000-match searches then pass 2 s, `Pattern::nth` returns
`None`, and the test fails on `assert_eq!(found, Some(...))`. The substitute test can fail the same way. Alone, each
passes.

## Change

Test-only. Add one helper in `crates/fe-vim/src/tests/mod.rs`: a `raised_time_limit()` that takes a static
`Mutex` (ignore poison), sets the limit to 600 s, and returns a guard that restores `DEFAULT_TIME_LIMIT` and releases
the lock on drop. Use it in `counted_search_is_bounded` (replacing `Restore`) and in the two blocks in
`substitute.rs` (the guard's scope covers each block; also restores on a panic). Keep every assertion, including the
`ROWS_SEARCHED <= 6` bound that still catches a quadratic regression. Do not touch production code or the
`tests/*.rs` integration binaries (separate processes, no race).

## Implementation notes

Added `raised_time_limit()` in `tests/mod.rs`: a static `Mutex` (poison ignored) plus a guard that sets 600 s and restores `DEFAULT_TIME_LIMIT` on drop. `counted_search_is_bounded` and both blocks of the substitute test use it.

Evidence (C2): 20 of 20 runs of `cargo test -p fe-vim --lib -- counted_search_is_bounded substitute --test-threads 64` passed (3 tests each: counted_search_is_bounded, the substitute test, and one more matching "substitute"). Each ran while a background `cargo test -p fe-vim -p fe-buffer -p fe-editor -p fe-display` (then `cargo test -p fe-vim`) ran the heavy workspace tests in parallel, on a shared host with other lanes active. One-minute load average at the start of each run (range 63.97 to 96.32):

```
run 1: pass, load1 at start 64.28, 3 passed
run 2: pass, load1 at start 71.54, 3 passed
run 3: pass, load1 at start 69.32, 3 passed
run 4: pass, load1 at start 66.91, 3 passed
run 5: pass, load1 at start 66.86, 3 passed
run 6: pass, load1 at start 64.95, 3 passed
run 7: pass, load1 at start 64.00, 3 passed
run 8: pass, load1 at start 67.51, 3 passed
run 9: pass, load1 at start 68.44, 3 passed
run 10: pass, load1 at start 63.97, 3 passed
run 11: pass, load1 at start 65.43, 3 passed
run 12: pass, load1 at start 76.94, 3 passed
run 13: pass, load1 at start 93.12, 3 passed
run 14: pass, load1 at start 96.32, 3 passed
run 15: pass, load1 at start 93.27, 3 passed
run 16: pass, load1 at start 83.93, 3 passed
run 17: pass, load1 at start 76.19, 3 passed
run 18: pass, load1 at start 70.19, 3 passed
run 19: pass, load1 at start 72.04, 3 passed
run 20: pass, load1 at start 68.18, 3 passed
```

`cargo test -p fe-vim` passes; `cargo clippy -p fe-vim --tests -- -D warnings` is clean.
