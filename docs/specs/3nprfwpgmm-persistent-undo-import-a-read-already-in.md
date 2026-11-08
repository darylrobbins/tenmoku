---
task_uid: "01m6v1fj7mgk6srz3nprfwpgmm"
task: "T509"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "undo_persist_tests::a_read_in_flight_when_the_window_re_roots_is_imported_once_its_settings_are_in: a read started before the window begins waiting for its root's settings (wait_for_window_persistent_undo, as a re-root calls it) and landing during the wait is imported once set_window_persistent_undo gives the window the setting on, at every interleaving the executor-stepping sweep reaches, and u gives back the text the record's history starts at"
    required: true
    manual: false
  - id: "C2"
    text: "undo_persist_tests::a_read_in_flight_when_the_window_re_roots_onto_a_root_with_it_off_imports_nothing: the same read, with the new root's settings giving the setting off, imports nothing, and the record on disk is left byte-for-byte as it was"
    required: true
    manual: false
  - id: "C3"
    text: "undo_persist_tests::a_buffer_edited_while_its_window_waits_gets_no_history: the deferred read keeps T483's freshness and hash checks: a buffer given a user edit during the wait gets no history once the settings are in, and u undoes only that edit"
    required: true
    manual: false
  - id: "C4"
    text: "undo_persist_tests::a_window_waiting_for_its_settings_reads_once_they_are_in, undo_persist_tests::reopening_a_saved_file_lets_u_undo_the_last_edit and undo_persist_tests::a_file_changed_on_disk_between_opens_starts_with_no_history still pass"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-gpui -p fe-workbench pass"
    required: true
    manual: false
---

# Persistent undo: import a read already in flight when the window re-roots, once the new root's settings are in (follow-up to T483)

Follow-up to T483 (persistent undo, lent from Mac T1411), from its review round 4 finding F7 (medium, on spec rev 6). T483 carries this as a Known limit. When T483 is cut back to the Mac, this card goes with it.

# Persistent undo: a read in flight when the window re-roots is imported once the new root's settings are in

## Problem
T483 reads a file's undo record off the main thread when the file's first editor opens. `start_undo_read` (`crates/fe-gpui/src/pane_item.rs`) checks the window first. If the window is waiting for its root's settings (`UndoPersist::waiting` holds it, set by `wait_for_window_persistent_undo`), the read is deferred: `(buffer, path)` is pushed onto the window's list, and `set_window_persistent_undo` starts it again once the settings are in. Otherwise the read is spawned at once, and when it lands `import_undo_into` gives the record to the buffer.

A re-root makes the window wait again (`watch_window`'s `RootChanged` subscription in `crates/fastedit/src/main.rs` calls `wait_for_window_persistent_undo`, then `settle`). A read already in flight at that moment isn't deferred. If it lands during the wait, `import_undo_into` finds `undo_store_for(window, cx)` is `None` (the window is in `waiting`) and returns at the `if !fresh || undo_store_for(window, cx).is_none()` check. The record is dropped, and nothing adds `(buffer, path)` to the window's deferred list. When the new root's settings come in with `editor.persistent_undo = true`, `set_window_persistent_undo` releases only the reads that were started during the wait. The open buffer has unchanged text and a matching record on disk, but `u` has no history. Only reads that start during the wait are deferred.

The review asked: "Defer or restart an in-flight read that completes while its window awaits settings, retaining the existing freshness checks. Test read-start → re-root → read-completion → enabled-settings-completion."

## Change
- In `import_undo_into`, tell "the window is waiting for its settings" apart from "the window has it off". When the buffer's window (`buffer_window`) is in `UndoPersist::waiting`, don't drop the read. Push `(buffer, path)` onto that window's deferred list, as `start_undo_read` does, and return. Skip it if that pair is already there. `set_window_persistent_undo` then restarts it like any other deferred read. The restarted read takes a write still waiting over the disk, as reads already do.
- Restart rather than keep the record that landed. The restarted read and its import repeat every check T483 makes: the window's policy as the new settings give it, the freshness check (`history_revision() == 0`, same buffer, same path), and the text hash in `Buffer::import_undo`. A buffer edited during the wait still gets no history. A window whose new root has the setting off reads nothing. A buffer whose editors all closed during the wait is skipped, as `import_undo_into` already skips one with no live editor.
- An overtaken settings load (T483's round 3 F1 fix in `finish_load`, `crates/fastedit/src/main.rs`) still releases nothing. Only the window's newest settings release the deferred reads. This card does not touch main.rs.
- A window that has the setting off (not waiting) still drops the read, as now.

## Tests
Add these to `crates/fe-workbench/src/undo_persist_tests.rs` (`undo_persist_tests::...`), which exists once T483 lands.
- `undo_persist_tests::a_read_in_flight_when_the_window_re_roots_is_imported_once_its_settings_are_in` (the regression test). Stage read start, then re-root (`wait_for_window_persistent_undo`, as a re-root calls it), then read completion, then settings completion with the setting on (`set_window_persistent_undo(window, Some(true))`). The read's two halves (the foreground `cx.spawn` and its `background_spawn`) are separate tasks, so step the executor. In a loop over `k = 0, 1, 2, …`: make the two editors with `Fixture::two_editors_reading` (a record of typing ` two` on disk), run `k` single tasks with `f.cx.executor().tick()`, then call `f.wait_for_window_undo()`, `draw`, then `f.set_window_undo(true)`, `draw`. Assert the buffer has the history: `has_history`, and `u` through the buffer gives back `one\n`. Then drop the editors, `draw`, and stop once `tick()` returned false before `k` ticks ran. Cap `k` (for example at 200) with an assertion so the loop always ends. At some `k` the read is in flight when the wait starts. T483 drops it there, which fails the test.
- `undo_persist_tests::a_read_in_flight_when_the_window_re_roots_onto_a_root_with_it_off_imports_nothing`: the same sweep, ending with `f.set_window_undo(false)`. Skip the `k` where the history was already imported before the wait (check `has_history` just before `wait_for_window_undo`). In every other case there is no history, and the record on disk is byte-for-byte as it was.
- `undo_persist_tests::a_buffer_edited_while_its_window_waits_gets_no_history`: the same sweep. After the wait's `draw`, edit the buffer (an `Origin::User` insert through `buffer_mut` in an editor update), then `f.set_window_undo(true)`, `draw`. Skip the `k` already imported before the wait. Nothing is imported, and `u` undoes only that edit.
- `undo_persist_tests::a_window_waiting_for_its_settings_reads_once_they_are_in`, `undo_persist_tests::reopening_a_saved_file_lets_u_undo_the_last_edit` and `undo_persist_tests::a_file_changed_on_disk_between_opens_starts_with_no_history` still pass.

The regression test must compile against the APIs T483 lands with (the base). It uses only `Fixture::two_editors_reading`, `wait_for_window_undo`, `set_window_undo`, `has_history`, `draw` and gpui's `executor().tick()` (used the same way in `auto_save_tests.rs` and `git_blame_tests.rs`). Otherwise the regression check fails as infra. Keep any new fixture helper in the test file, and build it on base APIs.

## Not in scope
The fallback rule for an editor with no known window (`undo_store_for(None, …)` is off while any window waits): a read for one isn't started then, and one landing then is dropped, as T483 has it. Queued writes after a window turns the setting off or starts waiting (T507). Vim insert mode entered before the read lands (T508). If T508 has landed first with a deferred retry in `import_undo_into`, compose with it: the retries must not double-import (a second import is refused as `Edited`, harmlessly, but don't rely on that). The import's effect on Vim marks and folds (the other round 4 follow-up). Changes to main.rs's settings loading.
