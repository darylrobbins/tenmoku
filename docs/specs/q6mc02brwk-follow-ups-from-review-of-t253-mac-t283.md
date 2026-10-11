---
task_uid: "01m4kxpvnpex98xkq6mc02brwk"
task: "T16"
rev: 2
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T253 (Mac T283)

Findings of the review of T253 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-buffer/src/proposed.rs:453: Unconditionally storing the verdict can resurrect a stale hunk under concurrent hunks() calls. ProposedChange is Sync: one thread checking an unchanged snapshot can read Pending, another checking an edited snapshot can record Stale, and the first can then overwrite it with Pending. This breaks permanent staleness, makes is_resolved() regress, and allows dry_run or accept after undo to treat that hunk as clean. Previously check() only stored Stale.
  fix: Record only the transition to Stale; never write Pending back from a previously read verdict. Preserve the shared non-mutating verdict helper and add concurrency regression coverage showing that a stale observation cannot be overwritten by an unchanged-snapshot check.
  instances: crates/fe-buffer/src/proposed.rs:453



Moved from the Mac board: Mac T283. Its dependencies are finalized on Mac and arrive here with the mac/main sync.
