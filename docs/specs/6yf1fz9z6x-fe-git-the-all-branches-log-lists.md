---
task_uid: "01m54y591hmw0bxq6yf1fz9z6x"
task: "T136"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test fe-git::log_many_tips all_branches_log_reads_more_tips_than_fit_in_arguments (crates/fe-git/tests/log_many_tips.rs, top level) passes: with 60,000 commits on main each tagged (packed-refs), Repo::log_tips(LogScope::All) returns 60,000 tips and Repo::log_from(\u0026tips, 1, 3) returns the 3 commits git log --all --topo-order --skip=1 --max-count=3 lists, in that order; it fails at the base with Argument list too long"
    required: true
    manual: false
  - id: "C2"
    text: "Repo::log_of passes revisions to git log on stdin (--stdin), keeping arguments that start with -- on the command line; Repo::log_tips dedupes in first-seen order without a quadratic scan; the existing tests in crates/fe-git/tests/commit_log.rs and commit_log_tips.rs pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-git passes with no existing test edited; cargo clippy -p fe-git --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-git: the All Branches log lists commits in a repository with more refs than fit in the argument list, as git log --all does, instead of failing with Argument list too long (Mac T775)

## Problem

The Git Log panel's All Branches log (`git.log_all`, `:Gclog --all`) fails with "Argument list too long (os
error 7)" in a repository with tens of thousands of refs, where `git log --all` lists the commits.
`crates/fe-git/src/commit_log.rs` `Repo::log_of` passes every tip from `Repo::log_tips` to `git log` as its own
argument (`args.extend(revisions…)`), so once the tips' bytes (41 each plus a pointer) pass `getconf ARG_MAX`
(1 MiB on the Mac, 2 MiB on most Linux) the spawn fails before git runs. `crates/fe-workbench/src/git_log.rs`
`read_first` and the paging in the same file call `log_from(&tips, …)`, so the panel shows the error and no
commits. `log_tips` also dedupes with `tips.iter().any(…)` per tip, which is quadratic in the number of refs.

Reproduced at Mac main 0e02c820: a repository with 60,000 commits on `main` and a tag on each (made with `git
fast-import --export-marks` and a written `.git/packed-refs`, about 2 s) gives `log_tips(LogScope::All)` its
60,000 tips, and `log_from(&tips, 1, 3)` returns `Err(Io("Argument list too long (os error 7)"))`; `git log
--all --topo-order --skip=1 --max-count=3` lists 3 commits. `rev-parse HEAD --branches --remotes --tags | git
log --stdin --topo-order …` lists the same 3. fe-git already moved staging's path lists off the argument list
for the same reason (T631, `--pathspec-from-file=-`).

## Change

In `crates/fe-git/src/commit_log.rs`:
- `Repo::log_of`: add `--stdin` to the `git log` flags, keep `--` as the last argument, and write the revisions
  one per line to git's stdin (`cli::run`'s `stdin` argument) instead of appending them to `args`. Arguments
  that start with `--` (`Repo::log`'s `--branches --remotes --tags`) stay on the command line, since `git log
  --stdin` takes pseudo-options only from git 2.42; `HEAD` and every id go on stdin.
- `Repo::log_tips`: dedupe through a `HashSet` while keeping first-seen order.

New `crates/fe-git/tests/log_many_tips.rs`, top level, no `mod` wrapper: the fixture above (60,000, so it
passes the Linux limit too), asserting `log_tips(LogScope::All).len() == 60_000` and that `log_from(&tips, 1,
3)`'s ids equal `git log --all --topo-order --skip=1 --max-count=3 --format=%H`'s. Build the fixture with one
`fast-import` run and one packed-refs write (sorted by ref name, header `# pack-refs with: peeled fully-peeled
sorted `), not 60,000 loose refs, which take about 40 s; run its git with `GIT_CONFIG_GLOBAL=/dev/null` and
`GIT_CONFIG_NOSYSTEM=1`. A prototype of both changes (about 15 lines) took the test from failing to passing in
about 3 s debug, and `cargo test -p fe-git` passed unchanged.
