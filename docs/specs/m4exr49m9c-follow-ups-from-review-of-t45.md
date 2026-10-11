---
task_uid: "01m4p31z6rmsrxjnm4exr49m9c"
task: "T48"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T45

Findings of the review of T45 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/vim_host_tests.rs:493: The revised :wa regression distinguishes the named-file failures but still creates every buffer in one pane. It therefore cannot detect incorrect ordering between panes, leaving the explicit cross-pane regression requested by spec F4 incomplete.
  fix: Place the distinguishable failures in separate panes and assert the diagnostic from the first failure in pane order. Retain the later successful saves and untitled-buffer checks, and count dirty buffers across all panes.
  instances: crates/fe-workbench/src/vim_host_tests.rs:493
