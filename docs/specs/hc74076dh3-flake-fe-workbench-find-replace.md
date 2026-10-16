---
task_uid: "01m50h97bdsg8bwghc74076dh3"
task: "T722"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "No test in find_replace_interrupted.rs depends on a wall-clock limit for its setup or its assertions: SETUP_LIMIT is ZERO or gone, and the two interruption tests interrupt after a counted number of regex calls through a doc(hidden) fe-vim search seam"
    required: true
    manual: false
  - id: "C2"
    text: "With the seam unset, fe-vim's behaviour is unchanged: cargo test -p fe-vim passes, and no non-test caller sets the seam"
    required: true
    manual: false
  - id: "C3"
    text: "find_replace_interrupted passes 20 of 20 runs while the workspace's heavy tests run in parallel, with the commands and results in the finish evidence"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench and clippy -D warnings pass"
    required: true
    manual: false
---

# Flake: fe-workbench find_replace_interrupted replace_on_an_expensive_row_steps_only_as_far_as_it_needs setup assert (selection 2..2 vs 2..3) still depends on the 1 ms wall-clock SETUP_LIMIT under load (T721 follow-up)

## Problem

T721 replaced the later wall-clock checks in `crates/fe-workbench/tests/find_replace_interrupted.rs` with work counters, but the setup of `replace_on_an_expensive_row_steps_only_as_far_as_it_needs` still depends on the 1 ms wall-clock `SETUP_LIMIT`. In T675's post-merge run 01m50g7bjvppt0mynhds214pqp (10-15, under load) line 251 `assert_eq!(selection, 2..3)` got `2..2`: when the typed query's search finishes inside 1 ms, vim mode leaves the caret at the match start instead of selecting it. It passed in the next post-check under the same load, so it is load- and timing-dependent.

## Change

Test only. Make the setup independent of the wall clock: select 2..3 explicitly after `replace_row`, or have the typed-query searches use a work limit (the `regex_calls()` counter / `Deadline` T721 added) or a zero limit so the result never depends on how fast the machine is. Give every other test in the file that uses `SETUP_LIMIT` the same treatment. Keep each test's intent and names.

## Rev 2: a work-count limit in fe-vim (lead respec, 10-15)

Rev 1's test-only fix (SETUP_LIMIT = ZERO, one_call() removed, a_search_over_several_rows counting regex calls) stays. Two tests can't be made deterministic from the test file, because `fe_vim::search::Deadline` is wall-clock only and a zero limit trips `current_match` before the row is searched:

- `a_row_cut_short_by_the_limit_is_an_interruption` (~line 281) needs the search to overrun a 400 ms limit;
- `a_match_found_past_the_limit_is_an_interruption` (~line 389) needs the deadline to expire during or right after the call that finds the match.

So add a test seam to `crates/fe-vim/src/search.rs`, next to T721's `regex_calls()`: a `#[doc(hidden)] pub` process-wide regex-call budget (for example `set_call_limit(Option<u64>)`, read once when a `Deadline` is made, like `time_limit()`), under which a `Deadline` passes once that many regex calls have been made since it started, as if its time ran out. `None` (the default) changes nothing: production behaviour and every existing test are untouched. `find_replace_interrupted.rs` is its own test binary, so a process-wide setting is safe there. Then rewrite those two tests to interrupt after a counted number of regex calls, keeping each test's name and what it proves (a row cut short is an interruption; a match found past the limit is an interruption, with the match found first).

This is test infrastructure plus test changes, so there is no regression test; prove it with repeated runs under load. Keep the change small: no new public API beyond the doc(hidden) seam, no change to how `:s` or search report `Interrupted`.

## Implementation notes

`SETUP_LIMIT` is now `Duration::ZERO`, so the searches a typed query starts always give up at once and the setup no longer depends on the wall clock.

Rev 2: `fe_vim::search::set_call_limit(Option<u64>)` is a `doc(hidden)` process-wide regex-call budget, read when a `Deadline` is made; with `None` (the default) nothing changes. Only `find_replace_interrupted.rs` sets it. `a_row_cut_short_by_the_limit_is_an_interruption` interrupts after 100 calls, and `a_match_found_past_the_limit_is_an_interruption` counts the calls of the step that finds the match, then budgets exactly that many. `one_call()` is gone, `SETUP_LIMIT` is zero, and `a_search_over_several_rows` counts regex calls, so no test in the file measures time.

The replace-all tests also run under no time limit (with a temporary zero one), not the 2 s default.

Load validation: a background loop ran `cargo test --workspace --no-fail-fast` repeatedly while the final `find_replace_interrupted` binary was run 20 times in a row (23:03:35 to 23:11:03). All 20 passed.
