---
task_uid: "01m6tv54mw7et75gwcdekxytk2"
task: "T507"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "undo_persist_tests::a_queued_write_of_a_window_turned_off_is_not_written and undo_persist_tests::a_queued_removal_of_a_window_turned_off_removes_nothing: a writer held back while the window had editor.persistent_undo on, released after the window turned it off and with nothing quitting, neither creates, overwrites nor removes the file's record"
    required: true
    manual: false
  - id: "C2"
    text: "undo_persist_tests::a_queued_write_of_a_window_waiting_for_its_settings_is_not_written: the same for a window that started waiting for its root's settings (wait_for_window_persistent_undo) after the write was queued"
    required: true
    manual: false
  - id: "C3"
    text: "undo_persist_tests::quitting_drops_waiting_writes_of_a_window_since_turned_off and undo_persist_tests::a_window_with_it_off_writes_nothing_while_another_writes still pass: the background writer checks the window's current policy under the writing lock just before each write or removal, and a window that keeps history still writes while another has it off"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-gpui -p fe-workbench pass"
    required: true
    manual: false
---

# Persistent undo: drop a queued undo write when its window turns editor.persistent_undo off or starts waiting for its settings (follow-up to T483)

Follow-up to T483 (persistent undo, lent from Mac T1411), from its review round 3 finding F4 (medium; round 2's F4, not fixed). T483 carries this as a Known limit. When T483 is cut back to the Mac, this card goes with it.

# Persistent undo: a write queued before a window turned editor.persistent_undo off, or began waiting for its settings, is not written

## Problem
T483 writes a file's undo record on the background executor. `queue_undo_write` (`crates/fe-gpui/src/pane_item.rs`) puts the capture in `UndoWrites::pending`, keyed by path, with the window it was captured for. It then spawns a writer that calls `UndoWrites::flush`. The writer waits on the `writing` mutex, then writes or removes whatever capture is newest for the file.

The policy is checked only when the capture is queued (`undo_store_for`). Quitting (`write_all_on_quit`) also drops captures whose window has since turned the setting off (`forget_unless`). Ordinary writers do no check of their own. A writer queued while the window had persistence on can wait behind `writing` or behind a failed write. If the window turns `persistent_undo` off meanwhile (`set_window_persistent_undo(window, Some(false))`), or starts waiting for its root's settings (`wait_for_window_persistent_undo`, on a re-root), the writer still creates, overwrites or removes the record when it runs. `flush` discards the capture's window, and neither of those two calls touches `pending`.

The review asked: "Invalidate pending captures when their window disables persistence or starts waiting for settings, and make background workers honor the current policy before performing a queued operation. Add a regression that holds a writer back, disables persistence, then releases it without quitting; cover writes and removals."

## Change
- In `set_window_persistent_undo` (when it turns a window off) and in `wait_for_window_persistent_undo`, drop the captures in `pending` that were taken for that window, as `forget_unless` does at quit. A capture with no known window (`window: None`) follows the same fallback rule as `undo_store_for`: drop it as soon as any window has the setting off or is waiting for its settings.
- Make the background writer check the current policy again just before it writes or removes a record. It runs off the main thread, so it cannot read the `UndoPersist` global. Give it shared state it can read under the `pending` lock. Two options: a per-window generation that the calls above bump, with each capture holding the generation it was taken under; or a shared map of each window's on/off/waiting state. The writer skips a capture whose window is now off or waiting, or whose generation is stale. Do the check and the write as one step under `writing`. A window that turns the setting off must not lose its turn to a writer that has already checked.
- Turning a window back on does not bring back a dropped capture. The next save or close of a clean buffer captures again.
- A newer capture from a window that still keeps history (another window on the same file) is still written.
- Removals ("no history left to keep") follow the same rule as writes.

## Tests
Add these to `crates/fe-workbench/src/undo_persist_tests.rs`, which exists once T483 lands. Each one holds a writer back with nothing quitting, then lets it run:
- `undo_persist_tests::a_queued_write_of_a_window_turned_off_is_not_written`: a write is queued while the window has the setting on and held back. The test turns the setting off and lets the writer run. The record file is not created, and an existing one is not overwritten.
- `undo_persist_tests::a_queued_removal_of_a_window_turned_off_removes_nothing`: the same for a removal (a save that leaves no user history). The old record is still on disk.
- `undo_persist_tests::a_queued_write_of_a_window_waiting_for_its_settings_is_not_written`: as the first test, with `wait_for_window_persistent_undo` in place of turning the setting off.

A writer can be held back with the trick the T483 tests already use: put a directory where the record file goes, so the write fails and the capture stays waiting, then remove the directory before the retry. If that cannot stage the race, a `#[doc(hidden)]` test hook in pane_item.rs that holds the `writing` mutex is acceptable. Document it in the Implementation notes. The existing `undo_persist_tests::quitting_drops_waiting_writes_of_a_window_since_turned_off` must still pass.

## Not in scope
The stale-settings race, where a late settings load overrides a re-root (T483 F1). T483 fixes that itself. The Vim insert-mode import (the other T483 follow-up). Writes from another process.

## Implementation notes

The setters, `set_undo_store` and `set_restore_style` publish an `UndoPolicy` (each known window's effective setting from `undo_store_for`, plus the fallback for other windows and captures with no window) through `UndoWrites::publish`, which drops captures the policy disallows. `flush` holds the `policy` lock from its check to the end of the write or removal, so `publish` waits for an operation in flight and none lands after it returns (the setter can block for one file write). Test hook: `#[doc(hidden)] pub fn flush_undo_writes_for_test` runs the held-back writers. Unit tests in `undo_writes_tests` cover the contention (`publishing_waits_for_the_write_in_flight`, `a_policy_published_during_a_write_holds_back_the_next_writers`).
