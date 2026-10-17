---
task_uid: "01m551cm02b5znycwv29cp2fh9"
task: "T784"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test fe-workbench workbench::git_branch::git_branch_tests::marks::branch_item_marks_staged_and_working_changes_apart (src/git_branch_marks_tests.rs, declared by #[path] mod marks at the end of src/git_branch_tests.rs) passes: a staged-only edit shows main+, staged then edited main*+, an untracked file alone main*, a clean tree main, and a merge stopped on a conflict main!, as VS Code's Repository.headLabel shows them; it fails at the base (main* for staged-only)"
    required: true
    manual: false
  - id: "C2"
    text: "The existing git_branch_tests tests pass as is (status_item still reads main* for a working-tree edit); workbench.rs is not changed; git_branch.rs's module docs describe the three marks"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Mark the status bar's branch item * for working-tree changes, + for staged ones and ! while a path is unmerged, as VS Code's headLabel does, instead of * for any change

## Problem

The status bar's branch item (crates/fe-workbench/src/git_branch.rs `status_item`) appends `*` "while
anything changed (VS Code's)": crates/fe-workbench/src/git.rs `GitState::read_status` passes
`!snapshot.kinds.is_empty()`. VS Code's item is `Repository.headLabel` in extensions/git/src/repository.ts:
the name, then `*` if the working-tree or untracked group has anything, `+` if the index group has
anything, and `!` during a merge or rebase. So VS Code shows `main+` with only staged changes, `main*+` with
both, `main*` for an untracked file, and `main!` while a merge is stopped on a conflict.

Reproduced at Mac main bcb3ea60 in fe-workbench's git_branch_tests fixture: after `git add a.txt` of an edit
the item reads `main*` where VS Code reads `main+` (assertion `left: Some("main*") right: Some("main+")`);
staged-then-edited is `main*` for VS Code's `main*+`, and a conflicted merge is `main*` for `main!`. The
existing `status_item` test only covers a working-tree edit (`main*`), which stays.

## Change

In crates/fe-workbench/src/git.rs add `pub(crate) struct Marks { working, staged, unmerged }` with
`Marks::of(&[SplitStatus])`: a path that is `Conflicted` on either side sets `unmerged` and nothing else;
otherwise `unstaged.is_some()` (untracked and intent-to-add included) sets `working` and `staged.is_some()`
sets `staged`. `read_status` passes `Marks::of(&statuses)` to `show_branch` in place of the bool, and
`show_branch` takes `Option<(Head, Marks)>`. In git_branch.rs, `Workbench::set_branch_item` and
`status_item` take `Marks` and write `{name}` then `*`, `+`, `!` for each that's set, in that order; the
module docs' item bullet says so. A merge with every conflict resolved but not yet committed shows no `!`
(VS Code reads MERGE_HEAD for that); that is left out. Marks lives in git.rs because `mod git_branch` is
private in workbench.rs, which this card must not touch.

New test file crates/fe-workbench/src/git_branch_marks_tests.rs, declared at the end of
crates/fe-workbench/src/git_branch_tests.rs as `#[path = "git_branch_marks_tests.rs"] mod marks;`, using
that file's fixture, window, refresh and branch_item helpers: staged-only `main+`, staged then edited
`main*+`, an untracked file alone `main*`, clean `main`, and a merge stopped on a conflict `main!`. A
prototype of exactly this passes, with all 15 git_branch tests and clippy clean.
