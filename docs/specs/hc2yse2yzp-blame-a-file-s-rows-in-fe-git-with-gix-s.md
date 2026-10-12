---
task_uid: "01m4rjt5n1q25gyphc2yse2yzp"
task: "T46"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-git test blame::runs: a file with rows from three commits by different authors gives runs with each row's commit id, author, author time and summary matching git blame --porcelain for that file; runs cover every row once, in order"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test blame::worktree_text: blaming a text with one row edited and one row inserted marks exactly those rows uncommitted (commit None) and shifts the rows below to their original commits; a file not in HEAD is one uncommitted run"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test blame::commit_texts: text_at(commit, path) returns the file at that commit, None where it didn't exist; parent returns the first parent, None for the root commit"
    required: true
    manual: false
  - id: "C4"
    text: "cargo deny check passes with the blame feature (gix-blame's license is in deny.toml's allow list); Cargo.lock's only new package is gix-blame (plus any gix sub-crate the feature needs); cargo test -p fe-git passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Blame a file's rows in fe-git with gix's blame, uncommitted rows marked, and read a file at a commit and its parent (Mac T539)

Filed on helium from Mac T539 (~/coordination/helium-cards/T539.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-w.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user (2026-10-12) asked for per-line blame of the current file with each commit's summary, author and date, and
to open the commit's diff. fe-git reads nothing per line or per commit. gix 0.89 has blame behind its `blame` feature
(`Repository::blame_file`, crate gix-blame, MIT OR Apache-2.0 like the rest of gix); reads stay on gix (the git-reads
card's decision).

## Change

- Workspace `Cargo.toml` :62: add `"blame"` to gix's features (it pulls `blob-diff`); `Cargo.lock` updated.
  `crates/fe-git/Cargo.toml` adds `imara-diff.workspace = true` (already in the workspace).
- New `crates/fe-git/src/blame.rs`:
  - `Repo::blame(path, text: &str) -> Result<Vec<BlameRun>, GitError>`: blame HEAD's blob with `blame_file` from HEAD,
    then map its rows onto `text` (the editor's current text) with a row diff (imara-diff, Histogram): unchanged rows
    keep their commit, changed and added rows are uncommitted. `BlameRun { rows: Range<u32> /* in text */, commit:
    Option<Arc<CommitInfo>> }`, `None` meaning not committed yet. A file not in HEAD (or HEAD unborn) is one
    uncommitted run. Binary or over 5 MiB: `Ok(vec![])`.
  - `CommitInfo { id: String, short: String /* 7 chars */, author: String, email: String, time: i64 /* author time,
    seconds */, offset_minutes: i32, summary: String, message: String }`, one `Arc` per commit shared by its runs.
  - `Repo::text_at(commit: &str, path) -> Result<Option<String>, GitError>` (as `head_text`, at any commit) and
    `Repo::parent(commit: &str) -> Result<Option<String>, GitError>` (first parent's id), for opening a commit's
    change to the file.
- `crates/fe-git/src/lib.rs`: the `mod` line and re-exports.

## Tests

New file `crates/fe-git/tests/blame.rs`, temp repos with commits by different authors (`-c user.name=…`) and dates
(`GIT_AUTHOR_DATE`).

## Implementation notes

- `text_at` and `parent` take a commit's full hex id (what `CommitInfo::id` and `parent` return), not any revision:
  gix's `rev_parse` sits behind its `revision` feature, which C4's lock budget doesn't cover.
- Rows end at `\n` (a `\r` before it is ignored when comparing), as gix-blame counts lines; the empty row after a
  final line break is in no run. A text without a final line break blames as the same text with one.
- `head_text` and `text_at` share `commit_text` in lib.rs; a path outside the working tree is
  `GitError::OutsideWorktree` for `blame`, `None` for `text_at` (as for `head_text`).
- Extra tests: `blame::unborn_head`, `blame::first_parent_of_a_merge`.
