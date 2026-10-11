---
task_uid: "01m4pbre130ms2x56zt0w6prr9"
task: "T16"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T10

Findings of the review of T10 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal/src/session.rs:274: The flush requires an observed child exit and another reader call. Shutdown and I/O-error exits bypass those conditions, so the reaper still discards buffered synchronized output before publishing Exited. The documentation explicitly leaves this part of the requested F6 fix unresolved; after an error, the Terminal remains available for snapshots.
  fix: Arrange an upstream-supported parser flush on every event-loop termination path before completion is published or parser state is discarded, and add coverage for error and shutdown cleanup.
  instances: crates/fe-terminal/src/session.rs:274, docs/decisions/terminal.md:68

- F2 medium crates/fe-terminal/src/session.rs:278: The synthetic escape sequence is injected even when no synchronized update is active, and it is not always harmless. For example, a child exiting after an unterminated OSC title or OSC 52 payload now dispatches that command: VTE's OSC parser dispatches on the injected ESC. This introduces title or clipboard events that the child's actual output never completed.
  fix: Flush through the parser's supported stop_sync operation instead of appending terminal input, preserving incomplete ordinary escape sequences. Add regressions for unterminated OSC title and clipboard commands without synchronized updates.
  instances: crates/fe-terminal/src/session.rs:278, docs/decisions/terminal.md:67

- F3 medium crates/fe-terminal/src/session.rs:272: The wrapper assumes the final exit drain will read until EOF or WouldBlock. Alacritty's pty_read instead returns once it processes 65,535 bytes, and its child-exit branch calls it only once before publishing completion. If that limit is reached, for example while an inherited PTY writer continues producing output, the synthetic terminator is never requested and already-read synchronized text is still discarded.
  fix: Make parser flushing independent of reaching a drained reader state, and verify completion when the final drain reaches its processing limit.
  instances: crates/fe-terminal/src/session.rs:272
