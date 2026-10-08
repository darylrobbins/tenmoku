---
task_uid: "01m4cvbsm6e2c50mdmb3bgzssm"
task: "T191"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T112

Findings of the review of T112 that don't block the merge (below merge.block_severity):

- F4 medium crates/fe-workbench/src/references_panel.rs:352: The new history predicate treats every listed reference's end as an automatic panel landing, including references never opened. After shift-F12, manually move the caret to the end of another listed occurrence, then open a panel result: record becomes false, so Back skips the manually chosen position. Before the first panel open, this can leave Back with no entry at all. The same false classification occurs for clamped ends. This regresses the requirement to record a caret the user moved; opening_records_where_the_caret_moved_to only checks a non-reference position.
  fix: Distinguish actual panel-initiated landings from coincident reference offsets, preserving pending loads and their destination panes. Record user-moved positions even when they match another reference's raw or clamped end. Add regressions for moving to an unvisited reference's end before the first open and between opens, while retaining the pane-switch and pending-read tests.
  instances: crates/fe-workbench/src/references_panel.rs:352, crates/fe-workbench/src/references_panel.rs:353
