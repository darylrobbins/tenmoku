---
task_uid: "01m79d3xnrgyx0pcq4skbbt84g"
task: "T368"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "branch_ops_create_rename_delete::{create_branch_at_a_remote_ref_tracks_it,rename_moves_upstream_config,delete_current_is_refused,delete_unmerged_needs_force} and branch_ops_merge::merge_conflicts_leave_merge_in_progress pass (nextest -p fe-git)"
    required: true
    manual: false
  - id: "C2"
    text: "existing fe-git branch and remote tests still pass"
    required: true
    manual: false
---

# fe-git: create branch from a ref, rename, delete and merge into current, for a branch menu (Mac T1767)

# fe-git: branch operations for a branch menu (create from a ref, rename, delete, merge into current, update project)

fe-git today has: branches(), switch/switch_to, create_branch(name) at HEAD, stash_*, fetch(prune), pull(PullMode::{Merge,Rebase,FastForwardOnly}), push(force_with_lease), upstream_status(). Missing for a JetBrains-style menu:
- `create_branch_at(name, &BranchRef, switch: bool)` (checkout -b name <start>).
- `rename_branch(old, new)` (branch -m), validated like create.
- `delete_branch(name, force)`: refuses the current branch; a not-fully-merged branch returns a typed `NotMerged` so the UI can ask before -D.
- `merge_into_current(&BranchRef)` (merge --no-edit; conflicts leave the merge in progress, reported as `Conflicts`, as git.pull Merge does).
- `update_project()` in fe-workbench git_remote.rs (not fe-git): fetch (prune off) then pull FastForwardOnly; when the branch can't fast-forward the toast says so and offers no merge/rebase (policy open).
Each through cli.rs like branch.rs; errors typed with l10n messages in fe-git/l10n.
## Tests
NEW files fe-git/tests/branch_ops_create_rename_delete.rs and branch_ops_merge.rs (real temp repos like tests/upstream_counts_remote_less.rs). Criteria ids bare: `create_branch_at_a_remote_ref_tracks_it`, `rename_moves_upstream_config`, `delete_current_is_refused`, `delete_unmerged_needs_force`, `merge_conflicts_leave_merge_in_progress`.
Platform: linux-ok.

## Implementation notes

- `create_branch_at` passes `--track` for a remote-tracking start and `--no-track` for a local one, whatever `branch.autoSetupMerge` says; an existing name is the new `SwitchError::Exists`.
- `rename_branch` / `delete_branch` return the new `BranchError` (`InvalidName`, `NotFound`, `Exists`, `Current`, `NotMerged`, `Failed`). A branch checked out in another worktree is `Failed` with git's output.
- `merge_into_current` returns `MergeError` and follows `pull`'s Merge policy: `--no-edit --ff --no-autostash --no-gpg-sign`. When `commit.gpgSign` or `branch.<name>.mergeOptions` would sign, it runs `--ff-only`, and a merge that needs a commit is `Failed` with the signing message. Detached HEAD is refused. It names the source by its short name when that resolves to it, so the message reads `Merge branch 'feat'`; otherwise (say a tag shares the name) it uses the full ref.
- Scope: `branch`, `unmerged_paths` and `signs_commits` in `remote.rs` became `pub(crate)` so the merge can share pull's checks.
- `update_project()` (fe-workbench git_remote.rs) is outside this card's scope and criteria and is left for a follow-up.
