---
task_uid: "01m61z5rrwkk56s7mdsjza37pf"
task: "T374"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T128

Findings of the review of T128 that don't block the merge (below merge.block_severity):

- F4 medium crates/fe-workbench/src/git_branch.rs:511: The ref-identity fix covers picker selections but leaves typed commands and their overwrite retries using ambiguous names. With remote team/origin/feat and local origin/feat, ':Git switch team/origin/feat' switches to the wrong local branch because resolve_target splits at the first slash. Likewise, creating local origin/feat while a typed remote checkout's overwrite prompt is open redirects Force/Stash to that new local branch.
  fix: Resolve typed names with local-first lookup and the complete configured remote name, then retain an explicit BranchRef through checkout and overwrite confirmation. Add direct-command tests for slash-containing remotes and collisions introduced while the prompt is open, including both retries.
  instances: crates/fe-workbench/src/git_branch.rs:121, crates/fe-workbench/src/git_branch.rs:419, crates/fe-workbench/src/git_branch.rs:467, crates/fe-workbench/src/git_branch.rs:468, crates/fe-workbench/src/git_branch.rs:511, crates/fe-workbench/src/git_branch.rs:514, crates/fe-workbench/src/git_branch.rs:516, crates/fe-git/src/branch.rs:158
