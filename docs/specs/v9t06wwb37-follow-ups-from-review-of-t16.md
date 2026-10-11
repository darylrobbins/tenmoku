---
task_uid: "01m4pdxn37m1h1q3v9t06wwb37"
task: "T21"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T16

Findings of the review of T16 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal/src/pty_loop.rs:217: The requested shutdown and I/O-error cleanup coverage is missing. The new tests exercise child exit only, so they do not establish that buffered synchronized output reaches the grid before completion on these other termination paths.
  fix: Add deterministic tests that buffer synchronized text, terminate through Shutdown and an injected I/O error before the synchronization timeout, and verify the text is present before completion is published.
  instances: crates/fe-terminal/src/pty_loop.rs:152, crates/fe-terminal/src/pty_loop.rs:189, crates/fe-terminal/src/pty_loop.rs:192, crates/fe-terminal/src/pty_loop.rs:217

- F2 medium crates/fe-terminal/tests/session.rs:369: The long-output test does not establish that the final exit drain reaches its processing limit. Ordinary readable events can consume the 300,000 bytes before child exit, allowing the previous implementation to pass; it also does not exercise completion with an inherited writer still producing output.
  fix: Add a controlled regression that forces the child-exit drain to reach MAX_LOCKED_READ, and verify buffered text survives and completion occurs even when the final drain exhausts its cap with a continuing writer.
  instances: crates/fe-terminal/tests/session.rs:369
