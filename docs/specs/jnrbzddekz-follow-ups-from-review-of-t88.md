---
task_uid: "01m48xzyz5gv91vsjnrbzddekz"
task: "T110"
rev: 4
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "F4: deterministic regressions in crates/fe-project/src/tests/tree_root_loss.rs cover both root-loss observation paths with an unloaded descendant; a Lost::Itself result that predates cut_seq or rescan_seq keeps the node and its refresh obligation"
    required: true
    manual: false
  - id: "C2"
    text: "The new tests fail at base"
    required: true
    manual: false
---

# Follow-ups from review of T88

Findings of the review of T88 that don't block the merge (below merge.block_severity):

- F4 medium crates/fe-project/src/tree.rs:403: Root-loss freshness still permits stale deletion results for unloaded directories. Expand p but leave p/c unloaded; run c's listing while c is temporarily absent, restore c, then record Lost::Root through another root child before filling c's result. The result is behind cut_seq, but older is false because c has no children, and seen_since checks only c and p. take consequently unlinks the restored c, and the refresh obligation is lost. A rescan cut short at the root has the same gap.
  fix: Include cut_seq and rescan_seq in the freshness check for Lost::Itself results, retaining the node and its refresh obligation when the result predates either barrier. Add deterministic regressions for both root-loss observation paths with an unloaded descendant.
  instances: crates/fe-project/src/tree.rs:403, crates/fe-project/src/tree.rs:432
