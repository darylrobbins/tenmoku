---
task_uid: "01m5592wngbpwgrkh6zk27kvs7"
task: "T793"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test fe-workbench workbench::git_branch::git_branch_tests::merge_marks::branch_item_marks_a_merge_until_it_is_committed (src/git_branch_merge_marks_tests.rs, declared by #[path] mod merge_marks at the end of src/git_branch_tests.rs) passes: a conflicted git merge feat reads main!, the conflict resolved and git added (MERGE_HEAD still present, nothing unmerged) reads main+!, and after git commit --no-edit main; it fails at the base (main+ for main+!)"
    required: true
    manual: false
  - id: "C2"
    text: "Tests fe-workbench workbench::git_branch::git_branch_tests::merge_marks::branch_item_marks_a_stopped_rebase and workbench::git_branch::git_branch_tests::merge_marks::branch_item_marks_no_conflicted_cherry_pick (same file) pass: a git rebase feat stopped on a conflict ends in ! (not +!), resolved and git added ends in +!, and after git rebase --abort reads main; a conflicted git cherry-pick feat (CHERRY_PICK_HEAD, no MERGE_HEAD) reads main; both fail at the base (\u003cid\u003e+ and main!)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git Repo::merging in src/refs.rs checks only the repository's own git dir (gix git_dir) for MERGE_HEAD, or REBASE_HEAD with rebase-merge/ or rebase-apply/, and runs no git process; Marks::unmerged is renamed merging; git_branch.rs's module docs and git.rs's Marks docs say ! means a merge or rebase in progress"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench -p fe-git passes with no existing test edited (git_branch_tests::marks::branch_item_marks_staged_and_working_changes_apart still reads main! for a conflicted merge); cargo clippy -p fe-workbench -p fe-git --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Mark the status bar's branch item ! while a merge or rebase is in progress (MERGE_HEAD, or REBASE_HEAD with rebase-merge/rebase-apply), as VS Code's headLabel does, instead of while a path is unmerged

## Problem

T784 gave the status bar's branch item VS Code's `*`, `+` and `!`, but read `!` as "a path is unmerged"
(crates/fe-workbench/src/git.rs `Marks::of`, which sets `unmerged` for a `Conflicted` path;
crates/fe-workbench/src/git_branch.rs `status_item`). VS Code's `Repository.headLabel`
(microsoft/vscode main, extensions/git/src/repository.ts) reads:

```ts
return head
    + (this.workingTreeGroup.resourceStates.length + this.untrackedGroup.resourceStates.length > 0 ? '*' : '')
    + (this.indexGroup.resourceStates.length > 0 ? '+' : '')
    + (this.mergeInProgress || !!this.rebaseCommit ? '!' : '');
```

`mergeInProgress` is `isMergeInProgress()`: `<dotGit>/MERGE_HEAD` exists. `rebaseCommit` is
`getRebaseCommit()`: `<dotGit>/rebase-apply` or `<dotGit>/rebase-merge` exists and `<dotGit>/REBASE_HEAD`
reads. `dotGit` is the worktree's own git dir. A conflicted path goes to the merge group only, so it sets
neither `*` nor `+`.

So fastedit diverges three ways. Reproduced at Mac main 92c57c57 in fe-workbench's git_branch_tests fixture
(main and feat both edit `a.txt`):

- A merge with its conflict resolved and staged but not committed (`MERGE_HEAD` present, nothing unmerged)
  reads `main+`; VS Code reads `main+!`.
- A rebase stopped on a conflict, then resolved and staged, reads `<id>+`; VS Code reads `<id>+!`.
- A conflicted `git cherry-pick` (`CHERRY_PICK_HEAD`, no `MERGE_HEAD`, no rebase) reads `main!`; VS Code
  reads `main`.

## Change

In crates/fe-git/src/refs.rs add `Repo::merging(&self) -> bool`, documented as VS Code's `headLabel` `!`:
true when the repository's git dir (`gix` `git_dir()`, the linked worktree's own dir for a worktree) holds
`MERGE_HEAD`, or holds `REBASE_HEAD` and a `rebase-merge` or `rebase-apply` directory. Plain filesystem
checks; no git process.

In crates/fe-workbench/src/git.rs rename `Marks::unmerged` to `merging` and drop the conflicted-path rule
for it: `Marks::of` takes `statuses` and `merging: bool`, sets `merging` from the bool, and a `Conflicted`
path still sets neither `working` nor `staged`. `read` (the background status run) sets a new
`Snapshot::merging` from `repo.repo.merging()`, and `read_status` passes `snapshot.merging` to `Marks::of`.
Update the `Marks` docs.

In crates/fe-workbench/src/git_branch.rs `status_item` reads `marks.merging` for `!`, and the module docs'
item bullet says `!` while a merge or rebase is in progress (as VS Code's), in place of "while a path is
unmerged".

New test file crates/fe-workbench/src/git_branch_merge_marks_tests.rs, declared at the end of
crates/fe-workbench/src/git_branch_tests.rs as `#[path = "git_branch_merge_marks_tests.rs"] mod
merge_marks;`, using that file's fixture, window, refresh, git and branch_item helpers:

- `branch_item_marks_a_merge_until_it_is_committed`: a conflicted `git merge feat` reads `main!`; after
  writing and `git add`ing a resolution, `main+!`; after `git commit --no-edit`, `main`.
- `branch_item_marks_a_stopped_rebase`: `git rebase feat` stops on the conflict and the item ends in `!`
  (not `+!`); after resolving and `git add`, it ends in `+!`; after `git rebase --abort`, `main`.
- `branch_item_marks_no_conflicted_cherry_pick`: a conflicted `git cherry-pick feat` reads `main`.

The existing marks test (`git_branch_marks_tests.rs`, conflicted merge reads `main!`) passes unchanged.
