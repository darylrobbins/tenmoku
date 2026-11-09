---
task_uid: "01m6zq19qsbk3sk2dq29fjwkk9"
task: "T545"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T349

Findings of the review of T349 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/git_stash.rs:228: Overlapping Pop and Apply requests use whichever listing finishes first, rather than the latest command. For example, invoking Pop then Apply through custom keybindings while listing is pending can show the older Pop picker and invalidate Apply when that modal appears. Confirming then drops the stash despite Apply being the latest request.
  fix: Track a listing generation and reject results from superseded requests, following the branch listing pattern. Add a deterministic test covering overlapping Pop/Apply requests and both completion orders.
  instances: crates/fe-workbench/src/git_stash.rs:228

Findings of review round 1 of T349 (review 01m5vq6wmyq4dr7r37ymq2cgqv) that don't block the merge either:

- F2 medium crates/fe-workbench/src/git_stash.rs:584: Stash commit IDs are not unique among stash entries: re-storing an older stash with `git stash store` creates another entry referencing that commit. Both rows then receive the same picker identity, and Picker::confirm_id resolves mouse clicks to the first matching row. Clicking the older duplicate therefore pops a different entry than the one clicked; both Pop and Apply use this picker.
  fix: Give each listed entry a unique identity, such as its original index combined with its commit ID. Add regression coverage with duplicate commit IDs that clicks the older row and verifies the selected stash index is used.
  instances: crates/fe-workbench/src/git_stash.rs:584
