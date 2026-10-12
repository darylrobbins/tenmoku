---
task_uid: "01m4pzqw8y0mjw7hppexzjd8ff"
task: "T26"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test pane_item::save_tests::cmd_s_saves_in_background: cmd-s starts a background save: before the executor runs the file is unchanged and is_saving() is true; once parked the file holds the text and the tab is clean; text typed before it finished stays dirty"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test pane_item::save_tests::changed_on_disk_asks: with the file rewritten by another writer, cmd-s shows the Overwrite/Cancel prompt; Overwrite writes the buffer; Cancel leaves the other writer's file, the buffer dirty and PaneItem::save's task an error"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test pane_item::save_tests::failure_is_shown: cmd-s into a read-only directory shows a prompt with the error"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test pane_item::save_tests::reads_record_stamps: a restored tab and a buffer from fe-workbench loader::read carry the stamp of what was read; pane_item_tests, fe-workbench close_tests, loader_encoding_tests and vim_host_tests pass unchanged"
    required: true
    manual: false
---

# Save off the UI thread from cmd-s, :w and close, and ask before overwriting a file changed on disk (Mac T432)

Filed on helium from Mac T432 (~/coordination/helium-cards/T432.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-i.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

After the previous card (`SaveJob`, `DiskStamp`, `SaveError::ChangedOnDisk` in `crates/fe-editor/src/disk.rs` and
fe-buffer), the app still saves on the UI thread and never asks about a conflict:

- cmd-s runs fe-editor's synchronous `editor.save`. `EditorView`'s `PaneItem::save`
  (`crates/fe-gpui/src/pane_item.rs:210`), which vim's `:w`/`:wq`/`:x`/`:wa` and the close flow use, calls
  `model.save()` and returns a ready task.
- A failed cmd-s is only logged (`dispatched`, `crates/fe-gpui/src/commands.rs` ~320): the user sees nothing.
- Hosts that read files don't record a disk stamp: `loader::read` (`crates/fe-workbench/src/loader.rs:24`, used by
  `Pane::open` and the launch file) and `PaneItem::restore` (`pane_item.rs` ~250) both build the buffer with
  `Buffer::from_reader_detecting`.

M6 is "save/encodings" (`~/.claude/plans/glittery-foraging-graham.md` line 110). User 2026-10-11: "Draft any work
in earlier phases we haven't drafted yet."

## Change

- `EditorView::save(&mut self, window, cx) -> Task<gpui::Result<()>>` in pane_item.rs: `prepare_save`, then
  `SaveJob::run` on `cx.background_spawn`, then `finish_save` on the main thread and `cx.notify()`. One save at a
  time per buffer: a save asked for while one runs waits for it, then saves the text as it is then.
  `EditorView::is_saving()` says whether one is running (the reload card skips buffers being saved).
  `PaneItem::save` returns this task.
- `SaveError::ChangedOnDisk` → `window.prompt(Warning, "“<name>” changed on disk since it was opened.",
  Some("Overwrite it with your version?"), ["Overwrite", "Cancel"])`. Overwrite runs the job again with
  `overwrite = true`. Cancel resolves the task with an error "not saved: <name> changed on disk", which vim's
  `:w` shows as its message.
- cmd-s: fe-gpui's editor registry (`editor_registry`, `crates/fe-gpui/src/commands.rs:40`) overrides
  `editor.save` in the view to start `EditorView::save`. Headless hosts keep fe-editor's synchronous command. A
  cmd-s that fails for any other reason shows a prompt with the error.
- Stamps: `loader::read` opens the file and builds the buffer with `Buffer::from_file`, and `PaneItem::restore`
  does too, so every buffer read from disk carries its stamp.

## Tests

New file `crates/fe-gpui/src/pane_item_save_tests.rs`, declared from pane_item.rs
(`#[cfg(test)] #[path = "pane_item_save_tests.rs"] mod save_tests;`).

## Implementation notes

- Stamps were already recorded on this base: T17 (fa4cc18) moved `loader::read` and `PaneItem::restore` to
  `Buffer::from_file`. C4 adds the test only.
- One save per buffer: a thread-local map from `BufferId` to the last save asked for, whose `Shared` oneshot the
  next save waits on; a guard removes the entry when the save ends or is dropped. `is_saving()` reads the map, so
  it covers splits over the same buffer. With no save running, `save` prepares the job at once, so text typed after
  cmd-s is unsaved; a queued save prepares once the one before it ends.
- The save runs on the app, not the window: its prompts go to the window's handle, so a restored editor not yet in
  a pane still saves. The returned task is fed by a oneshot, so dropping it doesn't stop the save.
- cmd-s: `kiln-command`'s `Registry` refuses a duplicate ID, so the view intercepts `editor.save` in
  `run_command` (given the window) instead of re-registering it: it validates the invocation, notifies observers
  as before, then calls `save_from_command`. `type_text` passes no window and keeps fe-editor's command. A failure
  other than Cancel shows "“<name>” couldn’t be saved." with the error.
- Outside the listed scope: `crates/fe-gpui/Cargo.toml` gains `futures` (oneshot, `Shared`) and a dev-dependency
  on `fe-workbench` so C4's test reads through `loader::read` (only fe-buffer types cross it; fe-gpui registers no
  gpui actions). `fe-workbench/src/tests/lsp_resolve_retry_pending.rs`'s `save_and_hold` saves through the model:
  it holds foreground tasks, and cmd-s now records its save in one.
- Overwrite retries the job prepared at cmd-s, not a fresh one, so text typed after cmd-s (before the executor
  ran, or while the prompt was up) stays unsaved and the tab dirty. For that, `fe-editor`'s `SaveJob` is `Clone`
  (its snapshot is a rope) and gains `SaveJob::overwriting`; review 5p68qq F1.
