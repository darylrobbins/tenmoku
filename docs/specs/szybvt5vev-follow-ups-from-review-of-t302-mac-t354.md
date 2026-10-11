---
task_uid: "01m4nqf1yzxdfzk2szybvt5vev"
task: "T34"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T302 (Mac T354)

Findings of the review of T302 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/hover.rs:438: Reply admission rejects valid command-triggered hover at a word's end. For `beta|;`, show_hover deliberately requests beta.start, but current.hovered.under remains beta.end. A normal reply with range beta.start..beta.end therefore fails this new check and is never displayed, even without pointer movement. The existing word-end command test uses a reply without a range, so it misses this regression.
  fix: Track the effective requested position for command-triggered hover and validate against it until pointer movement supplies a new position to validate. Add a regression test with the caret at beta.end and a reply whose range is exactly the beta word.
  instances: crates/fe-gpui/src/hover.rs:438

- F2 medium crates/fe-gpui/src/hover.rs:446: Rejecting a stale provider reply also removes valid diagnostic messages. If a diagnostic spans beta, the pointer moves within beta while a request is pending, and the narrower reply excludes the new position, hide_popover deletes the entire current state although its diagnostics still cover the pointer. The diagnostic disappears and stays absent until another pointer event.
  fix: Discard the non-covering provider content while retaining diagnostics that still stand at the latest position; only remove the entire popover when nothing valid remains. Add a delayed narrow-range reply test with a diagnostic spanning both pointer positions.
  instances: crates/fe-gpui/src/hover.rs:446
