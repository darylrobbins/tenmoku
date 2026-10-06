---
task_uid: "01m48rked6r4jwp9whbvmhj693"
task: "T97"
rev: 3
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "With one open tab, next_tab and previous_tab reactivate it and clear the loading placeholder while another file's read is pending"
    required: true
    manual: false
  - id: "C2"
    text: "With one open tab, next_tab and previous_tab reactivate it and clear the error placeholder after another file's read fails"
    required: true
    manual: false
  - id: "C3"
    text: "New tests live in crates/fe-workbench/src/tests/pane_cycle.rs and fail at base"
    required: true
    manual: false
---

# Follow-ups from review of T63

Findings of the review of T63 that don't block the merge (below merge.block_severity):

- F6 medium crates/fe-workbench/src/pane.rs:219: With exactly one existing tab, next_tab and previous_tab cannot restore its editor while another file loads or after that read fails: cycle only activates a tab when count > 1. The loading/error placeholder therefore remains and focus stays on the pane, unlike recovery with multiple tabs.
  fix: Cycle whenever at least one tab exists, allowing the sole tab to reactivate and clear the placeholder; test both pending and failed reads with one existing tab.
  instances: crates/fe-workbench/src/pane.rs:219

(Lead: F4, the changed-line cap overage, is dropped. The overage was accepted when T63 merged, and splitting merged code is churn.)
