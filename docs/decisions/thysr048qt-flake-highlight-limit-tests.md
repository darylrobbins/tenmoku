# highlight limit tests: zero or one-hour limits, not wall-clock ones

Status: **fixed**, measured 2026-10-13 (T621).

## Cause

`ran_out_latches_until_a_change` and `work_is_bounded_by_what_is_shown` let a
wall-clock limit (100 ms, and the 2 s default) decide whether a frame ran out.
Under load the first frame ran out before its first row was searched, or a
frame that should have finished ran out.

## Fix

`fe_vim::search::Deadline` reads only `Instant::now`, and a test clock there
is out of scope (T504 holds `search.rs`), so both tests set the existing
`TEST_LIMIT` to a value whose outcome load can't change: zero (runs out at the
first clock check) or an hour (never runs out). `ran_out_latches_until_a_change`
runs its first frame at zero, raises the limit to an hour, and checks the next
frame is empty only because of the latch, then that an edit and a new pattern
paint. `work_is_bounded_by_what_is_shown` runs at an hour.

## Load proof (C2)

Load proof (C2), run from the worktree on 2026-10-13, Apple Silicon, 14 cores. Script, verbatim:

  ```zsh
  BIN=$(cargo test -p fe-gpui --lib --no-run 2>&1 | rg -o 'target/debug/deps/fe_gpui-[0-9a-f]+' | head -1)
  N=$(sysctl -n hw.ncpu); pids=(); for i in $(seq $((N*2))); do yes >/dev/null & pids+=($!); done
  ok=0; bad=0
  for i in $(seq 20); do for t in element::highlight_limit_tests::ran_out_latches_until_a_change vim::search_tests::work_is_bounded_by_what_is_shown; do
   $BIN --exact $t 2>&1 | rg -q '^test result: ok. 1 passed; 0 failed' && ok=$((ok+1)) || bad=$((bad+1)); done; done
  kill $pids; echo "ncpu=$N yes=$((N*2)) ok=$ok bad=$bad"
  ```

  Result: `ncpu=14 yes=28 ok=40 bad=0`: 28 `yes >/dev/null` processes loading all 14 cores, each of the two tests run 20 times (`--exact`, so each invocation ran exactly one test: the pass check requires `1 passed; 0 failed`), 40 of 40 passed. An earlier run of the same loop without the one-test check also had 0 failures.
