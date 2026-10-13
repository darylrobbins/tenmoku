---
task_uid: "01m4s5kcte0aazkrn9ey02xnnq"
task: "T66"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-git test remote::fetch_and_pull: after clone B pushes a commit, A.fetch(false) moves origin/main to it and A.pull(PullMode::FastForwardOnly) moves A's HEAD there; after B deletes a branch on origin, A.fetch(true) prunes its remote-tracking branch and A.fetch(false) does not"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test remote::pull_conflicts: A and B change the same line and B pushes; A.pull(PullMode::Merge) is Err(RemoteError::Conflicts) naming that file by absolute path with the merge left in progress; from the same start A.pull(PullMode::FastForwardOnly) is Err(RemoteError::NotFastForward) with A's HEAD and worktree unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test remote::push: A.push(false) behind B's pushed commit is Err(RemoteError::Rejected) carrying git's output; after A.pull(PullMode::Rebase) it succeeds and origin/main is A's HEAD; A.push(true) with a lease made stale by another push is Err(RemoteError::Rejected)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-git test remote::publish_and_errors: on a new branch push is Err(RemoteError::NoUpstream) and publish(\"origin\") pushes it and sets its upstream to origin/\u003cbranch\u003e; publish on a detached HEAD is Err(RemoteError::Detached); fetch in a repo with no remote is Err(RemoteError::NoRemote); fetch from an unreachable ssh:// or http:// remote on 127.0.0.1 returns an Err carrying git's stderr in under 10 s with stdin never read"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-git passes; no Cargo.toml or Cargo.lock change; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Fetch, pull and push through the git CLI in fe-git, with conflicts, rejections and credential failures reported and never a prompt (Mac T572)

Filed on helium from Mac T572 (~/coordination/helium-cards/T572.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-zb.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note: lent in batch ZB to fill idle lanes; independent of the other ZB card and of helium's T49-T55 chain.

## Problem

fe-git can stage, commit and switch branches through the git CLI (`crates/fe-git/src/{stage,commit,branch}.rs`),
but nothing talks to a remote: there is no fetch, pull or push, so the Changes panel can't sync and a commit can't
be published. Nothing on the board covers it (`rgt task search "fetch pull push remote"`: none).

## Change

New module `crates/fe-git/src/remote.rs` (`mod remote;` and `pub use remote::{PullMode, RemoteError};` in
`crates/fe-git/src/lib.rs`), every call through `crate::cli::run_raw` with `Access::Write`, blocking (callers run it
off the UI thread, as the module doc says):

- `Repo::fetch(&self, prune: bool) -> Result<(), RemoteError>`: `git fetch --no-write-fetch-head` of HEAD's
  upstream remote, else `origin`, with `--prune` when asked; no remote at all is `RemoteError::NoRemote`.
- `Repo::pull(&self, mode: PullMode) -> Result<(), RemoteError>`, `PullMode::{Merge, Rebase, FastForwardOnly}`:
  `git pull --no-edit` with `--no-rebase`, `--rebase` or `--ff-only`. Conflicts stop it with
  `RemoteError::Conflicts { paths }` (absolute paths of the unmerged files, from `git diff --name-only
  --diff-filter=U`), the merge or rebase left in progress for the user; a pull `--ff-only` can't do is
  `RemoteError::NotFastForward`; no upstream is `RemoteError::NoUpstream`.
- `Repo::push(&self, force_with_lease: bool) -> Result<(), RemoteError>`: `git push` to the upstream
  (`--force-with-lease` when asked, never plain `--force`); no upstream is `RemoteError::NoUpstream`; a
  non-fast-forward rejection is `RemoteError::Rejected { output }`.
- `Repo::publish(&self, remote: &str) -> Result<(), RemoteError>`: `git push -u <remote> HEAD` for a branch with no
  upstream; detached HEAD is `RemoteError::Detached`.
- Nothing waits on a prompt: `GIT_TERMINAL_PROMPT=0` (already set by `cli.rs`) plus
  `GIT_SSH_COMMAND="ssh -o BatchMode=yes"` unless the environment or `core.sshCommand` already sets one. A
  credential failure is `RemoteError::Auth { output }` (git's stderr names it: "Authentication failed",
  "could not read Username", "Permission denied (publickey)"); every other failure is `RemoteError::Failed
  { output }` with all git printed. `RemoteError` also wraps `GitError` (`#[from]`).

## Tests

New file `crates/fe-git/tests/remote.rs`: two clones of a local bare repo (file path remotes, no network, isolated
config as in `crates/fe-git/tests/switch_branch.rs`).

## Implementation notes

- `PullMode::Merge` passes `--no-rebase --ff`, so a user's `pull.ff=only` doesn't turn a merge pull into a
  fast-forward-only one.
- `push` names its refspec (`git push -- <remote> HEAD:<branch.<b>.merge>`) rather than relying on `push.default`,
  so an upstream with a different name still works. `pull` and `push` on a detached HEAD are
  `RemoteError::Detached`, like `publish`; `fetch` on one fetches `origin`.
- `fetch` is `NoRemote` when HEAD's branch has no upstream remote (or HEAD is detached) and there is no `origin`.
- Nothing signs, since a signer can ask for a passphrase. A merge or rebase pull that would sign
  (`commit.gpgSign`, as `branch.<b>.mergeOptions` `-S`/`--gpg-sign`/`--no-gpg-sign` leave it) runs as `--ff-only`,
  and one that needs a new commit is `RemoteError::Failed` saying why, with nothing changed. `push.gpgSign` true
  as git reads a boolean (a valueless key, `2`, `-1`, `On` included) makes push and publish `Failed` before git
  runs; `if-asked` pushes unsigned (`--no-signed`). As a backstop every remote call runs with
  `-c gpg.program=false` (and the openpgp, x509 and ssh variants), which git hands to the commands it runs in
  submodules, so a signature nothing above caught (a recursive submodule push, say) fails instead of prompting.
- `pull` passes `--no-autostash` and `--no-recurse-submodules`: local changes in the way stop it (`Failed`) rather
  than being stashed and maybe left in conflict after git exits zero, and submodules are never updated, stashed
  or rebased by it (update them separately). Unmerged files are checked after a successful pull too.
- Git exits that don't mean "absent" (`symbolic-ref` and `config --get` exit 1) are `RemoteError::Git`, not
  `Detached`/`NoUpstream`/`NoRemote`. Conflict paths keep non-UTF-8 bytes (`gix::path::from_byte_slice`).
- Beyond `GIT_TERMINAL_PROMPT=0` and batch-mode ssh, remote calls set an empty `GIT_ASKPASS` (git otherwise falls
  back to `SSH_ASKPASS`, a GUI dialog on many desktops), `GCM_INTERACTIVE=never` and `GIT_EDITOR=true`.
- A `[remote rejected]` push (a server hook refusing it) is `Rejected` too, with git's output.
- The library's git calls inherit the process environment, so each test in `tests/remote.rs` reruns itself in a
  child process with `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, an identity and no proxy or ssh
  variables. `publish_and_errors` also checks a local
  HTTP server answering 401 gives `RemoteError::Auth` without a prompt.
