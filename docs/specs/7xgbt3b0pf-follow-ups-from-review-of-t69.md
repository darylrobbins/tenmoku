---
task_uid: "01m471sd6frey0an7xgbt3b0pf"
task: "T85"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T69

Findings of the review of T69 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/commands.rs:166: Propagation loses the focused editor's invocation context. run_command stamps [editor] onto a local clone, but cx.propagate() forwards the original RunCommand, whose context is empty. Consequently, cmd-q dispatched with the editor focused is recorded as app.quit with context [], contrary to the specification's context requirement. The app propagation test checks only command ID and source, so it misses this.
  fix: Preserve the focused view's context through propagation into app-registry dispatch, and extend the app propagation test to assert the complete observed invocation, including context [editor].
  instances: crates/fe-gpui/src/commands.rs:166
