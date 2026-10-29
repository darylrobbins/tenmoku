---
task_uid: "01m62s3d02jv2ekc94x0pqjvk1"
task: "T253"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test commit_stats::counts: a temp-repo commit editing a.rs (+3 -1) and adding n.rs (+5) gives per-path (3,1) and (5,0) and totals (8,1) from Repo::commit_stats(id)"
    required: true
    manual: false
  - id: "C2"
    text: "commit_stats::binary_and_rename: a binary file is reported as binary (not 0/0); a rename with edits reports counts under the new path with from set"
    required: true
    manual: false
  - id: "C3"
    text: "commit_stats::root_and_merge: a root commit counts against the empty tree; a merge counts against its first parent"
    required: true
    manual: false
  - id: "C4"
    text: "commit_stats::paths_with_spaces_and_unicode parse via -z, not quoted-path guessing"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-git passes; cargo clippy -p fe-git --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope"
    required: true
    manual: false
---

# fe-git reports each changed file's added and removed line counts for a commit, and the commit's totals, from git numstat (Mac T1214)

## Problem
VS Code's graph hover says "2 files changed, 10 insertions(+), 5 deletions(-)" and its file rows can show counts. fe-git's CommitFile (crates/fe-git/src/commit_log.rs) has only path, from and kind. This is the GPUI-free half; the hover card and file rows depend on it.
## Change
New crates/fe-git/src/commit_stats.rs, `pub use` from lib.rs: `Repo::commit_stats(&self, commit) -> Result<CommitStats, GitError>` with `files: Vec<FileStat { path, from, added: Option<u32>, removed: Option<u32> }>` (None = binary) and `total()`. Use `git show --numstat -z --format= -M`, through the crate's cli.rs runner as commit_files does.
