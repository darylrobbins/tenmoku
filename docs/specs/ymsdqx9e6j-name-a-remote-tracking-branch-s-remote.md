---
task_uid: "01m4t9knjdkcpe74ymsdqx9e6j"
task: "T107"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-git test branch_remote_names::remote_with_a_slash_names_its_branches_remote: with remotes origin and team/origin fetched, Repo::branches() lists team/origin/main with remote Some(team/origin) and origin/main with remote Some(origin)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test branch_remote_names::remote_head_of_a_remote_with_a_slash_is_not_listed: after git remote set-head team/origin main, no branch Repo::branches() lists has a name ending in /HEAD"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test branch_remote_names::remote_branch_with_a_slash_keeps_its_remote: origin/feature/x is listed with remote Some(origin)"
    required: true
    manual: false
  - id: "C4"
    text: "crates/fe-git/src/branch.rs is unchanged; the existing fe-git tests (branches.rs, upstream_counts.rs) pass unchanged; cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Name a remote-tracking branch's remote by the configured remote names in fe-git, so a remote called team/origin is named whole and its HEAD isn't listed (Mac T640)

## Problem

`Repo::branches` (crates/fe-git/src/refs.rs) names a remote-tracking branch's remote by splitting its
short name at the first `/`, and skips `<remote>/HEAD` only when the part after that slash is `HEAD`.
git allows a slash in a remote's name (`git remote add team/origin <url>` works, and its branches are
`refs/remotes/team/origin/<branch>`), so for such a remote every branch gets the wrong remote and its
HEAD symref is listed as a branch.

Checked 2026-10-13 at e6988aba (git 2.42.1): a clone with a second remote `team/origin`, fetched, with
`git remote set-head team/origin main`, lists `("team/origin/HEAD", Some("team"))` and
`("team/origin/main", Some("team"))`; `origin/main` is correctly `Some("origin")`.

## Change

In crates/fe-git/src/refs.rs `branches`: read the configured remote names once (gix
`Repository::remote_names()`), and for a `Category::RemoteBranch` take as its remote the longest
configured name `r` for which the short name starts with `r/` and has a non-empty rest. Skip the ref
when that rest is `HEAD`. A ref no configured remote prefixes (a remote since removed) falls back to
today's split at the first `/`. `Branch::remote` docs say it is the configured remote's whole name.
Nothing else changes: `name` stays the short name, and branch.rs (`resolve_target`, owned by T637) is
out of scope.

## Tests

New file crates/fe-git/tests/branch_remote_names.rs. Setup, git run with `GIT_CONFIG_GLOBAL=/dev/null`
and `GIT_CONFIG_NOSYSTEM=1`: a bare upstream.git seeded with one commit on `main`, cloned to `local`;
in `local`, `git remote add team/origin ../upstream.git`, `git fetch team/origin`,
`git remote set-head team/origin main`.

- `remote_with_a_slash_names_its_branches_remote` (regression, fails at base): `branches()` lists
  `team/origin/main` with `remote == Some("team/origin")`, and `origin/main` with `Some("origin")`.
- `remote_head_of_a_remote_with_a_slash_is_not_listed` (regression, fails at base): no listed branch's
  name ends in `/HEAD`.
- `remote_branch_with_a_slash_keeps_its_remote` (control): after pushing a local `feature/x` to origin,
  `origin/feature/x` is listed with `Some("origin")`.
