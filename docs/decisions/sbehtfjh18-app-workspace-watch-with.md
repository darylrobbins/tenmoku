# kiln app_workspace: no real config watcher in the start-panic test

Status: **in place** (T723), measured 2026-10-16.

`with_neither_a_root_nor_an_item_start_panics_naming_both` now sets
`config.watch_with(closed)` from `batch_channel()`, as `bare()` does. With
T678's assert order (`init` before the window check), `start` would otherwise
start the real `ConfigWatcher`, whose thread wakes gpui's test scheduler.

## The run

Main fast-forwarded to `fcb79fba` (T678's assert order), so the runs are on
this lane rebased onto main, no cherry-pick. Test binary built with
`cargo test -p kiln --no-run`, then:

- load: 3 concurrent `cargo test -p kiln --test host --test app_workspace -- --skip with_neither`
  (load average 1 min about 18)
- 20 sequential runs of
  `app_workspace-<hash> with_neither_a_root_nor_an_item_start_panics_naming_both`

Result: **20 of 20 passed.** (An earlier 20 of 20 on a scratch worktree at
`fcb79fba` with the commit cherry-picked agreed.)
