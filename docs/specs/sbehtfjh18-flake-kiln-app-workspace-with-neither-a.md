---
task_uid: "01m51g06qjqrgebpsbehtfjh18"
task: "T723"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "No test in kiln/crates/kiln/tests/app_workspace.rs starts the real ConfigWatcher: with_neither_a_root_nor_an_item_start_panics_naming_both sets config.watch_with(..) from batch_channel(), like bare() does, and kiln/src is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "With this commit cherry-picked onto fcb79fba (host-stack-10b-mac, where T678 runs init before the window assert), the test passes 20 of 20 runs while the workspace's heavy tests run in parallel; the decision file records the commands and counts"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p kiln and cargo clippy -p kiln --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Flake: kiln app_workspace with_neither_a_root_nor_an_item_start_panics_naming_both starts the real config watcher, which wakes gpui's test scheduler from its own thread; take changes from watch_with instead

## Problem

The Mac host stack 10b gate (2026-10-16, branch `host-stack-10b-mac`, top `fcb79fba`; log
`.worktrees/gate-stack-10b.test.run1.log` around line 2955, record `.worktrees/LANDING-stack-10b.md`) failed
`kiln::app_workspace with_neither_a_root_nor_an_item_start_panics_naming_both`. The expected start panic
happened (`kiln/crates/kiln/src/app_builder.rs:537`, "kiln::app needs a window: ..."), then gpui's test
scheduler panicked: "Detected activity on thread Some("config-watcher") ... Your test is not deterministic".
The real `ConfigWatcher` thread (kiln-core `watch.rs:88`, started by `ConfigService::start`,
kiln-platform `config.rs:333`) woke a foreground task through `BatchSender::send` (`config.rs:561`). The run
took 0.57 s under load against about 0.1 s normally, and the test passed 10 of 10 alone.

The test (`kiln/crates/kiln/tests/app_workspace.rs`, `with_neither_a_root_nor_an_item_start_panics_naming_both`,
from T672 119c3add) builds `kiln::app(ID, NAME).paths(dir.paths()).status(count)` with no watcher override.
Every other test in the file goes through `bare()`, which sets `.config(move |config|
config.watch_with(closed))` from `batch_channel()`. On main the window assert in `AppBuilder::start` runs
before `init`, so this test never starts a watcher there. Mac T678 (stack pick d2fbbb4d, argon T88) moves the
assert after `init(cx, config)`, so with that pick the test starts the real watcher before it panics, and a
filesystem event can wake the test's foreground executor from the watcher thread. This is the same class of
bug as the `kiln::host a_layout_saved_before_kiln_app_comes_back` fix in T675 (b39ea295, which switched
`tests/host.rs` to `watch_with`).

## Change

Test only. In `with_neither_a_root_nor_an_item_start_panics_naming_both`, take the changed files from a
closed batch channel instead of the real watcher, as `bare()` does:

```rust
let (_, closed) = batch_channel();
let app = kiln::app(ID, NAME)
    .paths(dir.paths())
    .status(count)
    .config(move |config| config.watch_with(closed));
```

Don't touch `AppBuilder::start` or the assert's order. Add a short comment saying why the test passes its
own channel: `start` runs `init`, and so the config service, before it checks for a window.

There's no regression test because this is a test-only flake fix. Prove it with repeated runs under load.
Main's assert runs before `init`, so make those runs on a tree that has T678's assert order: a scratch
worktree at `fcb79fba` (host-stack-10b-mac) with this commit cherry-picked. Record the commands and counts
in the decision file.
