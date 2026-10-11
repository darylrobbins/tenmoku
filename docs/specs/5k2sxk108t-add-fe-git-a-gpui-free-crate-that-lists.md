---
task_uid: "01m4p1wg85w8czgn5k2sxk108t"
task: "T1"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo deny check passes with gix added; crates/fe-git/Cargo.toml has no gpui dependency, and cargo tree -p fe-git -e normal lists no openssl, curl, reqwest or libgit2 crate"
    required: true
    manual: false
  - id: "C2"
    text: "crates/fe-git/tests/status.rs: in a temp repo with one commit, a modified tracked file, a deleted tracked file, a staged new file, an untracked file in a new directory, a gitignored file and a tracked file touched without changing its content, status() returns exactly Modified, Deleted, Added and Untracked for the first four (sorted, absolute paths) and nothing for the last two"
    required: true
    manual: false
  - id: "C3"
    text: "A file left unmerged by a conflicting git merge is reported Conflicted, and status() leaves .git/index's bytes unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "head_text returns the committed text of a modified file, and None for an untracked file, a binary file and any path in a repo with no commits; head_id is None there and changes after a new commit"
    required: true
    manual: false
  - id: "C5"
    text: "discover returns None for a temp dir outside any repo and finds the repo from a subdirectory of its workdir"
    required: true
    manual: false
---

# Add fe-git: a GPUI-free crate that lists the files changed from HEAD and reads a file's HEAD text, through gix (Mac T373)

Filed on helium from Mac T373 (~/coordination/helium-cards/T373.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-a.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user asked on 2026-10-11: "let's add support for showing git changes - a side pane with the list of
changed files and subtle highlighting of the changed rows in files". fastedit has no git support today:
fe-project only reads `.gitignore` through the `ignore` crate, and neither `gix` nor `git2` is a
dependency (Cargo.toml, Cargo.lock). The Changes panel and the editor's changed-row highlighting both need
two things from the repository: which files differ from HEAD, and a file's text at HEAD.

## Change

Add a new crate `crates/fe-git` ("Git status and HEAD contents for a project. Must not depend on GPUI."),
built on **gix** (gitoxide), added to `[workspace.dependencies]` in Cargo.toml as
`gix = { version = "<current>", default-features = false, features = ["status"] }` plus only what blob
reading needs. No network, HTTP, TLS or credential features.

Why gix, not git2: gix is pure Rust under MIT OR Apache-2.0, which deny.toml already allows. git2 links
libgit2 (GPL-2.0 with a linking exception), which deny.toml's allow list doesn't cover and the licensing
policy (permissive only, never GPL) would need an exception for, and it builds C. Shelling out to `git`
needs git installed and a process per refresh.

API (all paths absolute; `Repo` is `Send + Sync`, built on `gix::ThreadSafeRepository`, so the workbench
can run it in the background):

- `Repo::discover(path: &Path) -> Result<Option<Repo>, GitError>`: the repository containing `path`
  (searching upward), `None` outside any repository. A bare repository is `None`.
- `Repo::workdir(&self) -> &Path`.
- `Repo::status(&self) -> Result<Vec<FileStatus>, GitError>`: every file whose working-tree content
  differs from HEAD, combining staged and unstaged changes (gix's status with both the tree-index and
  index-worktree passes), sorted by path. `FileStatus { path: PathBuf, kind: ChangeKind }` with
  `ChangeKind::{Modified, Added, Deleted, Untracked, Conflicted}`:
  - `Added`: in the index but not in HEAD; `Untracked`: in neither (listed file by file, not as a
    collapsed directory; ignored files are never listed);
  - `Deleted`: in HEAD, missing from the working tree or removed from the index;
  - `Conflicted`: an unmerged index entry; it wins over every other kind for that path;
  - `Modified`: anything else that differs, a type change included.
  - No rename tracking: a rename is a `Deleted` plus an `Added`/`Untracked`. Submodules are skipped.
  - A file whose stat changed but whose content didn't is not listed. Don't write the refreshed stat cache
    back to the index (that is git's own business, and writing `.git/index` would wake the watcher).
- `Repo::head_text(&self, path: &Path) -> Result<Option<String>, GitError>`: the file's blob in HEAD's
  tree, decoded as UTF-8 (lossy). `None` when HEAD is unborn, the path isn't in HEAD's tree, the path is
  outside the workdir, or the blob is binary (a NUL in its first 8,000 bytes, as git decides).
- `Repo::head_id(&self) -> Option<String>`: HEAD's commit id in hex, `None` when unborn, so callers can
  tell when HEAD moved.
- `GitError` via `thiserror`, wrapping gix's errors as text.

Tests in `crates/fe-git/tests/status.rs` build repositories in `tempfile` directories by running the
`git` CLI (`git init`, `git add`, `git commit`, `git merge`) with `-c user.name=t -c user.email=t@t
-c commit.gpgsign=false` and `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null` in the command's
environment, so a developer's config can't change them. Keep them few and fast.

## Out of scope

The panel, editor highlighting, the watcher wiring (later cards), staging, committing, blame, branches.

## Implementation notes

- gix 0.89 with `status`, `sha1` and `parallel`. `parallel` is needed for the `Send + Sync` requirement:
  without it `ThreadSafeRepository` holds `Rc`s. It adds no network, TLS or C dependencies.
- `status()` runs the dirwalk with `empty_patterns_match_prefix(false)`, so the result covers the whole
  repository whatever the process's current directory is.
- Per-path combination (`Flags::kind`): Conflicted, then Added (a staged-new file that has since been
  deleted from disk is not listed: it is in neither HEAD nor the working tree), then Deleted, Modified,
  Untracked. A file staged as modified and then reverted on disk still shows Modified (no extra blob
  comparison against HEAD). Untracked directory entries are listed file by file; untracked nested
  repositories (dirwalk kind `Repository`) are skipped along with submodules.
- `Outcome::write_changes` is never called, so `.git/index` is not written.
