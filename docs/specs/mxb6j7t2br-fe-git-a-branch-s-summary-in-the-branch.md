---
task_uid: "01m551ckqfdfqyz7mxb6j7t2br"
task: "T783"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test fe-git::branch_summary_subject branch_summaries_are_the_subject_git_log_gives (tests/branch_summary_subject.rs, top level) passes: for branch tips with messages first line\\nsecond line\\n\\nbody, \\n\\n  lead  \\nnext\\n\\nbody and one\\r\\ntwo\\r\\n\\r\\nbody, each Branch::summary from Repo::branches equals git log -1 --format=%s \u003cbranch\u003e (first line second line, '  lead next', one two); it fails at the base (first line, an empty summary)"
    required: true
    manual: false
  - id: "C2"
    text: "blame.rs is not changed (helium T135 holds it); the existing tests in crates/fe-git/tests pass as is"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-git passes with no existing test edited; cargo clippy -p fe-git --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-git: a branch's summary in the branch picker is its tip's subject as git's %s gives it, the whole first paragraph after any blank lines joined by spaces, instead of the message's first raw line

## Problem

The branch picker (`git.switch_branch`, crates/fe-workbench/src/git_branch.rs `BranchPicker::render_match`)
shows each branch's `Branch::summary`, which `Repo::branches` (crates/fe-git/src/refs.rs) takes as the
tip message's first raw line. git's subject (`%s`, and what `git branch -v` shows) is the message's whole
first paragraph, after any leading blank lines, each line's trailing whitespace dropped, joined by one space.

Reproduced at Mac main bcb3ea60 with git 2.42.1, on commits made with `git commit-tree`:

    message "first line\nsecond line\n\nbody\n"   git log -1 --format=%s -> "first line second line"
                                                  Branch::summary      -> "first line"
    message "\n\n  lead  \nnext\n\nbody\n"        git log -1 --format=%s -> "  lead next"
                                                  Branch::summary      -> ""   (an empty row)
    message "one\r\ntwo\r\n\r\nbody\r\n"          git log -1 --format=%s -> "one two"

A message whose subject wraps (no blank line after it, common from tools and some editors) loses its rest,
and one starting with a blank line shows no summary at all.

## Change

In crates/fe-git/src/refs.rs add `pub(crate) fn subject(message: &[u8]) -> String`: split at `\n`, trim each
line's trailing whitespace (`ByteSlice::trim_end`, so `\r` goes too), skip the leading empty lines, take
lines up to the next empty one, and join them with a single space (lossy UTF-8). `Repo::branches` sets
`summary` from `subject(commit.message_raw_sloppy())` in place of the `.lines().next()` chain. Update the
`Branch::summary` doc ("the tip's subject, as git's %s gives it"). Leave blame.rs's `commit_info` (the
History and Log rows; helium T135 holds blame.rs) alone; it can call the same helper later.

New integration test crates/fe-git/tests/branch_summary_subject.rs builds branches whose tips have a wrapped
first paragraph, leading blank lines and CRLF lines, and asserts each `Branch::summary` equals
`git log -1 --format=%s <branch>` (trailing newline dropped). A prototype of exactly this passes, with all
of `cargo test -p fe-git`.
