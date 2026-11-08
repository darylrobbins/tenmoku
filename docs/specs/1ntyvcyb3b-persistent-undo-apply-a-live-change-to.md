---
task_uid: "01m6v5asty89eh6c1ntyvcyb3b"
task: "T512"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "undo_persist_tests::turning_it_off_in_a_project_file_stops_later_reads_and_writes: a live edit of a non-launch root's project settings to editor.persistent_undo = false, through a real ConfigService (no direct set_window_persistent_undo call), stops that window's later reads and newly captured writes"
    required: true
    manual: false
  - id: "C2"
    text: "undo_persist_tests::a_masked_user_change_follows_the_root_that_applies: a user-settings change masked by the root's override leaves the window's policy as the root says, and lifting the override applies the user value"
    required: true
    manual: false
  - id: "C3"
    text: "T483's generation and root checks still hold: tests::a_settings_load_overtaken_by_a_re_root_is_discarded and tests::settings_loads_completing_out_of_order_keep_the_newest (fastedit main.rs) and undo_persist_tests::setting_off_writes_nothing pass"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-gpui -p fe-workbench -p fastedit pass"
    required: true
    manual: false
---

# Persistent undo: apply a live change to editor.persistent_undo from any of a window's settings sources (follow-up to T483)

Follow-up to T483 (persistent undo, lent from Mac T1411), from its review finding F9 (medium, round 5). T483 carries it as a Known limit.

# Persistent undo: apply a live change to editor.persistent_undo from any of a window's settings sources

## Problem
T483 sets each window's undo-persistence policy in `give()` (`crates/fastedit/src/main.rs`, `fe_gpui::pane_item::set_window_persistent_undo`). Live reloads reach `give()` only through the `GlobalSettings` observer. That observer misses two cases:
- Editing a non-launch root's project settings emits `ChangedFiles` but doesn't update `GlobalSettings`.
- A user-settings change can be masked by the launch root's override.

In both cases, setting `editor.persistent_undo = false` never reaches the window's policy. Later reads and newly captured writes stay enabled until the window re-roots or the app restarts. The reverse (false to true) is missed the same way.

On helium, per-root settings reach a live window through `crates/fe-workbench/src/workbench_root.rs` `reload_settings` (around line 227). `Pane.settings` is private, and `GlobalSettings` misses per-root settings.

## Change
- Refresh a window's persistent-undo policy whenever the settings that actually apply to that window change: its root's project settings file, and user settings even when the launch root overrides them. Route it through `reload_settings` (or the same per-root settings path), not the `GlobalSettings` observer.
- Keep T483's generation and root checks. A reload that is no longer the window's newest must not change the policy.
- Turning it off stops later reads and newly captured writes for that window, and removes nothing, as T483's setting does. Queued background writes stay as T507 leaves them.

## Tests
Add to `crates/fe-workbench/src/undo_persist_tests.rs`. Use a real ConfigService and change the files on disk; don't call `set_window_persistent_undo` directly:
- `undo_persist_tests::turning_it_off_in_a_project_file_stops_later_reads_and_writes`: a window rooted at a non-launch project, whose `.fastedit` settings file is edited live to set `editor.persistent_undo = false`. After that, a save writes no record and an open imports none.
- `undo_persist_tests::a_masked_user_change_follows_the_root_that_applies`: a user-settings change that the root overrides leaves the policy as the root says, and lifting the override applies the user value.

These regression tests must compile against the APIs T483 lands with. If driving a real ConfigService from fe-workbench tests needs a helper, put it in the test file.

## Not in scope
T507's queued-writer race, and settings other than `editor.persistent_undo`.
