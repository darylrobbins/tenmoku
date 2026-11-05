---
task_uid: "01m6n3mg00hvcmtcp69appcpn9"
task: "T1384"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T1074

Findings of the review of T1074 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/search_panel_details_tests.rs:77: The field_bounds closure only returns details_shown and discards the include entity. Consequently, details_hidden_until_toggled passes even if both fields always render or never render, leaving the feature's central visibility requirement untested.
  fix: Inspect the rendered include and exclude fields after drawing each state, using debug selectors or equivalent rendered-element assertions. Verify both are absent initially, present after opening, and absent after closing.
  instances: crates/fe-workbench/src/search_panel_details_tests.rs:77
