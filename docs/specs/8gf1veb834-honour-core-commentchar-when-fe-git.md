---
task_uid: "01m4t6g1b91a29cc8gf1veb834"
task: "T101"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-git test commit_comment_char::hash_line_commits_when_comment_char_is_semicolon: with core.commentChar=; and a staged change, Repo::commit(\"#42 fix crash\\n#7 too\\n\", false) returns the new HEAD id and git log -1 --format=%B is \"#42 fix crash\\n#7 too\""
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test commit_comment_char::hash_line_commits_when_comment_char_is_auto: with core.commentChar=auto, Repo::commit(\"#42 fix crash\", false) commits that message"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test commit_comment_char::only_custom_comment_lines_is_empty_message: with core.commentChar=;, Repo::commit(\"; only\\n  \\n; comments\\n\", false) returns Err(CommitError::EmptyMessage) and HEAD is unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "The existing tests in crates/fe-git/tests/commit.rs pass unchanged (a # line is still a comment when core.commentChar is unset); cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Honour core.commentChar when fe-git checks a commit message for emptiness, so messages starting with #42 commit where git would (Mac T635)

## Problem

`Repo::commit` (`crates/fe-git/src/commit.rs:57-62`) refuses a message as empty when every line is blank
or starts with `#`, then runs `git commit --cleanup=strip`. But git strips lines that start with the
repository's comment character, `core.commentChar`, which is `#` only by default. People set it (to `;`,
or to `auto`) precisely so a message can start with an issue reference such as `#42`; fastedit refuses
those commits.

Checked 2026-10-13 at e6988aba with git 2.42.1, a temp repo with one staged change, global and system
config shut out:

- `core.commentChar=;`, `commit("#42 fix crash\n#7 too\n")` returns `Err(EmptyMessage)` and commits
  nothing; `git commit --cleanup=strip -F -` with the same config commits `#42 fix crash\n#7 too`.
- `core.commentChar=auto`, `commit("#42 fix crash")` returns `Err(EmptyMessage)`; git commits it.
- `core.commentChar=;`, `commit("; only\n  \n; comments\n")` runs git anyway and returns
  `Err(Failed { code: Some(1), output: "Aborting commit due to empty commit message." })` instead of
  `Err(EmptyMessage)` before git runs.

## Change

In `crates/fe-git/src/commit.rs`, add a private `fn comment_prefix(&self) -> Result<Option<String>,
GitError>` that reads the effective comment prefix through `run_raw(..., Access::Read, ...)`:
`git config --get core.commentString`, falling back to `git config --get core.commentChar`. Unset
(exit 1 for both) means `Some("#")`; `auto` means `None` (git then picks a character no line starts
with, so no line is a comment); any other value is the prefix. The empty check becomes
`line.trim().is_empty() || prefix.is_some_and(|p| line.starts_with(p))`. Update the doc comment's
"`#` lines stripped" to "comment lines (`core.commentChar`, `#` by default) stripped". No public API
change.

## Tests

New file `crates/fe-git/tests/commit_comment_char.rs`, using temp repos and the same isolation as
`crates/fe-git/tests/commit.rs` (rerun in a child with `GIT_CONFIG_NOSYSTEM=1` and
`GIT_CONFIG_GLOBAL=/dev/null`, author and committer from the environment). The pinned regression
tests are `hash_line_commits_when_comment_char_is_semicolon`,
`hash_line_commits_when_comment_char_is_auto` and `only_custom_comment_lines_is_empty_message`; all
three fail at the base.
