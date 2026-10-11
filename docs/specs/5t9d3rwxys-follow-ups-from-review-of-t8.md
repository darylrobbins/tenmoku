---
task_uid: "01m4paehajta1ca35t9d3rwxys"
task: "T14"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T8

Findings of the review of T8 that don't block the merge (below merge.block_severity):

- F10 medium crates/fe-gpui/src/view/ime.rs:354: Cancelling a composition does not restore visibility of the buffer caret. For example, compose a wide string at offset 100 in an unwrapped 2000-character row, draw it, then send an empty mark or commit: the composition's horizontal scroll persists with autoscroll=false, leaving the real caret off-screen. Cancellation through a no-op command, stale-vim cleanup, or blur has the same problem; existing cancellation tests use short rows.
  fix: Request caret autoscroll when dropping a composition without an edit, preferably through a shared cancellation helper. Add regressions for empty mark, empty commit, and a no-op movement after an overflowing composition on an unwrapped row shorter than MAX_SHAPED_ROW_BYTES.
  instances: crates/fe-gpui/src/view/ime.rs:218, crates/fe-gpui/src/view/ime.rs:235, crates/fe-gpui/src/view/ime.rs:244, crates/fe-gpui/src/view/ime.rs:354, crates/fe-gpui/src/view.rs:953

- F9 medium crates/fe-gpui/src/element.rs:621: The overflow relayout can move a long row's buffer-based shaped slice beyond a wide composition anchored in its middle. The resulting frame loses the inlay, underline, and composition caret, and past_edge then returns None instead of applying the fallback. The spec explicitly assigns this case to follow-up.
  fix: In the follow-up, retain the composition anchor in the shaped slice or preserve its shaped geometry while shifting the frame, and add a wide mid-row composition regression on a row longer than MAX_SHAPED_ROW_BYTES.
  instances: crates/fe-gpui/src/element.rs:621
