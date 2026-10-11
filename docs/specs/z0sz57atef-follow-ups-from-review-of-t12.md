---
task_uid: "01m4m01znb3y9tgwz0sz57atef"
task: "T26"
rev: 3
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T12

Findings of the review of T12 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/lsp.rs:237: Quit counts still do not reflect unfinished cleanup per server. Stopped servers are removed from running, and max(1) substitutes one for any number of unfinished stopped threads. For example, two servers with endless stderr both report Stopped before the graceful deadline, but Quit reports forced: 1 although the switch force-stops both. Mixed running and cleaning-up servers are also undercounted, and left has the same problem if cleanup outlasts the force deadline. The new single-server regression cannot detect this.
  fix: Track completion per server independently of terminal status and count all unfinished servers for both forced and left. Add multi-server coverage, including a mix of running servers and stopped servers still cleaning up.
  instances: crates/fe-workbench/src/lsp.rs:237
