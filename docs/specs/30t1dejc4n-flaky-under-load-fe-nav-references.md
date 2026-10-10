---
task_uid: "01m4m0qk01dw01h630t1dejc4n"
task: "T336"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-nav test references_encoding::a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work no longer asserts a fixed wall-clock limit, and passes 3 consecutive runs with the load average logged for each"
    required: true
    manual: false
  - id: "C2"
    text: "The hand-in shows the test failing against the quadratic prefix decoding (a temporary local revert of T304's fix in crates/fe-nav/src/references.rs, not committed)"
    required: true
    manual: false
  - id: "C3"
    text: "Only crates/fe-nav/tests/references_encoding.rs changes, apart from the task's own spec under docs/specs/ and its decision record"
    required: true
    manual: false
---

# Flaky under load: fe-nav references_encoding::a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work: check bounded work without a wall-clock limit

# Flaky under load: fe-nav references_encoding::a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work

The test (crates/fe-nav/tests/references_encoding.rs, from T304 / neon T4, landed 7327880) asserts
`elapsed.as_secs() < 5` around a search over one long legacy-encoded line with 2,000+ hits. On the Mac
under gate load it took 5.6 s and failed T219's and T307's finish gates (2026-10-10). The production code
is fine; the bound is wall-clock and load-dependent.

Make the test's check independent of machine load while keeping its teeth against the old behaviour
(decoding the whole line prefix again for every hit, quadratic in the line length). Options, worker's
choice: compare against a baseline measured in the same run (e.g. the time for the same search over a
line a quarter as long, asserting near-linear growth with generous slack), or count the decoded bytes
through a test-visible counter. Whatever the choice, show in the hand-in that the check still fails
against the quadratic prefix decoding (a temporary local revert of T304's fix, not committed).

## Implementation notes

The test now times the same search over the 2,000-hit line and a 500-hit line (a quarter as long), each as the least of 3 runs, and asserts the long one takes under 9x the short one: 4x for linear growth, 16x for quadratic. No wall-clock limit remains, so load scales both sides.

Against a temporary local revert of T304's fix (`Cursor::advance` restarting from the line's start for every hit, not committed) it fails: "4x the line took 17.9x as long (963ms, 17.2s)". Three passing runs on the fixed code had load averages 84.9, 108.3 and 102.1.
