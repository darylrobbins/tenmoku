---
task_uid: "01m4rjt59y2zx6mxzhb26pg5tc"
task: "T45"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test switch_branch::switches: switch to a local branch moves HEAD and carries a non-conflicting local change; switch to origin/feat creates a local feat tracking it, and again switches to the existing local feat; create_branch makes and switches to a new branch from HEAD"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test switch_branch::would_overwrite: with a local edit to a file that differs on the target branch, switch returns WouldOverwrite listing that file's absolute path and HEAD and the file are unchanged; switch with force=true succeeds and the edit is gone; stash_and_switch succeeds and git stash list shows one entry with the fastedit message"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test switch_branch::errors: create_branch('bad..name') is InvalidName and switch('nope') NotFound, both changing nothing"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-git passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Switch and create branches through the git CLI in fe-git, reporting local changes a switch would overwrite, with stash-and-switch and force (Mac T538)

Filed on helium from Mac T538 (~/coordination/helium-cards/T538.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-w.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user (2026-10-12) asked to list local and remote branches, check one out, create a branch, and "refuse/ask on
dirty conflicts". fe-git can't switch branches; gix has no checkout that keeps local changes and refuses to overwrite
them as `git switch` does, so this runs `git` (see the git-reads card).

## Change

New `crates/fe-git/src/branch.rs`, on `Repo`, through the `cli.rs` runner (the staging card):

- `switch(name: &str, force: bool) -> Result<(), SwitchError>`: a local branch, `git switch -q <name>`; a remote one
  (`origin/feature`), `git switch -q --track <remote>/<name>` making the local branch, or, when a local branch of
  that name already exists, switching to it (VS Code's checkout of a remote branch). `force` adds
  `--discard-changes`.
- `create_branch(name: &str) -> Result<(), SwitchError>`: `git switch -q -c <name>` from HEAD, local changes carried.
  The name is checked first with `git check-ref-format --branch`: `SwitchError::InvalidName`.
- `stash_and_switch(name: &str) -> Result<(), SwitchError>`: VS Code's "Stash & Checkout": `git stash push
  --include-untracked -m "fastedit: before switching to <name>"`, then `switch(name, false)`; the stash stays (not
  popped), as VS Code leaves it.
- `SwitchError::WouldOverwrite { paths: Vec<PathBuf> }` when git refuses because local changes would be overwritten
  (absolute paths parsed from git's tab-indented list), `InvalidName`, `NotFound`, `Failed { output }`. Nothing is
  changed on `WouldOverwrite`.
- `crates/fe-git/src/lib.rs`: the `mod` line and re-exports.

## Tests

New file `crates/fe-git/tests/switch_branch.rs` with temp repos, and a bare temp repo as `origin` for remote
branches (`git clone` of a local path, no network).
