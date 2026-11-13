---
task_uid: "01m796ph76c03a7rte7ec1yszf"
task: "T596"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "a new test asserts the test harness builds with no thread named config-watcher alive (Linux checkable)"
    required: true
    manual: false
  - id: "C2"
    text: "existing a11y and toast tests still pass"
    required: true
    manual: false
---

# Test harness: no config-watcher thread in gpui tests (Windows scheduler flake) (Mac T1762)

Windows CI flake: gpui-pre's test scheduler panics "Detected activity on thread `config-watcher` ... test not
deterministic" in random a11y/toast tests. The thread is kiln-core's ConfigWatcher reader
(kiln/crates/kiln-core/src/watch.rs:126), started through `ConfigService::start` (kiln-platform/src/config.rs:556)
when `kiln::init` runs with no `Config::watch_with(batches)` (kiln/crates/kiln/src/setup.rs:797-803).
On Linux/Mac the notify backend is quiet enough to not trip it; on Windows the ReadDirectoryChanges thread
reports activity inside a gpui::test.

Fix: find the test harness path that calls kiln::init (or the fe-workbench/fe-gpui test app builder; `rg "kiln::init|init("`
under crates/*/src test helpers) without watch_with, and give it `watch_with(closed_batches)`, the way the other
tests do, so no watcher thread exists. If the harness is production code shared with the app, gate on a test-only
Config flag rather than cfg(test), which other crates' tests don't see. Add a test (new file) that builds the harness
and asserts no thread named config-watcher is alive (/proc/self/task on Linux, skip elsewhere; or a counter on
ConfigWatcher::new behind cfg(any(test, feature)) if the card finds that simpler), so the fix is checkable on Linux.

Scope is a best guess: respec to add the single harness file the search finds. Only Windows CI proves the flake is gone.
