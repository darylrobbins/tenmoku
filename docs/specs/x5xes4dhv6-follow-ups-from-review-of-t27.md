---
task_uid: "01m5w1fc4g2jahp0x5xes4dhv6"
task: "T358"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T27

Findings of the review of T27 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/disk.rs:164: The final check validates the current destination of `path`, but persist writes to the previously resolved `target`. If a symlink originally points to A, then before_commit replaces A with a FIFO and retargets the link to regular file B, overwrite passes this check and replaces the FIFO at A. Ordinary save instead reports ChangedOnDisk. Neither reports the required I/O error for the actual save target.
  fix: Also validate the resolved commit target before the final conflict check and persist. Add deterministic run_with tests for this transition in both save modes, verifying that the non-regular target survives and the temporary file is removed.
  instances: crates/fe-editor/src/disk.rs:164

- F2 medium crates/fe-editor/tests/save_not_a_file.rs:51: The FIFO regression silently passes when mkfifo cannot run or fails, so a green test can provide no coverage of the required FIFO behavior.
  fix: Require FIFO creation to succeed, with a useful failure message, or use a Unix FIFO creation API; do not return successfully when setup fails.
  instances: crates/fe-editor/tests/save_not_a_file.rs:51
