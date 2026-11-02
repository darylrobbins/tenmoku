---
task_uid: "01m6ccm2qp0z56de98wdg0cb47"
task: "T399"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T201

Findings of the review of T201 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/find_commands.rs:109: Replace still does nothing when the terminal's Find input has focus. That row installs handle_in's ShowReplace handler, which stops propagation and calls TerminalFind::run, where ShowReplace is ignored. Consequently neither the new shortcut nor Edit > Replace reaches this fallback to open Replace in the active pane.
  fix: Let ShowReplace propagate from the terminal Find row to the workbench fallback, and add an integration test covering the shortcut and menu invocation from that input.
  instances: crates/fe-workbench/src/find_commands.rs:109
