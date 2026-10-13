---
task_uid: "01m4sd83w8kshamsd5j8bgwhvy"
task: "T70"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test file_log::follows_renames: a.rs committed (c1), edited (c2), git mv'd to b.rs with an edit (c3), b.rs edited (c4), and c.rs committed in between: file_log(b.rs, 100) is [c4, c3, c2, c1] by id, with paths b, b, a, a (absolute), from Some(a.rs) only on c3, and no c.rs commit"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test file_log::limits_and_empty: file_log(b.rs, 2) is [c4, c3]; limit 0, an untracked file and a repo with an unborn HEAD give Ok(empty); a path outside the working tree is Err(GitError::OutsideWorktree)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test file_log::matches_blame_and_ignores_config: with repo config color.ui=always, log.decorate=full, log.showSignature=true, log.follow=false, diff.renames=false and format.pretty=oneline, file_log gives the same list, and each commit equals the CommitInfo Repo::blame gives for that commit's rows (id, short, author, email, time, offset_minutes, summary, message)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-git passes; cargo clippy -p fe-git --all-targets -- -D warnings is clean; no Cargo.toml or Cargo.lock change; file_log runs one git process per call; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# List a file's commits in fe-git, newest first and following renames, through the git CLI (Mac T588)

Filed on helium from Mac T588 (~/coordination/helium-cards/T588.json; Mac lead message ~/coordination/from-mac-lead-2026-10-13-helium-zg.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Its fe-git lib.rs overlap with Mac T572 needs no ordering on helium: T572 landed here as T66.

## Problem

The user, 2026-10-13, wants file history: the current file's commits in a panel, like fugitive's `:0Gclog`,
where opening one shows that commit's change to the file in the DiffView. A repo-wide log with a graph is a
later card.

fe-git (main 5b11e1c4) can blame a file and read it at any commit (`Repo::blame`, `Repo::text_at`,
`Repo::parent` in `crates/fe-git/src/blame.rs`). It can't list the commits that changed a file, or follow the
file across renames. gix has no `--follow`. The `git` CLI does, and `crates/fe-git/src/cli.rs` already runs
reads with `Access::Read` (`commit.rs` :38, `branch.rs` :55; T571 counts commits through it too).

## Change

New module `crates/fe-git/src/log.rs`, declared in `crates/fe-git/src/lib.rs` (`mod log;`,
`pub use log::FileCommit;`, plus a mention in the crate doc):

```rust
/// One commit that changed a file, as `git log --follow` lists it.
pub struct FileCommit {
    pub commit: Arc<CommitInfo>,
    /// Absolute: the file's path in this commit (for a commit that deleted it, the path it had).
    pub path: PathBuf,
    /// Absolute: its path in the first parent, when this commit renamed or copied it there.
    pub from: Option<PathBuf>,
}

impl Repo {
    /// The commits that changed `path` (absolute), newest first, following renames, at most `limit`.
    pub fn file_log(&self, path: &Path, limit: usize) -> Result<Vec<FileCommit>, GitError>;
}
```

- One `cli::run` with `Access::Read`: `git log --follow -z --name-status --format=<NUL-separated full id>
  --max-count=<limit> -- <relative path>`. Pass explicit flags so the user's and the repo's config can't change
  the output: `--no-color`, `--no-decorate`, `--no-show-signature`, `--no-abbrev`, and `-c log.follow=false
  -c diff.renames=true` before `log`. The `--format` string is fixed, so `format.pretty` doesn't apply.
- The `CommitInfo` of each id comes from blame.rs's `commit_info` (make it `pub(crate)`). History and blame then
  show a commit identically. Each id is read once, even when it appears twice.
- A path outside the working tree is `Err(GitError::OutsideWorktree)`, as `blame` does. An untracked or
  never-committed file, an unborn HEAD or `limit` 0 is `Ok(vec![])`; git's "does not have any commits yet"
  failure is mapped to that.

Callers open a commit's change to the file with what exists:
`text_at(parent(id), from.unwrap_or(path))` against `text_at(id, path)`.

## Tests

New file `crates/fe-git/tests/file_log.rs`. It builds temp repos with the `git` CLI and shuts out developer
config, as `crates/fe-git/tests/blame.rs` does (`GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, fixed
author dates).
