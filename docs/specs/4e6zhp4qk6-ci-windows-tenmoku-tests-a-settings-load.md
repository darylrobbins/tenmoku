---
task_uid: "01m70zhjz5q0s88x4e6zhp4qk6"
task: "T1663"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "No #[gpui::test] in crates/fastedit/src/main.rs that does not test file watching starts a live config-watcher thread"
    required: true
    manual: false
  - id: "C2"
    text: "a_settings_load_overtaken_by_a_re_root_is_discarded and settings_loads_completing_out_of_order_keep_the_newest still assert what they assert today and pass"
    required: true
    manual: false
  - id: "C3"
    text: "Only crates/fastedit/src/main.rs changes"
    required: true
    manual: false
---

# CI (windows): tenmoku tests::a_settings_load_overtaken_by_a_re_root_is_discarded trips the GPUI test scheduler with activity on the config-watcher thread

# CI (windows): a_settings_load_overtaken_by_a_re_root_is_discarded trips the GPUI test scheduler

## Evidence

GitHub CI's `windows` job, step "Test", fails on runs 42668192832 (fb244b00, 5.4 s) and 42683260538 (d798b2d2, 3.0 s);
the same test passes on Linux and macOS:

    tenmoku bin  tests::a_settings_load_overtaken_by_a_re_root_is_discarded   (crates/fastedit/src/main.rs:1257)
      panicked at gpui-pre-scheduler-0.3.9 test_scheduler.rs:194:
      Detected activity on thread 'config-watcher' ... Your test is not deterministic.

## Cause (to confirm)

The test's `watched_window` helper starts a real `ConfigWatcher` (kiln/crates/kiln-core/src/watch.rs:126 spawns the
`config-watcher` thread). On Windows, ReadDirectoryChangesW reports events for the temp roots the test creates and
re-roots to, so the thread hands work to the app while the deterministic test scheduler runs, which it forbids. On
Linux/macOS no event arrives inside the test's window. Check the sibling tests that use `watched_window`
(e.g. settings_loads_completing_out_of_order_keep_the_newest) for the same exposure.

## Fix

Make the GPUI tests that do not test file watching deterministic on every OS: give them a window without a live watcher
(or a fake watcher source the test drives), rather than relaxing the scheduler. Keep watcher coverage in tests that use
real threads outside `#[gpui::test]`. Windows CI bugs are fixed on the Mac: reason from the code; a Linux/macOS run
cannot reproduce it, so say in the finish notes which threads each changed test still starts.

## Implementation notes

Cause: `watched_window` starts no `ConfigWatcher` itself (no config service runs in these tests). The thread comes from fe-workbench's `user_layer`, whose `cfg!(test)` guard is false when fe-workbench is a dependency of the tenmoku bin tests, so every Workbench with a user settings path spawned a `config-watcher` thread on the developer's real settings file. The tests now set `ConfigDirOverride(None)`, so `settings_path` is `None` and no user layer is made. The project-settings watcher (`watch_project`) only starts for a trusted root; these tests trust none.

Threads still started by the changed tests: GPUI background executor threads only (settings reads via `background_spawn`); no `config-watcher`. Not reproducible on Mac; reasoned from the code.
