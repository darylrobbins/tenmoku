---
task_uid: "01m4rjt4pck62zny0j0x8mamqa"
task: "T43"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test stage::files: stage of a modified, a deleted and an untracked file stages all three (status_split shows staged only); unstage returns them, including in a repo with an unborn HEAD; discard restores a tracked file to its index text and deletes an untracked one"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test stage::set_index_text: set_index_text writes the given text as the index blob (index_text equals it, the worktree file unchanged), keeps an executable entry's 100755 mode, adds a new entry, and with core.autocrlf=true stores LF for CRLF text"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test stage::errors: a path outside the worktree is OutsideWorktree; a git failure is Command with git's stderr; with PATH lacking git, GitMissing"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-git passes, no Cargo.toml or Cargo.lock change; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Stage, unstage and discard files and set a file's index text through the git CLI in fe-git (Mac T536)

Filed on helium from Mac T536 (~/coordination/helium-cards/T536.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-w.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user (2026-10-12) asked to stage and unstage whole files from the Changes panel and single hunks from the diff
view and gutter, and to discard a file's changes. fe-git can't write anything (`crates/fe-git/src/lib.rs` module
docs). Per the git-reads card, writes run the `git` executable (VS Code's approach): gix doesn't apply clean filters
or autocrlf when staging.

## Change

- New `crates/fe-git/src/cli.rs`: one runner every write uses. Finds `git` on `PATH` once (missing:
  `GitError::GitMissing`); runs `git -C <workdir> …` with `GIT_TERMINAL_PROMPT=0`, `GIT_OPTIONAL_LOCKS=0` for reads,
  `LC_ALL=C` so messages parse, stdin closed unless given; a non-zero exit is `GitError::Command { args, code,
  stderr }` (stderr trimmed, kept whole). Blocking: callers run it off the UI thread.
- New `crates/fe-git/src/stage.rs`, methods on `Repo`, all paths absolute and inside the working tree (else
  `GitError::OutsideWorktree`):
  - `stage(&[PathBuf])`: `git add -A -- <paths>` (adds, modifications and deletions, as VS Code's Stage Changes).
  - `unstage(&[PathBuf])`: `git reset -q -- <paths>`; with HEAD unborn, `git rm --cached -q -r -- <paths>`.
  - `discard(&[PathBuf])`: tracked paths `git checkout -q -- <paths>` (back to the index, VS Code's Discard Changes on
    the Changes group); untracked paths are deleted from disk (VS Code deletes them after its confirmation, which the
    wiring card shows).
  - `set_index_text(path, text: &str)`: VS Code's hunk staging: `git hash-object -w --stdin --path <rel>` (clean
    filters and autocrlf applied) then `git update-index --add --cacheinfo <mode>,<id>,<rel>`, `<mode>` the index
    entry's mode, or `100644` (`100755` if the file on disk is executable) for a new entry.
- `crates/fe-git/src/lib.rs`: the `mod` lines, `GitError` variants, and the module docs updated to say writes go
  through `git`.

## Tests

New file `crates/fe-git/tests/stage.rs` with temp repos (developer config shut out, as `tests/status.rs`). Each test
checks the result with `Repo::status_split`/`index_text` (the git-reads card) or `git diff --cached`.
