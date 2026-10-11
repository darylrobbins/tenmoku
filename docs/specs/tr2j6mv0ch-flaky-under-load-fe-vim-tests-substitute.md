---
task_uid: "01m4p9hkaq129mhftr2j6mv0ch"
task: "T54"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "flags_specials_and_bounds sets a higher time limit around its 100,000-match g substitute and restores DEFAULT_TIME_LIMIT after; its 10-s bound and every assertion are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "The fe-vim substitute tests pass 3 consecutive runs, with the 1-minute load average logged before each run"
    required: true
    manual: false
  - id: "C3"
    text: "Of the worker's own changes, only crates/fe-vim/src/tests/substitute.rs changes; Regatta's spec file in docs/specs/ is not the worker's change"
    required: true
    manual: false
---

# Flaky under load: fe-vim tests::substitute::flags_specials_and_bounds: raise the time limit around its 100,000-match g substitute too, as T43 did for the 200,000-row :%s

## Problem

`fe-vim tests::substitute::flags_specials_and_bounds` failed T42's gate (run 01m4p6wvt1r4dmqk2hdbpwhe9d, 20.1 s, panic at crates/fe-vim/src/tests/substitute.rs:275): the text was still "abab…". T43 (Mac T310, 17d65fa) added a 2-s wall-clock default limit to :s and searches, and raised the limit (600 s) only around the test's first part, the 200,000-row `:%s`. Its last part, `:s/\(a\)b/\u\1-/g` on one row of 100,000 matches (asserted under 10 s), still runs under the 2-s default, so under gate load it is Interrupted and leaves the row unchanged.

## Change

In `crates/fe-vim/src/tests/substitute.rs` only: in `flags_specials_and_bounds`, raise the time limit around the 100,000-match `g` substitute the same way T43 did for the `:%s` (`crate::search::set_time_limit(Duration::from_secs(600))` before, `DEFAULT_TIME_LIMIT` after), keeping its 10-s linear-work bound and all assertions unchanged. This extends the user's decision on T43 (that test may set a higher limit; the limit stays wall-clock) to the test's second part.

## Implementation notes

C2 evidence: `cargo test -p fe-vim tests::substitute` (the only substitute-module test is `flags_specials_and_bounds`), three consecutive runs, each passing, with the 1-minute load average (first field of /proc/loadavg) read immediately before each run:

- run 1: load 16.87 — `tests::substitute::flags_specials_and_bounds ... ok` (5.03s)
- run 2: load 15.99 — `tests::substitute::flags_specials_and_bounds ... ok` (6.30s)
- run 3: load 16.31 — `tests::substitute::flags_specials_and_bounds ... ok` (5.51s)

An earlier set of three runs (`cargo test -p fe-vim substitute`) at loads 0.90, 6.32 and 6.54 also all passed.
