---
task_uid: "01m54zqf40d4pd4ey2wqh8wgsy"
task: "T148"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test fe-git::file_log_merges a_merge_that_resolved_the_file_is_listed_as_git_log_lists_it (tests/file_log_merges.rs, top level) passes: with a conflict merge M resolved to a third text and a later clean --no-ff merge that takes f.txt from one parent, Repo::file_log(f.txt, 100) returns the ids git log --format=%H -- f.txt prints (other, M, main, side, base: the clean merge not listed), and M's entry has path \u003croot\u003e/f.txt and from None; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Unchanged: tests/file_log.rs and tests/file_log_paths.rs pass as is (renames still followed, from set for a rename); a merge whose combined diff is empty under --follow is skipped without misreading the commits after it"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-git passes with no existing test edited; cargo clippy -p fe-git --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-git: a file's history lists a merge that resolved the file differently from every parent, as git log -- <file> does, instead of dropping it (Mac T778)

## Problem

At Mac main 9b406468 `Repo::file_log` (`crates/fe-git/src/log.rs`, which fe-workbench's `git_history.rs` reads
for a file's History) drops a merge that changed the file against every parent, such as a conflict's
resolution, which `git log -- <file>` lists. `file_log` runs `git log --follow --root -z --name-status
--format=%H`, and `--follow` shows only commits whose diff names the path; log prints no diff for a merge
unless asked (`--diff-merges` is off by default), so the merge never shows.

Checked 2026-10-17 (git 2.42.1 on the Mac): base `a/b/c` in f.txt; branch side sets `a/B/c`; main sets
`a/b2/c`; `git merge side` conflicts and is resolved to `a/merged/c` (commit M). `git log --format=%h --
f.txt` lists M, main, side, base; `git log --follow` and `file_log` list main, side, base. The new test below
fails at the base with M missing.

`--cc` (dense combined) lists exactly the merges `git log -- <file>` keeps: a merge whose file matches one
parent's is left out of both. Its `-z --name-status` output differs from a plain commit's, which `parse`
doesn't read:

- a merge that changed the file: `<id>\0\0MM\0f.txt\0` (an empty token, then one status letter per parent, no
  leading `\n`, one path);
- a merge `--follow` still prints with nothing listed (one that took the file from a parent):
  `<id>\0\0` followed directly by the next commit's id.

## Change

In `crates/fe-git/src/log.rs` `Repo::file_log`, add `--cc` to the `git log` args. In `parse`, after the id:
an empty token marks a combined entry; take the next token as its status only if it is non-empty and all ASCII
uppercase (a commit id is lowercase hex), and then one path, never two (`from` is `None`); otherwise skip the
merge, which listed nothing, and go on with the next id. A plain commit's `\n<status>` is read as now (strip the
`\n` before testing for `R`/`C`). Update `parse`'s doc comment for the combined form. A prototype of exactly
this passed the new test and all of `cargo test -p fe-git` at 9b406468.

Known limit, unchanged by this card: a merge after a rename in one parent (side renames f to h, main edits f,
the merge resolves h) is listed by `git log -- h` but its combined diff is empty under `--follow`, so `file_log`
still leaves it out, as it does at the base. Note it in `file_log`'s doc comment.

New integration test `crates/fe-git/tests/file_log_merges.rs`, top-level fn
`a_merge_that_resolved_the_file_is_listed_as_git_log_lists_it`, building its repository with the `git` CLI as
`tests/file_log.rs` does (`GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, `rerere.enabled=false`), but
with a distinct author and committer date per commit so log order is fixed: the conflict merge above, then a
branch `other` editing f.txt, a commit on main touching only g.txt, and `git merge --no-ff other` (a clean
merge that takes f.txt from `other`). `file_log(f.txt, 100)`'s ids equal `git log --format=%H -- f.txt`'s
(other, M, main, side, base: the clean merge not listed), and M's entry has path `<root>/f.txt` and `from`
`None`.
