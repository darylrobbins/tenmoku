---
task_uid: "01m6qk553r99e1h8vb2gppzqd9"
task: "T470"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T465

Findings of the review of T465 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-06, note 1106c), for helium batch 106:

- F1 medium crates/fe-vim/src/feature/exchange.rs:166: Once the last marked row is deleted, subsequent edits map `tail` without checking whether its row survives. Deleting that row collapses `tail` onto the next unmarked row, which line 394 then includes. On "a\nb\nc\nd\ne" at (0,0), `3cxxjjddkddGcxx` consequently produces "e\na\nd" instead of "e\nd\na".
  fix: Track the surviving tail boundary through further row deletions so a collapsed offset at the next row's start excludes that row. Add a regression for sequential deletions of the last two marked rows.
  instances: crates/fe-vim/src/feature/exchange.rs:166, crates/fe-vim/src/feature/exchange.rs:394
