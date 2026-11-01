---
task_uid: "01m6c61xkp63yvjtzf4tns3tr1"
task: "T395"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench tests a_save_records_the_saved_text, saving_unchanged_text_records_once, an_external_change_records_the_disk_text, a_dirty_buffer_records_the_disk_text_before_asking and no_store_no_recording pass (src/local_history_record_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "Only crates/fastedit/src/main.rs installs the store (rg -n set_local_history finds no other caller outside tests), so no test writes to the real state dir"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-gpui -p fe-workbench passes with no existing test edited"
    required: true
    manual: false
---

# Record a file's text in local history on every save and on every change found on disk, under ~/.local/state/tenmoku/history (Mac T1276)

## Problem

The local history store (`fe_buffer::local_history::LocalHistory`, previous card) records nothing until the app
feeds it. User direction 2026-11-01: snapshot "on save (and on external change)".

## Change

1. `crates/fe-gpui/src/pane_item.rs`: an App global holding an optional `LocalHistory`,
   `pub fn set_local_history(cx: &mut App, history: Option<LocalHistory>)` (reached as `fe_gpui::pane_item::set_local_history`,
   as `set_window_backups` is; no `crates/fe-gpui/src/lib.rs` change, which T659/T1168 reserve). In `save_asking`, once `finish_save` reports the save `recorded`, the saved text
   (`Saved::snapshot()`'s text, UTF-8) is recorded with `Reason::Saved` on the background executor; a failed
   recording is logged at debug and never fails or delays the save. No global, no recording: tests and every
   other caller see no change.
2. `crates/fe-workbench/src/disk_sync.rs`: when a check reads a file whose text differs from the buffer's
   (`read_landed`, before reloading or asking Reload / Keep Mine), the file's text is recorded with
   `Reason::External`, on the background executor, through the same global (read with a small
   `pub fn local_history(cx: &App) -> Option<LocalHistory>` accessor next to the setter).
3. `crates/fastedit/src/main.rs`: in the existing `if let Some(dir) = fe_config::paths::state_dir()` block (line
   ~417), `fe_gpui::pane_item::set_local_history(cx, Some(LocalHistory::new(dir.join("history"))))`, and one
   `prune(now)` on the background executor at launch. Only the app installs it, so no test writes to the real
   state dir.
4. Tests in new `crates/fe-workbench/src/local_history_record_tests.rs`, declared with `#[cfg(test)] #[path]` at
   the end of `crates/fe-workbench/src/disk_sync.rs` (not `lib.rs`, which T1062/T1063 reserve), with a `LocalHistory` on a tempdir:
   `a_save_records_the_saved_text`, `saving_unchanged_text_records_once`, `an_external_change_records_the_disk_text`
   (a clean buffer reloads: one `external` entry with the new text), `a_dirty_buffer_records_the_disk_text_before_asking`,
   and `no_store_no_recording` (no global set: saving works and nothing is written).
