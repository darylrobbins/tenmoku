---
task_uid: "01m4ss7jpn10ee3tthysr048qt"
task: "T621"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Neither test's outcome depends on machine load: each time limit it sets is either zero, so it runs out at the first clock check, or far longer than any run (an hour), so it never runs out; each test still checks the same behaviour as before (latching after running out, one limit per frame, work bounded by what is shown). A test-controlled clock in fe_vim::search::Deadline is out of scope (lead decision, 2026-10-13: crates/fe-vim/src/search.rs is held by T504)"
    required: true
    manual: false
  - id: "C2"
    text: "Each test passes 20 runs in a row while every core is loaded, with the command and result in the decision file"
    required: true
    manual: false
  - id: "C3"
    text: "No file outside the two test files changes, except this lane's own spec notes"
    required: true
    manual: false
---

# Flake: fe-gpui highlight_limit_tests::ran_out_latches_until_a_change and vim::search_tests::work_is_bounded_by_what_is_shown fail under load on their wall-clock time limit

Two fe-gpui tests from T363 (one search highlighting time limit per painted frame) depend on wall-clock time and fail on a loaded gate machine:

- element::highlight_limit_tests::ran_out_latches_until_a_change (crates/fe-gpui/src/element/highlight_limit_tests.rs). Failed in T601's gate run 01m4srczrmfw03xmy21k0zqdnc on base 4c3a2cc2: `assertion failed: !draw(&view, cx).is_empty()` at line 100. With TEST_LIMIT at 100 ms and the pattern `a.*z\|a` over 200 000-char rows, the first frame runs out of its limit before the first row's search ends when the machine is loaded, so nothing is painted. The same base passed in T564's gate run 01m4srcfk0528karw45swe57k3.
- vim::search_tests::work_is_bounded_by_what_is_shown (crates/fe-gpui/src/vim/search_tests.rs). Failed once in T564's fast check on the Mac under load (worker report, 2026-10-13 ~04:15Z).

Make both tests independent of machine load while they keep testing the same behaviour: drive the time limit from something the test controls (a deadline that expires after a set number of clock reads or regex calls, or a test clock), not from wall time. If that can't be done without changing production code in fe-gpui, stop and ask the lead for a respec rather than widening the scope.

Prove it with repeated runs under load (for example 20 runs of each test while `cargo build` or several `yes > /dev/null` processes load every core). A flake fix to the tests themselves can't fail at base on demand, so this is a chore with no regression test.

## Implementation notes

- `fe_vim::search::Deadline` reads only the wall clock and has no test hook outside fe-vim, so both tests drive the existing `TEST_LIMIT` override instead; no production file changes.
- `ran_out_latches_until_a_change`: a zero limit has run out at the frame's first clock read (`reaching` checks `expired()` on entry), so the first frame latches deterministically. The limit is then raised to an hour, so the next empty frame can only be the latch; the edit and the new pattern then paint. Rows shortened to 1 000 chars (no longer needed to outlast a limit).
- `work_is_bounded_by_what_is_shown`: limit set to an hour so the 2 s default can't run out.
- Load proof (C2), run from the worktree on 2026-10-13, Apple Silicon, 14 cores. Script, verbatim:

  ```zsh
  BIN=$(cargo test -p fe-gpui --lib --no-run 2>&1 | rg -o 'target/debug/deps/fe_gpui-[0-9a-f]+' | head -1)
  N=$(sysctl -n hw.ncpu); pids=(); for i in $(seq $((N*2))); do yes >/dev/null & pids+=($!); done
  ok=0; bad=0
  for i in $(seq 20); do for t in element::highlight_limit_tests::ran_out_latches_until_a_change vim::search_tests::work_is_bounded_by_what_is_shown; do
   $BIN --exact $t 2>&1 | rg -q '^test result: ok. 1 passed; 0 failed' && ok=$((ok+1)) || bad=$((bad+1)); done; done
  kill $pids; echo "ncpu=$N yes=$((N*2)) ok=$ok bad=$bad"
  ```

  Result: `ncpu=14 yes=28 ok=40 bad=0`: 28 `yes >/dev/null` processes loading all 14 cores, each of the two tests run 20 times (`--exact`, so each invocation ran exactly one test: the pass check requires `1 passed; 0 failed`), 40 of 40 passed. An earlier run of the same loop without the one-test check also had 0 failures.
