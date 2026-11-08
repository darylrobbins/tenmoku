---
task_uid: "01m6y59f4ht3eq7bxr7r74tn6d"
task: "T1618"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "ConfigDirOverride in crates/fe-workbench/src/open.rs exists in every build, is public and re-exported from fe_workbench's root with a doc comment saying it is for tests of binaries that embed the workbench; settings_path honours it in every build, and with no override behaves as before (None in fe-workbench's own tests, the environment's path otherwise)"
    required: true
    manual: false
  - id: "C2"
    text: "fastedit test tests::new_workbench_starts_the_frame_watchdog sets ConfigDirOverride(None) as a global before opening its windows and keeps every assertion; no real ConfigWatcher or other watcher thread is started by it (any other found is named in the hand-in and stopped the same way, no sleeps or retries)"
    required: true
    manual: false
  - id: "C3"
    text: "The hand-in records, at the task base and with the change, 20 runs each of cargo nextest run -p fastedit -E 'test(=tests::new_workbench_starts_the_frame_watchdog)' with TENMOKU_CONFIG_DIR set to a temp dir holding an empty settings.toml: the failure count at base, and 20 of 20 passing with the change"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fastedit -p fe-workbench passes and cargo clippy --workspace --all-targets -- -D warnings is clean; only the three scoped files change"
    required: true
    manual: false
---

# CI (windows): fastedit's new_workbench_starts_the_frame_watchdog runs with no user config, so no real LSP-settings watcher thread wakes GPUI's test scheduler

# CI (windows): fastedit's new_workbench_starts_the_frame_watchdog watches the real user LSP settings, and the watcher thread trips GPUI's test scheduler

## Evidence

GitHub CI's `windows` job on the Windows-port merge aa9e4988b (run 42503331207):

    FAIL fastedit::bin/tenmoku tests::new_workbench_starts_the_frame_watchdog
      panicked at gpui-pre-scheduler-0.3.9/src/test_scheduler.rs:194:17:
      Detected activity on thread Some("config-watcher") ThreadId(5), but test scheduler is running on
      Some("tests::new_workbench_starts_the_frame_watchdog") ThreadId(2). Your test is not deterministic.

The backtrace runs from `kiln_core::watch::ConfigWatcher::with_hook`'s thread through the sink closure in
`fe_workbench::lsp::user_layer` (`wake.send_blocking(())`) to a GPUI foreground task's waker. The same
test passed on the windows-port branch's last run (42491561875) and on the macOS and Linux jobs, so it
depends on whether the watcher's sink fires during the test.

The test came in with T1610 (7df4e977b, crates/fastedit/src/main.rs).

## Cause

`fe_workbench::lsp::user_layer` (crates/fe-workbench/src/lsp.rs ~line 206) and the project LSP-settings
watcher (~line 1233) skip their `ConfigWatcher` with `if cfg!(test)`, and `settings_path` in
crates/fe-workbench/src/open.rs returns `None` under `#[cfg(test)]` ("A test with no override has no user
config, not the developer's"). Both are only true for fe-workbench's own unit tests. In fastedit's test
binary fe-workbench is a normal dependency, so `new_workbench` reads the machine's real
`<config_dir>/settings.toml` and starts a real watcher on it. Whenever that watcher calls its sink during
the test (on the first pass when the file exists, or on an event), it wakes a GPUI task from its own
thread, which `TestScheduler` refuses. On a developer Mac with a real `settings.toml` it can also read the
developer's config.

## Change

Give embedders' tests the same "no user config" that fe-workbench's own tests have, and use it here:

1. crates/fe-workbench/src/open.rs: make `ConfigDirOverride` exist in every build (not just
   `#[cfg(test)]`), public and re-exported from the crate root, with a doc comment saying it is for tests
   of binaries that embed the workbench. `settings_path` honours it in every build; with no override it
   behaves as today (`None` in fe-workbench's own tests, the environment's path otherwise). Check every
   other `#[cfg(test)]` user of `ConfigDirOverride` still compiles and means the same thing.
2. crates/fastedit/src/main.rs, `new_workbench_starts_the_frame_watchdog`: set
   `ConfigDirOverride(None)` as a global before opening the windows, so no user LSP layer and no watcher
   is made. Keep every assertion.
3. If, with that, another real watcher or thread in `new_workbench` still runs in this test, name it in
   the hand-in and stop it the same way; don't add sleeps or retries.

## Proving it on this Mac

Before the change, try to reproduce at the task base by giving the test a real settings file:
`TENMOKU_CONFIG_DIR=<a temp dir holding an empty settings.toml>` running
`cargo nextest run -p fastedit -E 'test(=tests::new_workbench_starts_the_frame_watchdog)'` 20 times.
Record how many failed. After the change it must pass 20 of 20 the same way. If the base never fails
here, say so in the hand-in; the Windows job on the next push to GitHub is then the proof, which the
lead checks.

## Do not touch

Production behaviour of the app (the real app still watches the user's LSP settings), lsp.rs's watcher
code, kiln-core's `ConfigWatcher`, the frame watchdog itself (`start_frame_watch`,
`kiln::shell::watch_frame_requests`), and the other tests in main.rs.

## Implementation notes

- Base (af9fddc) with TENMOKU_CONFIG_DIR=<temp dir, empty settings.toml>: 0 of 20 runs failed on this Mac (the sink never fires during the test here); with the change 0 of 20 failed (20/20 pass). The Windows job on the next push is the proof.
- No other watcher thread found in new_workbench for this test; `settings_path` is the only reader of the override that this test reaches, and `keymap_path`/`user_snippets` stay test-only.
- Re-run at the task base commit 4328833e972f6fb0e2ddd7811ec5c53ff2913eb5 (crates identical to af9fddc; the three scoped files checked out from it): 0 of 20 failed. With the change (HEAD): 0 of 20 failed, 20/20 pass.
