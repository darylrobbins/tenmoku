---
task_uid: "01m6tv560tvxk18ccfra4hgs4x"
task: "T508"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "undo_persist_tests::insert_mode_entered_before_the_read_lands_still_gets_the_history: with a file's undo record on disk, entering Vim insert mode (an open transaction that has changed nothing) before the read lands, then leaving it without typing, still imports the history, and u undoes the persisted edit"
    required: true
    manual: false
  - id: "C2"
    text: "undo_persist_tests::typing_before_the_read_lands_still_gets_no_history: an import is still refused once the buffer has real edits (a typed character in the open transaction), and u undoes only that edit"
    required: true
    manual: false
  - id: "C3"
    text: "undo_persist_tests::a_read_landing_in_an_open_empty_transaction_keeps_the_record (T483's Known-limit pin) is updated to expect the import, and still asserts the text and the record on disk are left as they were; undo_persist_tests::reopening_a_saved_file_lets_u_undo_the_last_edit and undo_persist_tests::a_file_changed_on_disk_between_opens_starts_with_no_history still pass"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-buffer -p fe-gpui -p fe-workbench pass"
    required: true
    manual: false
---

# Persistent undo: import a file's undo history when Vim insert mode was entered before the read completed (follow-up to T483)

Follow-up to T483 (persistent undo, lent from Mac T1411), from its review round 3 finding F5 (medium). T483 carries this as a Known limit, pinned by `undo_persist_tests::a_read_landing_in_an_open_empty_transaction_keeps_the_record`. When T483 is cut back to the Mac, this card goes with it.

# Persistent undo: import a file's history when Vim insert mode was entered before the read completed

## Problem
T483 reads a file's undo record off the main thread when the file opens. `import_undo_into` (`crates/fe-gpui/src/pane_item.rs`) then gives the record to the buffer if the buffer is still fresh, meaning `history_revision() == 0`.

Entering Vim insert mode (`i`, `a`, `o` with no typing yet) opens a buffer transaction (`Buffer::start_transaction`). That transaction pushes an empty `HistoryEntry` and sets `open`, but it does not advance `history_revision`. So the freshness check passes. `Buffer::import_undo` (`crates/fe-buffer/src/buffer.rs`) then refuses with `UndoImportError::Edited`, because `self.open.is_some() || !self.history.is_empty()`. `import_undo_into` logs the refusal and drops the record, and nothing retries the import after Escape closes the empty transaction. `finish_transaction` removes the empty entry, so the text and history end exactly as they were before. Yet `u` has nothing to undo, even though the file is unchanged and the record matches.

The review asked: "Defer the import while an unedited transaction is open and retry when it closes, or safely preserve that transaction while installing the history. Continue rejecting imports after actual edits. Add a delayed-read regression that enters insert mode, lets the read complete, exits without typing, and then undoes the persisted edit."

## Change
Pick one approach and record the choice in the Implementation notes:
- **Defer and retry:** when `import_undo_into` finds an open transaction that has changed nothing (a new fe-buffer predicate, for example `Buffer::open_transaction_is_empty`, or the import error telling an empty open transaction apart from real history), keep the record for the buffer and retry once the transaction closes. The retry can run on the next notify of one of the buffer's editors, or on a buffer hook. It must not race a later edit: re-run the full freshness and hash checks at retry time. The record is dropped as soon as the buffer gets real history, and when the buffer's last editor closes.
- **Install beneath the open transaction:** have `Buffer::import_undo` accept an open transaction with no batches. Install the history below it, so the open transaction stays open and on top with its id and depth, and Escape closes it as before (removing it, since it is empty). Typing that follows still extends that same transaction.

Either way:
- An import still fails after real edits: a typed character, or any batch in the open transaction.
- The text, every editor's selections and the Vim mode are as they were. Imports through split editors behave as in T483.
- The text's hash must still match the record's. A file changed on disk still gets no history.

## Tests
- Add `undo_persist_tests::insert_mode_entered_before_the_read_lands_still_gets_the_history` to `crates/fe-workbench/src/undo_persist_tests.rs` (the regression test). A record of typing ` two` is on disk. The file is opened, and insert mode is entered, or a `User` transaction opened on the buffer, before the read lands. Then the read lands, insert mode is left without typing, and `u` gives back `one\n`.
- Add `undo_persist_tests::typing_before_the_read_lands_still_gets_no_history`. Insert mode is entered and a character typed before the read lands. No history is imported, and `u` undoes only the typed character.
- T483's pin `undo_persist_tests::a_read_landing_in_an_open_empty_transaction_keeps_the_record` asserts the limit this card removes. Change its no-history assertion to expect the history, or fold the test into the regression test. Its record-untouched assertion stays.
- With the install-beneath approach, add a fe-buffer test to `crates/fe-buffer/tests/undo_store.rs` (flat, bare id): `import_beneath_an_empty_open_transaction_keeps_it_open`.

The regression test must compile against the APIs T483 lands with (the base): drive insert mode through the fixture's `keys`, or through `start_transaction` / `end_transaction` on the buffer inside an update before the background read runs (see `Fixture::two_editors_reading`). Otherwise the regression check fails as infra.

## Not in scope
Imports refused because the buffer really was edited before the read landed. Those stay refused, as T483 documents. Background writes after a window turns the setting off are the other T483 follow-up.

## Implementation notes

Chose "install beneath the open transaction": `Buffer::import_undo` accepts an open transaction that is the only history entry and has no batches. It sets that entry aside, rebuilds the history, then puts it back on top with its id and depth. Real edits in the open transaction still fail with `Edited`.
