---
task_uid: "01m4kxpvqmkzkqfde81fmgarp3"
task: "T18"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T268 (Mac T285)

Findings of the review of T268 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-fileview/src/stamp.rs:66: The unsupported-class predicate omits ERROR_INVALID_LEVEL (124), the error returned on CIFS/SMB filesystems documented by the spec's [Gnulib reference](https://github.com/coreutils/gnulib/blob/master/lib/stat-w32.c). Consequently, FileView::open still rejects these readable files instead of trying the legacy identity query. The new test repeats the incomplete error list, so the passing gate misses this required compatibility case.
  fix: Include ERROR_INVALID_LEVEL (124) in the fallback predicate and its documentation, and add deterministic coverage showing that this error invokes the legacy query and returns its identity.
  instances: crates/fe-fileview/src/stamp.rs:66, crates/fe-fileview/src/stamp.rs:174



Moved from the Mac board: Mac T285. Its dependencies are finalized on Mac and arrive here with the mac/main sync.
