---
task_uid: "01m4rjt4czv5m6758kgs54ef35"
task: "T42"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test index_status::index_text: in a temp repo, a file staged then edited on disk gives index_text == the staged content and head_text == the committed one; an untracked file, a binary file and a path outside the worktree give None"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test index_status::split: a file modified and staged then modified again has staged Some(Modified) and unstaged Some(Modified); staged-only, unstaged-only, untracked (unstaged Untracked), staged-new (staged Added) and deleted-unstaged (unstaged Deleted) files each come out as such, sorted by path"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test branches::lists_and_heads: with branches main, feat (newer commit) and a clone's origin/main, branches() lists feat then main (local, newest first, current marked) then origin/main (remote Some(\"origin\")), no origin/HEAD; current_branch is Branch(\"main\"), Detached(\u003c7 chars\u003e) after checking out a commit, Unborn(\"main\") in a fresh repo; head_message returns the full HEAD message"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-git passes, no Cargo.toml or Cargo.lock change; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Read the index text, staged and unstaged status, the current branch, branches and HEAD's message in fe-git through gix (Mac T535)

Filed on helium from Mac T535 (~/coordination/helium-cards/T535.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-w.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user (2026-10-12) asked for diffs against the index, staging per file and hunk, an amend toggle and a branch
switcher. fe-git (`crates/fe-git/src/lib.rs`) reads only a combined status (`Repo::status`: staged and unstaged
merged into one `ChangeKind`), HEAD's text (`head_text`) and HEAD's id. The Changes panel can't show VS Code's
"Staged Changes" and "Changes" groups, the diff view can't show the index side, and nothing lists branches.

**Git library (for every git card from this request):** fe-git reads through gix 0.89 (workspace `Cargo.toml` :62,
features `status`, `sha1`, `parallel`), as `status` and `head_text` do; reads in these cards stay on gix. Writes
(staging, commit, branch switch) run the `git` executable, as VS Code and Zed do, because gix doesn't run hooks, sign
commits, apply clean filters when staging, or check local changes on a switch. No new crate for reads.

## Change

- New `crates/fe-git/src/index.rs`: `Repo::index_text(path) -> Result<Option<String>, GitError>`, the file's stage-0
  blob in the index, `None` when it isn't in the index, is binary (NUL in the first 8000 bytes, as `head_text`) or is
  outside the working tree. `Repo::status_split() -> Result<Vec<SplitStatus>, GitError>`, `SplitStatus { path,
  staged: Option<ChangeKind>, unstaged: Option<ChangeKind> }`: `staged` from the HEAD-to-index pass, `unstaged` from
  the index-to-worktree pass (untracked files: `unstaged = Some(Untracked)`; conflicts: both `Conflicted`), sorted by
  path, same options as `status` (no renames, no submodules). `status` stays as it is.
- New `crates/fe-git/src/refs.rs`: `Repo::current_branch() -> Result<Head, GitError>`, `Head::Branch(String)` (short
  name), `Head::Detached(String /* 7-char id */)` or `Head::Unborn(String /* branch name */)`. `Repo::branches() ->
  Result<Vec<Branch>, GitError>`, `Branch { name, remote: Option<String>, is_current, upstream: Option<String>,
  committed: i64 /* tip's committer time */, summary: String }` for `refs/heads/*` and `refs/remotes/*` (not
  `*/HEAD`), local first, each group newest commit first (VS Code's `git.branchSortOrder` default, committer date).
  `Repo::head_message() -> Result<Option<String>, GitError>`: HEAD commit's full message, for amend.
- `crates/fe-git/src/lib.rs` gets the `mod` lines and re-exports only; its doc comment's "Nothing here writes to the
  repository" stays true for this card.

## Tests

New files `crates/fe-git/tests/index_status.rs` and `crates/fe-git/tests/branches.rs`, building temp repos with the
`git` CLI as `crates/fe-git/tests/status.rs` does (developer config shut out).
