---
task_uid: "01m5v40m25qerndqc9jq1q51nw"
task: "T355"
rev: 2
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T23

Findings of the review of T23 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/brackets.rs:63: Each descent overwrites in_error, so only the bracket's immediate parent is checked. A complete expression nested beneath an ERROR ancestor is paired, contrary to the requirement to return None when either bracket is inside ERROR. The doc comment explicitly describes this unsupported exception.
  fix: Preserve ERROR ancestry throughout descent or return immediately upon entering ERROR. Update the doc comment and add a regression covering a complete pair nested beneath an ERROR ancestor.
  instances: crates/fe-syntax/src/brackets.rs:63

- F2 medium crates/fe-syntax/src/brackets.rs:90: After 4,096 siblings, matching discards nesting and chooses the opposite bracket nearest the parent's far end. Multiple sibling pairs exist in linked grammars: SQL column_definition contains flattened REFERENCES (...) and CHECK (...) constraints. A REFERENCES list with 3,000 identifiers exceeds the limit, causing its opening parenthesis to pair with the later CHECK closing parenthesis. The second bounded scan can also return None despite an existing counterpart.
  fix: Retain correct nesting across the entire sibling scan, or use an optimization that proves the selected brackets match. Remove the arbitrary correctness cutoff and add wide-parent regressions with multiple sibling pairs, checking both directions while preserving the performance criterion.
  instances: crates/fe-syntax/src/brackets.rs:90, crates/fe-syntax/src/brackets.rs:178
