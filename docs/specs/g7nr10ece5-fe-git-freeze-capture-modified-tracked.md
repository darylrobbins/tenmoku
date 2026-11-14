---
task_uid: "01m7bdv6m96wcc21g7nr10ece5"
task: "T376"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "crates/fe-git/tests/worktree.rs holds a sparse-checkout test: a modified tracked file and a non-ignored untracked file outside the cone are captured by the freeze, absent sparse entries stay absent, and the real index is untouched"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-git passes"
    required: true
    manual: false
---

# fe-git freeze: capture modified tracked files outside a sparse checkout's cone (follow-up from review of T374, Mac T1799)

# fe-git freeze: capture modified tracked files outside a sparse checkout's cone (follow-up from review of T374, Mac T1799)

T374's review (codex, round 2, finding F5, medium): in a sparse checkout, clearing skip-worktree flags on the temporary index still does not make `git add --all` capture modified tracked files outside the sparse cone, because Git filters those paths independently (read-cache.c). The freeze can therefore return stale contents for materialized files outside the cone. This limitation predates the round-2 fix.

## Change
In crates/fe-git/src/worktree.rs, when staging the temporary index, allow staging materialized paths outside the sparse cone (for example `git add --all --sparse`) while preserving the absent sparse entries as they are. Tests in crates/fe-git/tests/worktree.rs: a sparse checkout with a modified tracked file and a non-ignored untracked file outside the cone, both captured by the freeze; absent sparse entries stay absent from the frozen tree's changed-file list; the real index (and its sparse flags) is untouched. Do not assert git's wording; mind the git version differences.
