---
task_uid: "01m4sk8h1bm2ppsahfn9z4q7qr"
task: "T608"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-git test commit_log::head_and_all: in a temp repo with main c1-c2, topic t1 off c1 merged into main as m, an unmerged side branch s1 off c2, tag v1 on c1 and a remote-tracking ref origin/main: log(Head, 0, 100) and log(All, 0, 100) give the ids and parents git log --topo-order --format='%H %P' gives for HEAD and for --branches --remotes --tags HEAD; only All has s1; m carries main (Branch, is_head), c1 carries v1 (Tag), the origin/main commit carries origin/main (Remote); a detached HEAD gives a RefKind::Head label"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test commit_log::paging_and_edges: log(All, 0, 2) followed by log(All, 2, 100) equals log(All, 0, 100); limit 0, an empty repo and an unborn HEAD give Ok(empty); with repo config color.ui=always, log.decorate=short, log.showSignature=true, log.abbrevCommit=true and format.pretty=oneline the results are unchanged; each LogCommit's commit equals the CommitInfo Repo::blame gives for it"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test commit_log::commit_files: a commit editing a.rs and adding n.rs lists a.rs Modified and n.rs Added; a git mv a.rs b.rs commit lists b.rs Modified with from Some(a.rs); a deletion lists Deleted; the root commit lists its files Added; the merge m lists the files that differ from its first parent; an unknown id is Err; paths are absolute"
    required: true
    manual: false
  - id: "C4"
    text: "fe-git test graph::layouts: a linear history is all column 0 with one lane; a merge puts the merge at column 0, the side branch's commits at column 1, and closes lane 1 at the fork; an octopus merge opens two lanes; two unrelated roots get columns 0 and 1; a parent not yet pushed keeps its lane open; laying out a list in two pages with one GraphLayout gives the same rows as all at once; a lane keeps its colour along its first-parent line"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-git passes; cargo clippy -p fe-git --all-targets -- -D warnings is clean; no Cargo.toml or Cargo.lock change; log and commit_files each run one git process per call; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# List a branch's or every branch's commits with parents and ref names in fe-git, a commit's changed files, and lay out a commit graph's lanes as a pure function

## Problem

The user, 2026-10-13, after asking for file history (T590): "a repo-wide log with graph is a later card". This is
that card's GPUI-free half: fe-git can list one file's commits (T588's `Repo::file_log`, `crates/fe-git/src/log.rs`)
and blame, but it can't list a branch's or every branch's commits with their parents and ref names, can't say
which files a commit changed, and nothing lays out the lanes of a commit graph. The UI half (the Git Log panel,
`:Gclog`) is a separate card that depends on this one.

## Change

### Commit list: new `crates/fe-git/src/commit_log.rs`

Declared in `crates/fe-git/src/lib.rs` (`mod commit_log;`, `pub use commit_log::{LogCommit, LogScope, RefLabel,
RefKind, CommitFile};`, and a line in the crate doc).

```rust
/// Which commits a log lists.
pub enum LogScope {
    /// HEAD's history (the current branch, or a detached HEAD).
    Head,
    /// Every local branch, remote-tracking branch and tag, and HEAD.
    All,
}

pub struct LogCommit {
    pub commit: Arc<CommitInfo>,
    /// Full hex ids, first parent first; empty for a root commit.
    pub parents: Vec<String>,
    /// The refs pointing at this commit, in git's decoration order.
    pub refs: Vec<RefLabel>,
}

pub struct RefLabel {
    /// Short name: `main`, `origin/main`, `v1.0`, or `HEAD` for a detached HEAD.
    pub name: String,
    pub kind: RefKind,
    /// HEAD points at this branch (or, for `RefKind::Head`, is detached here).
    pub is_head: bool,
}

pub enum RefKind { Head, Branch, Remote, Tag }

/// A file a commit changed against its first parent (a root commit: against the empty tree).
pub struct CommitFile {
    /// Absolute: the file's path in the commit (for a deletion, the path it had).
    pub path: PathBuf,
    /// Absolute: its path in the first parent, when the commit renamed it.
    pub from: Option<PathBuf>,
    /// Added, Modified (a rename included) or Deleted.
    pub kind: ChangeKind,
}

impl Repo {
    /// `limit` commits after the first `skip`, in `git log --topo-order` order, with parents and refs.
    pub fn log(&self, scope: LogScope, skip: usize, limit: usize) -> Result<Vec<LogCommit>, GitError>;
    /// The files `commit` (a full or short id) changed against its first parent, renames detected, by path.
    pub fn commit_files(&self, commit: &str) -> Result<Vec<CommitFile>, GitError>;
}
```

- `log` is one `cli::run` with `Access::Read`: `git log --topo-order -z --format=<%H, %P, %D separated by
  NUL/unit separators> --decorate=full --skip=<skip> --max-count=<limit>`, plus `--branches --remotes --tags HEAD`
  for `All` and `HEAD` for `Head`. Pass explicit flags so the user's and the repo's config can't change the output
  (`--no-color`, `--no-show-signature`, `--no-abbrev`, `-c log.showSignature=false` and the like before `log`),
  as T588's `file_log` does. `%D` with `--decorate=full` gives `HEAD -> refs/heads/main, refs/remotes/origin/main,
  tag: refs/tags/v1`; map `refs/heads/` to `Branch`, `refs/remotes/` to `Remote` (drop `origin/HEAD`),
  `tag: refs/tags/` to `Tag`, `HEAD -> x` to `x` with `is_head`, and a bare `HEAD` to `RefKind::Head`.
- Each `CommitInfo` comes from blame.rs's `commit_info` (T588 makes it `pub(crate)`), so the log, history and blame
  show a commit identically. Each id is read once per call.
- An unborn HEAD, an empty repository or `limit` 0 is `Ok(vec![])` (git's "does not have any commits yet" mapped
  to that, as T588 does).
- `commit_files` is one `cli::run` (`git diff-tree -r -z -M --name-status --root` against the first parent, or the
  equivalent): a merge is compared with its first parent only, as VS Code's and GitHub's commit views do. An
  unknown id is `Err`.

### Graph lanes: new `crates/fe-git/src/graph.rs`

A pure function of ids and parents, no `Repo`, declared in lib.rs (`pub mod graph;`). It lays out lanes the way
VS Code's Source Control Graph ("swimlanes") does: each row's input lanes are the previous row's output lanes.

```rust
pub struct GraphLayout { /* the open lanes: the id each expects next and its colour */ }

pub struct GraphRow {
    /// The commit's column.
    pub column: usize,
    /// Its colour index: a lane keeps its colour along its first-parent line.
    pub color: usize,
    /// Segments in the row's top half (row top edge at `from` to the middle at `to`)
    /// and bottom half (middle at `from` to the bottom edge at `to`).
    pub top: Vec<GraphLine>,
    pub bottom: Vec<GraphLine>,
}

pub struct GraphLine { pub from: usize, pub to: usize, pub color: usize }

impl GraphLayout {
    pub fn new() -> Self;
    /// The row for the next commit, in log order.
    pub fn push(&mut self, id: &str, parents: &[String]) -> GraphRow;
}
```

Rules: the commit's column is the first input lane expecting its id, else a new lane at the right. Every input
lane expecting this id ends at the commit's dot (top half); the first of them continues to the first parent with
its colour, the others close. Each further parent gets the lane already expecting it if any, else a new lane
appended at the right with the next colour (a rising counter; the consumer takes it modulo its palette). A
commit no lane expects (a branch tip, or the first row) opens a new lane with the next colour. Lanes not involved pass straight through; when a lane closes, the lanes to its right shift left in the
bottom half. A parent not yet listed (off the loaded page) keeps its lane open, so laying out page 2 with the same
`GraphLayout` continues page 1.

## Tests

- New file `crates/fe-git/tests/commit_log.rs`, building temp repos with the `git` CLI and shutting out developer
  config as `crates/fe-git/tests/blame.rs` does (`GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, fixed
  author dates).
- New file `crates/fe-git/tests/graph.rs` for the layout (pure, no repo).

## Decided by the lead overnight

- **Where it lives.** New modules `commit_log.rs` and `graph.rs`, not T588's `log.rs`, so this card only adds
  lines to lib.rs and reuses T588's `commit_info`/cli pattern; it depends on T588 for that.
- **Order.** `--topo-order`, as `git log --graph` forces and as VS Code's graph uses, so a branch's commits stay
  together. Paging is `--skip`/`--max-count`; the UI asks for pages of 200.
- **Merges.** `commit_files` compares a merge with its first parent (VS Code, GitHub), not a combined diff.
- **Graph algorithm.** VS Code's swimlane model (input lanes = previous output lanes, extra parents appended at
  the right); colours are indexes, the UI picks the palette.
- **Ref names** come from git's `%D` in the same process, not a second refs read.

## Implementation notes

- `log` checks HEAD with gix first: an unborn HEAD (an empty repository, or an orphan branch beside other
  branches) is `Ok(vec![])` without running git, since `git log HEAD` fails there with "bad revision". So `All`
  on an orphan branch is empty too, as the spec's "an unborn HEAD … is `Ok(vec![])`" reads.
- Decorations are limited with `--decorate-refs=HEAD --decorate-refs=refs/heads/ --decorate-refs=refs/remotes/
  --decorate-refs=refs/tags/`: an explicit pattern overrides the repo's `log.excludeDecoration` (tested), and it
  leaves out `refs/stash` and notes.
- `commit_files` resolves the id (full or short) with gix's prefix lookup, so git only ever sees full ids, then
  runs `git diff-tree -r -z -M --name-status --no-relative <first parent> <id>` (`--root <id>` for a root
  commit): the two-tree form compares a merge with its first parent alone. One git process per call.
- `path_of` is duplicated from `log.rs` rather than shared, since `log.rs` is outside this card's scope.
