---
task_uid: "01m6j99zt1mz3wmb4d4jb6vqgd"
task: "T442"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "update_hook_tests::hook_taking_the_check_skips_github passes"
    required: true
    manual: false
  - id: "C2"
    text: "update_hook_tests::hook_declining_falls_back_to_github passes"
    required: true
    manual: false
  - id: "C3"
    text: "update_hook_tests::no_hook_is_todays_behaviour passes"
    required: true
    manual: false
  - id: "C4"
    text: "update_check_tests (all existing tests) still pass"
    required: true
    manual: false
---

# Let the app binary take over Check for Updates…: an UpdaterHook global that app.check_for_updates asks first, with the GitHub check as fallback (Mac T1372)

## Problem

`app.check_for_updates` (crates/fe-workbench/src/update_check.rs, T1309) always asks GitHub. With Sparkle the app binary will own the real updater, but `fe-workbench` has no AppKit code and must stay platform-free (and Linux keeps the GitHub check).

## Change

In `update_check.rs` add a small seam: `pub struct UpdaterHook(pub Arc<dyn Fn(&mut App) -> bool + Send + Sync>)` as a GPUI Global (set with `update_check::set_updater_hook(cx, hook)`). `run()` first asks the hook: when present and it returns true (it started Sparkle's own user-driven check), nothing else happens, no toast, no GitHub request; when absent or false it does the existing GitHub check unchanged. Document it in the module comment. Export nothing else; `app.check_for_updates` and the menu item stay as they are.

## Tests

New `crates/fe-workbench/src/update_hook_tests.rs`, declared in lib.rs next to `update_check_tests`, using the fake `UpdateSource` the existing tests use: `hook_taking_the_check_skips_github`, `hook_declining_falls_back_to_github`, `no_hook_is_todays_behaviour`.
