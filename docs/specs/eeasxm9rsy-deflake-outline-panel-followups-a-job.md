---
task_uid: "01m4eg871rzgc3breeasxm9rsy"
task: "T202"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The test passes 30 consecutive runs under artificial CPU load (e.g. cargo nextest run -p fe-workbench a_job_for_a_language_switched_away_from_lands_unshown with stress or a parallel build running), recorded in the finish notes with the load average"
    required: true
    manual: false
  - id: "C2"
    text: "The test still asserts that Python's job starts, that it lands unshown after the switch back to Rust, and that the Rust outline stays; no other test's assertions change"
    required: true
    manual: false
  - id: "C3"
    text: "The finish notes name the confirmed cause of the flake"
    required: true
    manual: false
---

# Deflake outline_panel_followups::a_job_for_a_language_switched_away_from_lands_unshown under load

`fe-workbench` `tests::outline_panel_followups::a_job_for_a_language_switched_away_from_lands_unshown`
fails under machine load (load average ~110-140, four lanes building at once), passing otherwise:

    panicked at crates/fe-workbench/src/tests/outline_panel_followups.rs:84:5:
    assertion `left == right` failed: Python's job runs
      left: 1
     right: 2

Seen twice on 2026-10-08: the main health run on d99064c (turning main red and blocking every
landing) and T193's re-integration gate (run 01m4efwdmkwhj3xbhdz8jrhg53), a change only to
fe-syntax. Both times every other test passed.

The test's `set_language` helper assumes that "the editor parses within its render", so a
language switch starts the panel's outline job in the same draw. Likely cause: the in-render parse
is time-budgeted, and under load it overruns, falls back to a background parse, and the panel sees
no new syntax in that frame, so no job starts. Confirm the actual cause before fixing.

## Change

Make the test deterministic without weakening what it checks (F3 of T78's review: a job for a
language switched away from lands unshown, and the Rust outline stays). Prefer waiting on the
condition the test relies on (e.g. the editor's syntax being Python at the new version before
counting jobs) over raising a time budget. If the production code has a real ordering bug under a
slow parse, fix it in scope and say so in the finish notes.

Don't change any other test's assertions.

## Implementation notes

- Cause (confirmed): `SYNC_PARSE_BUDGET` (1ms) in `fe-gpui/src/view.rs` bounds the in-render parse. Setting it to 1ns reproduced the exact failure (`Python's job runs`, left 1 right 2). Under load the parse overruns and finishes on a background thread, so the panel sees no new syntax that frame and starts no job. No production ordering bug.
- Fix: a test-only gate on the panel's outline jobs (`hold_jobs`/`release_jobs`). The test holds Python's job, so waiting for either language's parse (`set_language` runs the executor and redraws until the syntax is parsed at the text's version) can't land it early; then it releases the job, which lands unshown as before. No retries, and the test passes with the budget forced to 1ns (every parse in the background). Assertions unchanged. Adds `futures` as a fe-workbench dev-dependency for the gate.
- Earlier run: 30 consecutive passes, 0 failures, with 12 `yes` burners, load average 87 at start and 220 at end for the final version; load average 48 at the end.
