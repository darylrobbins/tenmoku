---
task_uid: "01m5yapxy0mta4fy6efwx2b401"
task: "T372"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T501 (Mac T1194)

Findings of the review of T501 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/scripted_ai.rs:512: Finish reports success when a deferred write fails. For example, push a lone carriage return over a selection, edit inside that target, then finish: EditStream::finish silently discards its Conflict error, and the driver returns Finished(None). A deferred flush into an undone transaction similarly hides Undone.
  fix: Track and flush deferred output through an error-returning path, propagating Conflict or Undone as StepError and dropping the stream. Add tests for both failed completion cases.
  instances: crates/fe-editor/src/scripted_ai.rs:512

- F2 medium crates/fe-editor/src/scripted_ai.rs:515: Completion decoration tracking assumes the buffer grows. A stream containing only a deferred carriage return can replace a longer selection with one newline, so finish changes text but creates no decoration. When it inserts into an empty selection, a decoration is created, but the new transaction is never added to marked, so undo leaves its collapsed mark and redo cannot restore it correctly.
  fix: Determine the actual landed range after the deferred flush, mark it regardless of net buffer growth, and register any transaction first created during finish. Test replacement, insertion, undo and redo.
  instances: crates/fe-editor/src/scripted_ai.rs:515

- F3 medium crates/fe-editor/src/scripted_ai.rs:698: Touched decorations have no transaction ownership: mark removes every contained decoration, and catch_up removes decorations by spatial span. Stream a complete line, accept a proposal replacing that line, then undo the accepted hunk: the original streamed text returns, but its decoration was discarded during acceptance and the remaining hunk decoration is removed during undo. The still-applied stream therefore loses its touched-row coverage.
  fix: Retain decoration records associated with their transactions and update or remove only the affected transaction's records, preserving or reconstructing marks for restored underlying streams. Add an overlapping stream/proposal undo and redo test.
  instances: crates/fe-editor/src/scripted_ai.rs:698, crates/fe-editor/src/scripted_ai.rs:662

- F4 medium crates/fe-editor/src/scripted_ai.rs:499: A successful push is decorated only if it produces a transaction. If the primary selection contains 'old' and the first chunk is also 'old', EditStream successfully establishes its region without creating a transaction, and touched remains empty. This violates the requirement to cover the stream's region after each push.
  fix: Mark the region after every successful push independently of transaction creation; record transaction ownership when a transaction exists. Add a same-text first-chunk test.
  instances: crates/fe-editor/src/scripted_ai.rs:499
