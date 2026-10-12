---
task_uid: "01m4s5kckxvtmf8nftrpc0yqn8"
task: "T65"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test upstream_counts::branch_counts: with a local bare repo as origin, local main 2 commits ahead of and 1 behind origin/main (after a fetch) has ahead Some(2), behind Some(1), upstream_gone false in Repo::branches(); a local branch with no upstream and every remote-tracking branch have ahead and behind None"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test upstream_counts::gone_upstream: after its upstream branch is deleted on origin and fetched with --prune, the local branch has ahead and behind None and upstream_gone true"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test upstream_counts::head_counts: Repo::upstream_counts() is Ok(Some((2, 1))) in branch_counts' setup, Ok(Some((0, 0))) once in sync, and Ok(None) on a detached HEAD, an unborn branch, a branch with no upstream and one whose upstream is gone"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-git passes; Repo::branches() makes one extra git process call in total, not one per branch; no Cargo.toml or Cargo.lock change; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Count each branch's commits ahead of and behind its upstream in fe-git, and HEAD's, through the git CLI (Mac T571)

Filed on helium from Mac T571 (~/coordination/helium-cards/T571.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-zb.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note: lent in batch ZB to fill idle lanes; independent of the other ZB card and of helium's T49-T55 chain.

## Problem

fe-git knows each local branch's upstream (`Branch::upstream`, `crates/fe-git/src/refs.rs`), but not how far apart
they are. The status bar's branch item (T556) and any sync/publish action need "2 ahead, 1 behind" for HEAD's branch,
and the branch picker can show it per branch, as VS Code's and Zed's do. Nothing on the board computes it.

## Change

In `crates/fe-git/src/refs.rs` only (no new module, so `lib.rs` is untouched):

- `Branch` gains `ahead: Option<u32>`, `behind: Option<u32>` and `upstream_gone: bool`. For a local branch with an
  upstream that exists, `ahead`/`behind` are the commits on the branch not on its upstream and the reverse. `None`
  for remote-tracking branches, branches with no upstream, and branches whose upstream ref is missing; the last also
  set `upstream_gone` (git's `[gone]`).
- They come from **one** `git for-each-ref --format=%(refname) %(upstream:track,nobracket) refs/heads` call through
  `crate::cli::run` with `Access::Read` (no optional locks), parsed per branch (`ahead N`, `behind N`,
  `ahead N, behind M`, `gone`, empty), and merged into `Repo::branches()`'s result by full ref name. git computes
  the counts itself, so no gix rev-walk feature or Cargo change is needed.
- New `Repo::upstream_counts(&self) -> Result<Option<(u32, u32)>, GitError>`: `(ahead, behind)` for HEAD's branch,
  `None` when detached, unborn, without an upstream or with it gone. One `git rev-list --left-right --count
  HEAD...@{upstream}` call (read access); "no upstream configured" / "upstream gone" exits are `Ok(None)`, any
  other git failure is the `GitError`.
- Doc comments say the counts are as of the last fetch (no network here).

## Tests

New file `crates/fe-git/tests/upstream_counts.rs`, with a local bare repo as `origin` (file path remote, no
network), using the same `git` helper pattern (isolated config) as `crates/fe-git/tests/switch_branch.rs`.
