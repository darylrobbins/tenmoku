---
task_uid: "01m6enx6f98dkatf06b9b630k8"
task: "T1308"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Command ids git.fetch, git.pull, git.push and git.push_force_with_lease exist with palette titles 'Git: Fetch', 'Git: Pull', 'Git: Push' and 'Git: Push (Force with Lease)': typed_command_ids_tests and command_catalog_tests list them"
    required: true
    manual: false
  - id: "C2"
    text: "git_remote_tests::push_without_upstream_publishes: with no upstream, git.push calls Repo::publish to origin (or the only remote) and the toast says the upstream was set; with several remotes and no origin it toasts a readable error"
    required: true
    manual: false
  - id: "C3"
    text: "git_remote_tests::force_push_is_separate: git.push never passes force_with_lease; only git.push_force_with_lease does, and it asks a yes/no toast question first"
    required: true
    manual: false
  - id: "C4"
    text: "git_remote_tests::runs_off_main_thread: the Repo call runs in cx.background_spawn; a second remote command while one is running toasts 'already running' instead of starting"
    required: true
    manual: false
  - id: "C5"
    text: "git_remote_tests::errors_are_readable: NoRemote, NoUpstream, Detached, Conflicts, NotFastForward and the push-rejected/auth/signing-refused variants of RemoteError each become a toast naming the cause and what to do, and the conflict case names the file count"
    required: true
    manual: false
  - id: "C6"
    text: "git_remote_tests::pull_uses_fast_forward_only_by_default: git.pull uses PullMode::FastForwardOnly; after a successful pull the open buffers reload through the existing external-change path"
    required: true
    manual: false
  - id: "C7"
    text: "git_commands_tests::ex_forms_fetch_pull_push: ':Git fetch', ':Git pull' and ':Git push' route to the same commands, and ':Git push!' stays 'Not supported'"
    required: true
    manual: false
  - id: "C8"
    text: "cargo clippy --workspace --all-targets passes with no allow added outside the new module"
    required: true
    manual: false
---

# Run git fetch, pull, push and force-push-with-lease from the palette and ex line, off the main thread, with toasts for results and errors

## Problem
`crates/fe-git/src/remote.rs` has `Repo::fetch(prune)`, `pull(PullMode)`, `push(force_with_lease)` and `publish(remote)`, each with tests, but no workbench command, palette entry or ex form calls them: a user can commit in Tenmoku but cannot publish. The user approved wiring them on 2026-11-02 ("Yes please" to the day-one gap survey).

## Change
Add `crates/fe-workbench/src/git_remote.rs` (declared like `git_stash`, via `#[path]` in `git_commands.rs`) as a new `Feature::Remote`. Add `GitCommand::{Fetch, Pull, Push, PushForceWithLease}` in `git_commands.rs` with typed commands `git.fetch`, `git.pull`, `git.push` and `git.push_force_with_lease` in `workbench_commands.rs`, mirroring `git.stash` (`run_typed_git`). Add the ex forms to `EX_FORMS`: `:Git fetch`, `:Git pull`, `:Git push` (force-with-lease has no ex form; a bang stays 'Not supported').

Mirror `git_stash.rs` / `git_branch.rs`:
- run the `Repo` call in `cx.background_spawn`, then `cx.spawn_in` back to the workbench;
- toast progress ("Fetching…") and the result through `toast::show` with `Toast::info` or the error variant;
- one remote operation at a time.

Scope decisions:
- Fetch uses prune=false, so the repo's `fetch.prune` decides.
- Pull is fast-forward-only by default, so the editor never starts a merge or rebase unasked.
- Push goes to the upstream. With no upstream, `publish` to `origin` (or the sole remote) and say the upstream was set; with several remotes and none called origin, toast an error.
- Force-with-lease is its own palette command and confirms through the existing question toast first.
- Map every `RemoteError` variant, including auth failures and push rejections, to one readable sentence.
- Reload the open buffers after a pull through the existing external-change path.
- Outside a repository, answer as the other git commands do.
- There is no Git menu; this card adds no menu entries.
- New tests go in `git_remote_tests.rs` with a local bare-remote fixture (no network). The catalog and id lists get one row each.

## Implementation notes

- `Repo::remotes` is private and fe-git is out of scope, so choosing the remote to publish to runs a plain `git remote` in the working tree, as `git_stage::git_output` does for its reads; the force-push target and the repository identity read git the same way.
- A pull changes the index and working tree, so it queues through `git_stage::mutate` like stash. Staging and commits wait for it, for as long as the network takes. Fetch and push touch neither and don't queue. Landing uses `cx.spawn` with `update_window`, as `git_stash` does, so the result is still told if the window closed.
- "One at a time" is per repository, across windows, keyed by the canonical `git rev-parse --git-common-dir`, so a subdirectory root or a linked worktree shares the claim. Finding that takes git, so every command starts with a background step. The global holds weak claims: a claim dies with its task, so a closed window can't leave a repository stuck as running.
- The force-push question names the branch and upstream and holds no claim, so an unanswered one doesn't block other commands. The claim is taken on Yes, which can still answer 'already running'. Just before pushing, the background step reads the branch, its commit and its upstream again and pushes nothing if they differ from what was confirmed.
- This card expands its scope to `crates/fe-git/src/remote.rs` and `crates/fe-git/tests/remote.rs`. `Repo::push` rereads HEAD and the upstream, so a check before it can't hold against a branch switch landing between the two (review F1). `Repo::force_push_commit` pushes a named commit to a named destination under the same `push_args` and `run_remote` safeguards, and `git.push_force_with_lease` uses it instead of `Repo::push(true)`. Plain `git.push` still calls `Repo::push(false)`.
- After a pull, the open files are checked even if the root changed meanwhile, because a new root keeps the tabs but replaces the watcher. The git state is only refreshed if the root is unchanged.
- `RemoteError::Failed` has no signing variant to match, so its toast uses git's first line, which for signing refused already says to run it from a terminal.
