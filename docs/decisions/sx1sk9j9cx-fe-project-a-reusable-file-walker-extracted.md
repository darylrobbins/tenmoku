# fe-project: a reusable file walker (T707)

The walk under the project search is now `walk_files` in
`crates/fe-project/src/search/walk.rs`, exported from `fe_project` with
`WalkedFile`, `WalkControl` and `WalkSummary`. Other scans (structural search)
share its rules: the listing walk's gitignore handling, a `PathFilter` that
prunes excluded directories, regular files only, a cancel flag, and each file
opened with `open_file_under` beneath the canonical root.

- **Order.** `threads == 1` walks in path order on the calling thread, as the
  search always did; otherwise `visit` runs on the walker's threads.
- **Open errors** go to `WalkSummary::errors`; they never reach `visit`.
- **Search keeps its own checks.** The excluded file and the match cap are
  decided before a file is opened, so the crate-internal `walk_sequential` and
  `walk_parallel` take a `pre` hook for them. `search_inner` uses those, so
  its counters and ordering are unchanged. `walk_files` is the public wrapper
  with no hook.
- No existing test changed.
