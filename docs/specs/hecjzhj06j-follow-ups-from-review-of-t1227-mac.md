---
task_uid: "01m7fmr64fh54yfrhecjzhj06j"
task: "T393"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "F1 (commit_diff.rs:174) fixed: clipping TextDiff's hunks after removing synthetic final rows no longer loses real blank-line changes; regression tests for adding and for deleting files with leading blank lines and no final newline pass (cargo nextest run -p fe-workbench commit_diff)"
    required: true
    manual: false
  - id: "C2"
    text: "F2 (commit_diff.rs:331) fixed: outstanding reads are tracked across generations (a reload/show does not run extra reads beside stale ones; slots are released even when results are stale; current-generation work is scheduled as slots free up), with a test that repeated reloads never exceed the read limit"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes for commit_diff and git_log tests; cargo clippy for fe-workbench is clean"
    required: true
    manual: false
---

# Follow-ups from review of T1227 (Mac T1872)

Findings of the review of T1227 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/commit_diff.rs:174: Clipping TextDiff's hunks after removing synthetic final rows loses real blank-line changes. For an added file containing "\nx", TextDiff matches the empty left text's synthetic row to the leading blank line; this layout then shows only Added(1), dropping the blank line. Deleting that file instead produces a Same row referencing the empty right side, rather than removing both lines.
  fix: Reconcile synthetic-row matches before constructing layout rows, or compute hunks using the same physical-line representation as SideText. Add regression tests for adding and deleting files with leading blank lines and no final newline.
  instances: crates/fe-workbench/src/commit_diff.rs:174, crates/fe-workbench/src/commit_diff.rs:176, crates/fe-workbench/src/commit_diff.rs:178

- F2 medium crates/fe-workbench/src/commit_diff.rs:331: Reload/show resets reading to zero while previously spawned reads remain detached and running. Once the new listing lands, another four reads can start alongside them; repeated reloads can accumulate expensive blob reads and diffs beyond the documented READS limit. Request IDs prevent stale results from landing but do not bound outstanding work.
  fix: Track outstanding reads across generations, releasing their slots even when their results are stale, and schedule current-generation work as slots become available. Alternatively, implement cancellation that actually stops outstanding work. Test reload/show with earlier reads deliberately held in flight.
  instances: crates/fe-workbench/src/commit_diff.rs:331, crates/fe-workbench/src/commit_diff.rs:458, crates/fe-workbench/src/commit_diff.rs:461
