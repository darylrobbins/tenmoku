---
task_uid: "01m4cdb47pqtgzydj02k0any93"
task: "T187"
rev: 2
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T107

Findings of the review of T107 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-workbench/src/nav.rs:549: The unavailable-caret fix remains incomplete: caret_place recognizes only the latest workbench-wide navigation destination. After a navigation jump, open another unopened file through quick-open or the project panel and press Back before loading finishes; caret_place returns None, Back consumes an entry without recording a Forward destination, and the loading file cannot be revisited through Forward. Forward has the symmetric problem. Navigation opens in different splits also overwrite the single opened slot, leaving earlier pending destinations unrecognized. The new regression covers only the latest navigation open in one pane.
  fix: Track pending destinations per pane, including other file-opening paths, or defer history traversal whenever the current place cannot be determined. Add regressions for ordinary file loads and overlapping navigation opens across splits, checking restored paths and offsets in both directions.
  instances: crates/fe-workbench/src/nav.rs:91, crates/fe-workbench/src/nav.rs:98, crates/fe-workbench/src/nav.rs:549, crates/fe-workbench/src/nav.rs:563, crates/fe-workbench/src/nav.rs:616, crates/fe-workbench/src/nav.rs:617
