---
task_uid: "01m4ykvsp4xr5jha5vqxv40pct"
task: "T719"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git switch_typed_names: with remote team/origin (branch feat) and a local origin/feat, Repo::resolve_branch(\"team/origin/feat\") is BranchRef::Remote { remote: team/origin, branch: feat }, a local name wins over a remote one of the same short name, and Repo::switch and Repo::stash_and_switch of team/origin/feat leave HEAD at refs/heads/feat tracking refs/remotes/team/origin/feat"
    required: true
    manual: false
  - id: "C2"
    text: "git_branch_typed_tests: :Git switch team/origin/feat with a local origin/feat leaves HEAD at refs/heads/feat with upstream refs/remotes/team/origin/feat"
    required: true
    manual: false
  - id: "C3"
    text: "git_branch_typed_tests: :Git switch origin/feat with a conflicting local edit and no local feat asks the overwrite question; a local origin/feat is created while it is open; Force Checkout, and in a second run Stash \u0026 Checkout (one stash), each leave HEAD at refs/heads/feat tracking refs/remotes/origin/feat, not refs/heads/origin/feat"
    required: true
    manual: false
  - id: "C4"
    text: "The existing git_branch_tests and fe-git switch_ref, switch_branch and branch_errors tests pass unchanged, and the gate is green"
    required: true
    manual: false
---

# Resolve typed :Git switch names by their full configured remote, and keep that ref through the overwrite retries (T556 review F4)

## Problem

Mac T556 (branch picker and `:Git switch`, landed as helium T128 in host stack 9, e4d2b586's stack) was approved by codex
(review f2pvts on candidate 97h4hm) with one medium finding open, F4 (not fixed across two rounds; no card covers it):

> The ref-identity fix covers picker selections but leaves typed commands and their overwrite retries using ambiguous
> names. With remote team/origin/feat and local origin/feat, ':Git switch team/origin/feat' switches to the wrong
> local branch because resolve_target splits at the first slash. Likewise, creating local origin/feat while a typed
> remote checkout's overwrite prompt is open redirects Force/Stash to that new local branch.

On Mac main e4d2b586:

- `crates/fe-git/src/branch.rs` `Repo::resolve_target` (used by `Repo::switch` and `Repo::stash_and_switch`): for a
  name that is only a remote-tracking ref it finds the local branch with `name.split_once('/')`, so
  `team/origin/feat` yields `origin/feat`, and a local `origin/feat` is switched to instead of `feat` tracking
  `team/origin/feat`. `crates/fe-git/src/refs.rs` already has the right split, `split_remote` (longest configured
  remote name), used by `Repo::branches`.
- `crates/fe-workbench/src/git_branch.rs`: `run` makes a typed name `Target::Name(name)`; `run_switch` passes it to
  `repo.switch`/`repo.stash_and_switch` by name; on `Outcome::Overwrite`, `ask_overwrite` retries with
  `Switch::Force(target)`/`Switch::Stash(target)` of the same `Target::Name`, which is resolved again (local first)
  when the answer comes, so a local `origin/feat` made while the prompt is open takes the retry. The picker path
  (`Target::Ref(BranchRef)`, `branch_ref`) is fixed (F2, F8).

## Change

**fe-git** (`crates/fe-git/src/branch.rs`, `crates/fe-git/src/refs.rs`):

- `pub fn resolve_branch(&self, name: &str) -> Result<BranchRef, SwitchError>` on `Repo`: `refs/heads/{name}` first
  (`BranchRef::Local`), else `refs/remotes/{name}` split by `split_remote` over the configured remote names into
  `BranchRef::Remote { remote, branch }`, else `SwitchError::NotFound`. Make `split_remote` `pub(crate)`.
- `Repo::switch` and `Repo::stash_and_switch` resolve through `resolve_branch` and run `switch_to` /
  `stash_and_switch_to`; drop `resolve_target` and the private `Target` enum.

**fe-workbench** (`crates/fe-workbench/src/git_branch.rs`):

- A typed name is resolved once, on the background executor in `run_switch`, with `resolve_branch`, and switched
  with `switch_to` / `stash_and_switch_to`. `Outcome::Overwrite` carries the `BranchRef` it resolved, and
  `ask_overwrite` retries Force and Stash with `Target::Ref` of it, so the retry goes to the ref the first attempt
  meant whatever branches appear while the prompt is open. Messages still show the name as typed.

## Tests

New `crates/fe-git/tests/switch_typed_names.rs` (temp repos as `tests/switch_ref.rs` builds them) and new
`crates/fe-workbench/src/git_branch_typed_tests.rs`, declared `#[cfg(test)] #[path = ...]` from git_branch.rs beside
`git_branch_tests`. The fixture and helpers in `git_branch_tests.rs` may be made `pub(super)` for reuse; no test there
changes.
