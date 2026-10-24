---
task_uid: "01m5nwr4409hf57760d8nt4pdw"
task: "T241"
rev: 1
type: "chore"
complexity: ""
criteria:
  - id: "C1"
    text: "The cause of the extra scope query is found and stated in the commit message; the test's counts no longer depend on timing (idle before each count, or the redundant query removed), with the assertion still exact."
    required: true
    manual: false
  - id: "C2"
    text: "No skip-under-load guard and no loosened count range is added."
    required: true
    manual: false
  - id: "C3"
    text: "Proof: 40 runs of the test at 4 parallel (cargo nextest, --retries 0) under 16 busy loops pass; the run count and command are in the commit message; the loops are killed afterwards."
    required: true
    manual: false
---

# Flake: fe-gpui sticky_scroll scopes_are_found_once_per_top_row_and_version counts 5 scope queries where it expects 4 in a full gate (passes alone)

## Problem
`fe-gpui element::sticky_scroll::tests::scopes_are_found_once_per_top_row_and_version` fails in full gates and passes alone.
Seen twice on argon on 2026-10-24: T237's gate (sticky_scroll_tests.rs:256, candidate 86f87ff) and the sync gate of mac/main 0f138277 (7ef2b4a3), run 01m5nvaybw20bb6vxf6ma54wkh:

    sticky_scroll_tests.rs:260: assertion `left == right` failed  left: 5  right: 4

i.e. at the last step (an edit at the same top row) `queries() - before` was 5, one scope query more than the test expects. The same test passed in the sync gate of bc65af8e a few hours earlier, and passes 3/3 alone on T237's branch and on its base.

## Cause to find
One scope query more than the test counts: something asks the scope provider an extra time between the steps (a second draw, a notify from a background task, or a version bump that lands late under load), so the count depends on timing. Find which and fix the test or the code so the count is deterministic: either wait for the app to be idle (`cx.run_until_parked()`) before each count, or count work that cannot be double-triggered. Do NOT loosen the assertion to a range that would hide a real double-query regression, and add no skip-under-load guard.

## Tests
Test-only chore unless the extra query turns out to be a real redundant query in `sticky_scroll.rs`; then fix it there and keep the assertion exact.
