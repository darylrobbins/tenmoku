---
task_uid: "01m72r28rzfsrkcf7qv9sdate3"
task: "T565"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_match_boundary_atom_in_a_pattern_name_is_not_prefiltered starts with let _limit = unhurried(); so its three pattern-name searches run under the 60 s limit, not the 2 s DEFAULT_TIME_LIMIT; every assertion stays, and no file changes except crates/fe-vim/tests/delimiters_regressions.rs and the task's decision file under docs/decisions/"
    required: true
    manual: false
  - id: "C2"
    text: "Every other test in crates/fe-vim/tests/delimiters_regressions.rs that runs a pattern-name search without unhurried() either gets it or is named in the decision file as left alone on purpose (for example pattern_pairing_past_many_other_tags_finishes_in_time, which is about the limit), with the reason"
    required: true
    manual: false
  - id: "C3"
    text: "While a cargo build or busy loops hold the load average at 2x the cores or more, cargo nextest run -p fe-vim --test delimiters_regressions --retries 0 passes 20 times in a row; the command, each run's load average and the result are in the decision file; any load generator registers its trap before starting and leaves no process behind"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-vim passes"
    required: true
    manual: false
---

# Flake: fe-vim delimiters_regressions a_match_boundary_atom_in_a_pattern_name_is_not_prefiltered fails under load (runs without the 60 s unhurried() search limit) (Mac T1719)

# Flake: delimiters_regressions a_match_boundary_atom_in_a_pattern_name_is_not_prefiltered under load

## Problem

On host argon, a Linux gate of a batch that does not touch fe-vim failed once on
`fe-vim` integration test `delimiters_regressions`
`a_match_boundary_atom_in_a_pattern_name_is_not_prefiltered`; the rerun passed, and the test passed on the
same base elsewhere.

## Cause (likely, to confirm)

A tag-name pattern search runs on a wall-clock deadline, `fe_vim::search::DEFAULT_TIME_LIMIT` (2 s, `crates/fe-vim/src/search.rs`). A search that passes it
leaves the buffer untouched, which reads as a missing text object (`dit` is a no-op). Sibling tests in
`crates/fe-vim/tests/delimiters_regressions.rs` call `unhurried()` (60 s limit) for this reason; this test
(three `\zs`/`\ze` pattern names, each tried at every span, no prefilter) does not, so a stalled thread under
load can pass 2 s. This is the same shape as T331 (commit 5625ac8b4: `set_time_limit(60 s)` in
`delimiters_oracle.rs`, after T622 did it for the siblings).

## Change (test only)

Add `let _limit = unhurried();` as the first line of
`a_match_boundary_atom_in_a_pattern_name_is_not_prefiltered`. Also audit the other tests in
`delimiters_regressions.rs` that lack `unhurried()` and run a pattern-name search (the tests from
`distinct_joined_chars_in_a_pattern_name_keep_their_own_requirements` to `a_bare_lt_before_an_empty_closing_tag_keeps_the_boundary`, and
`pattern_pairing_past_many_other_tags_finishes_in_time`, which may deliberately test the default limit): give
each one that is not about the limit itself the same guard, and say in the decision file which were left alone and why.

If the cause turns out to be in `crates/fe-vim/src/` (a deadline that ignores `set_time_limit`, as T1499
checked and found it did not), change nothing outside the decision file and block the task for a respec as a bugfix.

## Proof

A test-only fix cannot fail at base on demand: prove it under load. While a `cargo build` (or busy loops
bringing the load average to at least twice the cores) runs, run
`cargo nextest run -p fe-vim --test delimiters_regressions --retries 0` 20 times in a row. Any load generator registers its
`trap` before starting and leaves no process behind. Record the command, load average and result in the decision file.
