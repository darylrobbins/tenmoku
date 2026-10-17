---
task_uid: "01m557xq9qca092mqssrf5g6fg"
task: "T153"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test fe-git::blame_summary_blank_lines summary_skips_leading_blank_lines_as_git_blame_does (tests/blame_summary_blank_lines.rs, top level) passes: for a commit whose message is \"\\n  \\n\\tSubject here  \\nmore\\n\\nBody\\n\" and a later one with \"Plain\\n\", the summaries Repo::blame gives are \"\\tSubject here\" and \"Plain\", and Repo::file_log gives \"Plain\" then \"\\tSubject here\"; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "A message with text on its first line keeps the summary it has now (trailing whitespace dropped) and CommitInfo::message stays the whole raw message: fe-git's tests/blame.rs, tests/file_log.rs and tests/commit_log.rs pass as is"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-git passes with no existing test edited; cargo clippy -p fe-git --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-git: a commit's summary in blame and history is its message's first non-blank line, as git blame gives it, instead of empty when the message starts with blank lines (Mac T792)

## Problem

`commit_info` (`crates/fe-git/src/blame.rs`) takes a commit's `summary` from the message's first line,
even when that line is blank. `git blame --porcelain` (and so VS Code's blame, which reads its
`summary` field) skips blank and whitespace-only lines before the first line with text. A message can
start with blank lines when it was written with `git commit --cleanup=verbatim`, by an import tool, or
through plumbing (`git commit-tree`). `commit_info` also feeds `file_log` (log.rs) and the commit log
(commit_log.rs), so the blame gutter, its hover, the file history and the commit graph all show an empty
subject for such a commit.

Reproduced at Mac main 3e3eaa03 with git 2.42 and a new integration test: for a commit whose message is
`"\n  \n\tSubject here  \nmore\n\nBody\n"`, `git blame --porcelain` prints `summary \tSubject here  `,
while `Repo::blame` and `Repo::file_log` give `summary == ""`.

## Change

In `commit_info`, take the first line of the raw message that isn't empty or whitespace only, then
`trim_end` it as now (fastedit already drops trailing whitespace; git keeps it, which no view shows).
`message` stays the whole raw message. Nothing else changes. Test in a new
`crates/fe-git/tests/blame_summary_blank_lines.rs`, which makes the repo with the `git` CLI as
tests/blame.rs does. Prototyped on 3e3eaa03: all of `cargo test -p fe-git` passed.
