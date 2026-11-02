---
task_uid: "01m6d33nknkvkxfcsgpspcef3r"
task: "T406"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T211

Findings of the review of T211 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/feature/reindent.rs:87: Every host call receives the original snapshot, so later suggestions cannot observe earlier rows being re-indented as the spec requires. Substituting the planned basis indentation only works when the host's choice of basis, delta, or None remains unchanged. That is not guaranteed by EditorHost, particularly when indentation changes syntax structure. The fixed Braces test host cannot detect this mismatch.
  fix: Evaluate later suggestions against a working snapshot containing preceding indentation edits, while retaining one undo transaction, or provide an equivalent host bulk-indent API. Add a regression host whose later suggestion depends on an earlier row's updated indentation.
  instances: crates/fe-vim/src/feature/reindent.rs:87
