---
task_uid: "01m6wt0x4ege6wagxxe7dqwq1e"
task: "T331"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The decision file names the cause of the empty dit result under load on dit_pattern_name_starting_with_a_keyword_atom, with the evidence (a reproduction or the code path)"
    required: true
    manual: false
  - id: "C2"
    text: "If the cause is in the test, the fix is in crates/fe-vim/tests/delimiters_oracle.rs only, and cargo nextest run -p fe-vim --test delimiters_oracle --retries 0 passes 20 times in a row while another cargo build runs, with the command and result in the decision file"
    required: true
    manual: false
  - id: "C3"
    text: "If the cause is in crates/fe-vim/src/, nothing outside the decision file changes and the task is blocked for a respec as a bugfix"
    required: true
    manual: false
---

# Flake: fe-vim delimiters_oracle delimiters_matches_neovim fails under load on dit_pattern_name_starting_with_a_keyword_atom despite T622's 60 s search limit (Mac T1499)

`fe-vim::delimiters_oracle delimiters_matches_neovim` failed once in the stack 93 macOS gate (2026-11-07, load 80-150) and passed on a single rerun:

    delimiters: 1 of 292 cases differ from nvim:
    dit_pattern_name_starting_with_a_keyword_atom ("<\\ka>x</ba>" at (0, 0), keys "dit"): got ("<\\ka>x</ba>", (0, 0), "normal", {}, None, {}, None), nvim ("<\\ka></ba>", (0, 5), ...)

So fe-vim's `dit` found no tag block (a no-op) where nvim deletes "x". Log: .worktrees/gate-stack-93.test.run2.log:4583 on the Mac (removed with the stack-93 worktree; the text above is the whole failure).

T582/T622 already raised the search time limit for this test to 60 s (fe_vim::search::set_time_limit) after a load failure on yit_count_2_over_pattern_names. This run took 26.6 s for the whole test, so a 60 s limit cannot have tripped, unless the limit does not reach the code path `it`/`at` uses for tag names (for example a separate deadline, a default limit read elsewhere, or a per-thread value the matcher never sees). Treat it as real until shown otherwise.

Do: find why this case can come back empty under load. Look at how the tag text object compiles and runs the tag-name pattern (`\k` atom in a pattern name), and whether any search deadline or budget on that path ignores set_time_limit. Reproduce under load (e.g. run the test 30 times while another cargo build runs). If the cause is in the test or its setup, fix it in the test file. If the cause is in production code under crates/fe-vim/src/, stop, write what you found in the decision file, and block the task asking the lead to retype it as a bugfix with a regression test: that change is out of this chore's scope.

## Implementation notes

**Cause (C1).** The cause is in the test. T582 (310c94a8) removed T622's `set_time_limit(60 s)` line from `delimiters_matches_neovim`, so the test ran on `DEFAULT_TIME_LIMIT` (2 s) again. `it`/`at` with a pattern name build a `Clock` (`crates/fe-vim/src/delimiters.rs`, `Clock::new`) that is `Deadline::new()` outside `cfg(test)`. That deadline reads `time_limit()` and is wall-clock, so an integration test gets the 2 s default. A thread stalled for over 2 s under load (80-150) makes the deadline expire mid-command. The text object then fails, and `dit` is a no-op with the buffer, cursor and registers unchanged. That is the failure's exact shape. So the 60 s limit was not ignored on this path; it was no longer set.

**Reproduction.** With `set_time_limit(Duration::ZERO)` as the first line of the test, 153 of 292 cases differ. They include `dit ("<p>hello</p>" ...)`, with the same "got = unchanged buffer, nvim = deleted" output as the report. Restoring the 60 s limit fixes it.

**Fix (C2).** `crates/fe-vim/tests/delimiters_oracle.rs` sets the limit to 60 s again, with a comment saying why. No production change. The 2 s expiry is intended behaviour (`Interrupted`).

**Verification.** While `CARGO_TARGET_DIR=$PWD/target/loadbuild cargo build -p fe-vim --all-targets` ran in the background:
`for i in $(seq 20); do cargo nextest run -p fe-vim --test delimiters_oracle --retries 0; done` gave 20 PASS and 0 FAIL.

C3 does not apply: nothing under `crates/fe-vim/src/` changed.
