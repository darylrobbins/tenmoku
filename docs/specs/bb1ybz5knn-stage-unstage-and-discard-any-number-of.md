---
task_uid: "01m4t4yqhqe8rbkybb1ybz5knn"
task: "T99"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test stage_many_paths::stage_and_unstage_more_paths_than_arg_max: with more untracked files than fit in getconf ARG_MAX bytes of relative paths, Repo::stage of all of them stages every one (status_split shows each staged Added) and Repo::unstage of all returns every one to Untracked, both while HEAD is unborn and after a first commit"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test stage_many_paths::discard_more_paths_than_arg_max: with that many committed files modified plus a few untracked ones, Repo::discard of all restores every tracked file to its index text and deletes the untracked ones; status_split is then empty"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test stage_many_paths::literal_names_via_stdin: files named '*.txt' and ':(glob)b' are staged, unstaged and discarded alone, never matched as patterns against other files"
    required: true
    manual: false
  - id: "C4"
    text: "The existing tests in crates/fe-git/tests/stage.rs pass unchanged; cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass; no Cargo.toml, Cargo.lock or crates/fe-git/src/lib.rs change"
    required: true
    manual: false
---

# Stage, unstage and discard any number of files in fe-git by passing paths to git on stdin, not on its command line (Mac T631)

## Problem

`Repo::stage`, `Repo::unstage` and `Repo::discard` (`crates/fe-git/src/stage.rs`) put every path on git's
command line: `git add -A -- <paths>` (stage.rs:20-23), `git reset -q -- <paths>` or
`git rm --cached ... -- <paths>` (stage.rs:31-46), and `git ls-files -z -- <paths>` then
`git checkout -q -- <paths>` (stage.rs:55-80). Once the paths' bytes pass the OS argument limit
(`getconf ARG_MAX`: 1 MiB on this Mac, usually 2 MiB on Linux) the spawn fails with E2BIG and
the call returns `GitError::Io("Argument list too long")`; nothing is staged, unstaged or discarded.

Checked 2026-10-13 with git 2.42.1 on this Mac: 6,000 untracked files with ~300-byte paths
(1.84 MB) — `git add -A -- <paths>` failed "Argument list too long" (exit 126); the same paths
through `git add -A --pathspec-from-file=- --pathspec-file-nul` on stdin staged all 6,000, and
`git reset -q --pathspec-from-file=- --pathspec-file-nul` unstaged them.

The git UI cards call these with whole groups: T552's "Stage All Changes", "Unstage All Changes" and
"Discard All Changes" group buttons pass every row of the Changes panel. A repository with a large
untracked or regenerated tree (an un-ignored `node_modules`, a vendored dependency, a mass rename or
reformat) has tens of thousands of rows (`Repo::status_split` lists untracked files one by one), so
those buttons would fail exactly where they're most needed.

## Change

In `crates/fe-git/src/stage.rs`, make `stage`, `unstage` and `discard` work for any number of paths,
with the same results as today for small lists:

- Pass path lists to git on stdin rather than argv where git supports it (`--pathspec-from-file=-
  --pathspec-file-nul` on `add`, `reset`, `rm` and `checkout`; `run` in `crates/fe-git/src/cli.rs`
  already pipes stdin). `--literal-pathspecs` stays in force, so names are never patterns.
- `discard`'s tracked/untracked split must not need the paths on argv either (for example, read the
  index through gix as `index_mode` already does, or batch the `ls-files` calls).
- Keep the existing behaviour: unborn HEAD unstages with `rm --cached`, untracked files are deleted
  only after `check_deletable`, metadata and out-of-tree paths are refused before anything runs.

## Tests

New file `crates/fe-git/tests/stage_many_paths.rs`, using temp repos like `crates/fe-git/tests/stage.rs`.
Size the file set from `getconf ARG_MAX` so the relative paths' bytes exceed it (for example a
200-byte directory name holding enough files), so the tests fail at the base on both macOS and Linux.
