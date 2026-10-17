---
task_uid: "01m54y5b4k52km3rac363jwp78"
task: "T135"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test fe-git::commit_merge_in_progress a_merge_resolved_to_head_commits_as_git_commit_does (crates/fe-git/tests/commit_merge_in_progress.rs, top level) passes: after git merge side conflicts and is resolved with checkout --ours and add, has_staged() is false and Repo::commit(\"Merge branch 'side'\", false) returns the new HEAD, whose parents are main's old HEAD and side, with .git/MERGE_HEAD gone; it fails at the base with Err(NothingStaged)"
    required: true
    manual: false
  - id: "C2"
    text: "Unchanged: with no merge in progress an index matching HEAD is still Err(NothingStaged) with no git commit run (the same test after the merge, and refusals in crates/fe-git/tests/commit.rs); amend, the EmptyMessage check and hook failures behave as before"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-git passes with no existing test edited; cargo clippy -p fe-git --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-git: committing during a merge whose resolution matches HEAD records the merge commit, as git commit does, instead of refusing it as nothing staged (Mac T776)

Filed on argon from Mac T776 (card ~/coordination/argon-cards/zze/T776.json rev 1; note zze). Mac task uid 01m54y00h9095q6ecdq8g2hsgm. Base: argon trunk beb3d17 (= mac/main 9a36816 synced); crates/fe-git/src/commit.rs is byte-identical on Mac main 0e02c820. Title the host commit "... (Mac T776)".

## Problem

Committing from the Changes panel can't conclude a merge whose resolution matches HEAD, which `git commit`
does. Merge a branch, hit a conflict, resolve it by keeping HEAD's side (`git checkout --ours a.txt && git add
a.txt`): the index now equals HEAD's tree, and `crates/fe-git/src/commit.rs` `Repo::commit` returns
`CommitError::NothingStaged` from its `!amend && !self.has_staged()?` check (`git diff --cached --quiet` exits
0) without running git. `crates/fe-workbench/src/git_commit.rs` `committed` turns that into the "nothing
staged" prompt offering Stage All and Commit, which has nothing to stage. The merge stays in progress
(`MERGE_HEAD` remains) until the user leaves the editor for `git commit`.

Reproduced at Mac main 0e02c820 in a temp repository (`main` and `side` each change `a.txt` from `init`; `git
merge side` conflicts; resolved with `--ours` and added): `repo.has_staged()` is `Ok(false)` and
`repo.commit("Merge branch 'side'", false)` is `Err(NothingStaged)`. In the same state `git commit
--cleanup=strip -F -` exits 0 and records a merge commit whose parents are `main` and `side`, clearing
`MERGE_HEAD`. (`git merge --no-commit -s ours side` gives the same state.)

## Change

In `crates/fe-git/src/commit.rs` `Repo::commit`, skip the `NothingStaged` check while a merge is in progress:
`matches!(self.repo.to_thread_local().state(), Some(gix::state::InProgress::Merge))`. git then records the merge
commit with `MERGE_HEAD` as its second parent, as from a terminal. Cherry-picks, reverts and rebases keep the
check: `git commit` refuses an empty one of those without `--allow-empty`. No other change.

New `crates/fe-git/tests/commit_merge_in_progress.rs`, top level, no `mod` wrapper. Its git runs with
`GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`, and it sets `user.name`, `user.email`,
`commit.gpgsign=false` and `core.hooksPath=.git/hooks` in the repository's own config, since `Repo::commit`'s
git inherits the developer's global config. A prototype (5 lines) took the test from failing to passing, and
`cargo test -p fe-git` passed unchanged.
