---
task_uid: "01m6cw9fxskmmkkzv4pxpwrcd9"
task: "T1303"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "opens_in_live_windows_or_asks_for_one calls cx.executor().allow_parking() before it opens the Workbench window, and passes on macOS (cargo test -p fastedit --test open_urls)"
    required: true
    manual: false
  - id: "C2"
    text: "The diff touches only crates/fastedit/tests/open_urls.rs and changes no assertion"
    required: true
    manual: false
---

# Let fastedit's open_urls opens_in_live_windows_or_asks_for_one test allow parking, as other Workbench-on-a-real-root tests do, so the project-watcher thread stops failing it 9 of 9 on Linux (T1272)

`fastedit::open_urls opens_in_live_windows_or_asks_for_one` (crates/fastedit/tests/open_urls.rs, from T1272) fails 9 of 9 on Linux (neon gate for mac/main 3ac61191; also at neon main 2f0c082d). The gpui test scheduler panics: "Detected activity on thread project-watcher ... test scheduler is running on opens_in_live_windows_or_asks_for_one. Your test is not deterministic." The test builds a real Workbench on a tempdir root, so fe-project's watcher (crates/fe-project/src/watcher.rs, thread "project-watcher") starts a real OS thread. macOS passes by timing; the Linux watcher reports activity while the scheduler runs.

Other fe-workbench tests that open a Workbench on a real root call `cx.executor().allow_parking()` first (save_commands_tests.rs:31, path_commands_tests.rs:38, tests/git_log_restore_first_list.rs:89). Do the same here, at the top of the test, before `window(cx, &project)`. Change nothing else: no production code, no other test.

This blocks neon's sync to mac/main (every neon gate that runs the fastedit package fails), so it is urgent. The Mac gate cannot run Linux; after it lands the lead asks neon to rerun the test 10 times.
