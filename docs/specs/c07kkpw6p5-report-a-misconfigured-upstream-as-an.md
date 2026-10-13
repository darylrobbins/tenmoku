---
task_uid: "01m4sjhenz7brr3fc07kkpw6p5"
task: "T80"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "test unmapped_upstream_is_an_error (cargo test -p fe-git --test upstream_counts_errors): with an upstream the fetch refspec doesn't map, or whose remote doesn't exist, upstream_counts() is Err(GitError::Command) naming 'not stored as a remote-tracking branch'"
    required: true
    manual: false
  - id: "C2"
    text: "head_counts and gone_upstream in crates/fe-git/tests/upstream_counts.rs pass unchanged (no upstream and a gone upstream are still Ok(None))"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Report a misconfigured upstream as an error from fe-git's upstream_counts instead of no counts (T65 review F1) (Mac T571 follow-up)

Follow-up from T65 (Mac T571, landed cadf302), filed on helium: review finding F1 (medium) of review
01m4s78nj0wed3497ntd98v73c (`rgt review show T65`).

## Problem

`Repo::upstream_counts` (`crates/fe-git/src/refs.rs` :220-229) turns any `git rev-list` failure whose stderr
contains "upstream branch" into `Ok(None)`. T65's spec says only "no upstream configured" and "upstream gone" are
`Ok(None)` and every other git failure is the `GitError`. A misconfigured upstream is swallowed: with
`remote.origin.fetch` not mapping `refs/heads/main`, or `branch.main.remote` naming a remote that doesn't exist,
git 2.53 exits 128 with `fatal: upstream branch 'refs/heads/main' not stored as a remote-tracking branch`, and the
status bar shows no counts instead of an error.

What git 2.53 prints, checked on helium (LC_ALL=C, as `cli.rs` sets):

- no upstream: `fatal: no upstream configured for branch 'nou'`
- upstream ref gone: `fatal: no such branch: 'HEAD...'` (older gits: "unknown revision")
- unmapped upstream: `fatal: upstream branch 'refs/heads/main' not stored as a remote-tracking branch`

## Change

In `crates/fe-git/src/refs.rs`, `upstream_counts`: map to `Ok(None)` only stderr containing
"no upstream configured", "no such branch" or "unknown revision"; drop the `"upstream branch"` match, so the
unmapped case and anything else is `Err(GitError::Command { .. })`. The doc comment says which failures are
`None`.

## Tests

New file `crates/fe-git/tests/upstream_counts_errors.rs`, self-contained (copy the isolated-config `git` helper
from `crates/fe-git/tests/upstream_counts.rs`; don't edit that file), with a local bare repo as origin.

- `unmapped_upstream_is_an_error`: after `git config remote.origin.fetch +refs/heads/other:refs/remotes/origin/other`,
  `upstream_counts()` is `Err(GitError::Command { stderr, .. })` with "not stored as a remote-tracking branch" in
  `stderr`; the same after restoring the refspec and setting `branch.main.remote` to `nosuch`.
