---
task_uid: "01m4rjt50dyshg1qr8regmykav"
task: "T44"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test commit::commits_and_amends: commit of a staged change returns the new HEAD id with the message stripped of # lines; commit with amend=true replaces HEAD (same parent, new message); has_staged is true before and false after"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test commit::hooks: a pre-commit hook that prints 'lint failed' and exits 1 gives HookFailed { hook: \"pre-commit\", output } with output containing 'lint failed' and HEAD unchanged; a commit-msg hook rejecting gives HookFailed for commit-msg; a passing hook runs (its marker file exists)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test commit::refusals: an all-comment message is EmptyMessage and nothing staged without amend is NothingStaged, both without running git (HEAD unchanged)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-git passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Commit staged changes through the git CLI in fe-git, with amend, hooks run, and hook and git failures reported with their output (Mac T537)

Filed on helium from Mac T537 (~/coordination/helium-cards/T537.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-w.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user (2026-10-12) asked for a commit box in the Changes panel that commits what's staged, with an amend toggle,
and that surfaces hook failures and other errors. fe-git can't commit. Commits run `git` (see the git-reads card): gix
runs no hooks and doesn't sign.

## Change

New `crates/fe-git/src/commit.rs`, on `Repo`, through the `cli.rs` runner (the staging card):

- `commit(message: &str, amend: bool) -> Result<String /* new HEAD id */, CommitError>`: `git commit -q --cleanup=strip
  -F -` with the message on stdin (`--amend` when asked), so `pre-commit`, `commit-msg` and `post-commit` hooks run
  and `commit.gpgsign` is honoured as in a terminal. `GIT_EDITOR=true` so nothing waits on an editor.
- `CommitError`: `EmptyMessage` (only whitespace or `#` lines, checked before running git, as `--cleanup=strip` would
  leave nothing); `NothingStaged` (no HEAD-to-index change and not amending, checked before running git);
  `HookFailed { hook: String, output: String }` when git's output names a failing hook (`pre-commit`, `commit-msg`,
  `prepare-commit-msg`), `output` being the hook's stdout and stderr as git relayed them; `Failed { code, output }`
  for any other failure, git's whole output kept.
- `has_staged() -> Result<bool, GitError>`.
- `crates/fe-git/src/lib.rs`: the `mod` line and re-exports.

## Tests

New file `crates/fe-git/tests/commit.rs`, temp repos with hooks written into `.git/hooks` (executable shell scripts).
