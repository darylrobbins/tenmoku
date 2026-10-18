---
task_uid: "01m565rwj1trtjksgjdzp140n8"
task: "T976"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "TempDir::new in kiln/crates/kiln/tests/app_workspace.rs names its directory from the pid, a nanosecond timestamp and a counter, creates it with fs::create_dir (on AlreadyExists, the next counter), and never opens a directory it did not just create"
    required: true
    manual: false
  - id: "C2"
    text: "Every test in the file quits each TestAppContext it started before its TempDir drops, and after a run of the file no kiln-app-workspace-* directory is left under the temp dir"
    required: true
    manual: false
  - id: "C3"
    text: "20 runs of kiln/crates/kiln/tests/app_workspace.rs pass while the workspace's heavy tests run in parallel, and cargo clippy -p kiln --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Flake: kiln app_workspace an_empty_workspace_takes_kilns_keys restores a stale layout from a leaked kiln-app-workspace-<pid>-0 temp dir when a pid is reused: create each TempDir exclusively and quit the app before removing it

## Problem

`kiln::app_workspace an_empty_workspace_takes_kilns_keys` failed once in the post-merge check of main
813adf57 (T858 landing), run 01m564a0psvenn6sj20qjfaahe (`test.fastedit.log` around line 3283), and passed on
the immediate rerun of the same commit (run 01m5650ykb2fhx090qdmhkr566). The machine was under heavy load
and low on disk. The failure is at `kiln/crates/kiln/tests/app_workspace.rs:333`,
`assert_eq!(titles(pane, cx), Vec::<String>::new())`: `left: ["a"]`, `right: []`. The empty workspace opened
holding a note `a` that the test never adds.

T858 is not the cause. Its diff (`git show 813adf57 --stat`) touches only `crates/fe-gpui` (element.rs,
host_blocks.rs, lens.rs, lib.rs, view.rs and two view test files) and its task record. Nothing in kiln.

The cause is the file's `TempDir` helper (`app_workspace.rs:143-180`):

- It names the directory `$TMPDIR/kiln-app-workspace-<pid>-<n>`, where `n` is a per-process counter from 0.
  nextest runs each test in its own process, so every test uses `<pid>-0`, and `create_dir_all` quietly
  reuses a directory that is already there.
- The directory leaks. `Drop` removes it when the test body returns, but gpui's test harness quits the app
  after that. `Workspace`'s `on_app_quit` hook (`kiln/crates/kiln-shell/src/workspace.rs:592`, `save_at_quit`)
  then writes the layout and creates `state/layouts/<hash>.json` again. On this Mac, `$TMPDIR` holds 308
  `kiln-app-workspace-*` directories dated 2026-10-14 to 2026-10-17. Each one holds a saved layout, and the
  ones from `notes()` tests hold a single pane with note `a`. No other kiln test file leaks.
- macOS reuses pids. When a later test process gets the pid of a leaked directory, `bare()` (layout restore
  is on by default) restores that layout, so `an_empty_workspace_takes_kilns_keys` sees `["a"]`. A heavily
  loaded machine starts many processes and wraps pids sooner, which makes this more likely.
  `the_layout_is_saved_and_the_next_start_restores_it` and
  `without_restore_layout_a_fresh_window_saves_nothing` can fail the same way.

This is a test bug. Writing the layout at quit is the intended production behaviour.

## Change

Test only, in `kiln/crates/kiln/tests/app_workspace.rs`:

1. `TempDir::new` builds the name from the pid, the current time in nanoseconds and the counter. It creates
   the directory with `fs::create_dir`, not `create_dir_all`, and on `AlreadyExists` tries the next counter
   value. It never opens a directory it did not just create.
2. Every test that starts an app quits it before its `TempDir` drops: call `TestAppContext::quit` on each
   context the test used (`cx`, or `first` and `second`), so the layout written at quit lands before the
   directory is removed. Keep each test's assertions as they are.

Don't add a dependency. Don't change kiln-shell's layout saving.

## Tests

There is no regression test, because the fix is to the tests. To prove it, run the file 20 times while the
workspace's heavy tests run in parallel, then check that the runs leave no `kiln-app-workspace-*` directory
in `$TMPDIR`. Record the evidence in the decision file.

## Implementation notes

- `TempDir::new` names from pid, nanosecond timestamp and counter, uses `fs::create_dir`, retries on `AlreadyExists`.
- Every test quits its `TestAppContext`(s) before the `TempDir` drops.
- Evidence (C3): `cargo nextest run --workspace` (3787 tests) was running in the background throughout. Meanwhile the app_workspace test binary (`target/debug/deps/app_workspace-*`) was run in a loop, 2 x 20 = 40 runs, with 0 failures. No `kiln-app-workspace-<pid>-<nanos>-<n>` directory was left in `$TMPDIR`; the 13 `kiln-app-workspace-<pid>-0` directories there all use the old naming (leaks from earlier code in other lanes). `cargo clippy -p kiln --all-targets -- -D warnings` passes.
