---
task_uid: "01m5t4dxe07w2mg8mrvx3d1yt2"
task: "T345"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T1

Findings of the review of T1 that don't block the merge (below merge.block_severity):

- F5 low crates/fe-git/tests/status.rs:54: Configuration isolation applies only to child git commands. The library assertions open gix repositories in the parent process, which still reads developer configuration; for example, a global excludes file matching newdir/ makes the status test fail.
  fix: Run library assertions with isolated Git configuration too, through an isolated test process or repository-opening configuration.
  instances: crates/fe-git/tests/status.rs:54, crates/fe-git/tests/status.rs:179, crates/fe-git/tests/status.rs:185
