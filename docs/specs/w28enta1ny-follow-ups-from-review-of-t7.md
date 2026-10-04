---
task_uid: "01m41hp8nww57c4nw28enta1ny"
task: "T36"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T7

Findings of the review of T7 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-buffer/examples/history_memory.rs:134: Replace-all runs after the long-session buffer and history are dropped in the same process. Allocator pages retained from that scenario can enter the replace-all baseline and then satisfy its allocations without increasing RSS, understating its memory growth. Keeping the current inputs alive does not prevent this contamination.
  fix: Run each scenario in a fresh process, retaining its inputs through its final sample, and update the recorded release measurements.
  instances: crates/fe-buffer/examples/history_memory.rs:134, crates/fe-buffer/examples/history_memory.rs:98

- F2 low crates/fe-buffer/examples/history_memory.rs:93: Replace-all prints seven lines: a header, five pass results, and an undo summary. The spec explicitly requires one line per scenario.
  fix: Collect the five pass measurements and print them together with the total and undo result on one replace-all line.
  instances: crates/fe-buffer/examples/history_memory.rs:93, crates/fe-buffer/examples/history_memory.rs:113, crates/fe-buffer/examples/history_memory.rs:124
