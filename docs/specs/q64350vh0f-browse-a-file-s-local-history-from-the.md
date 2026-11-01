---
task_uid: "01m6c61xxsh0zty4q64350vh0f"
task: "T396"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench tests show_history_lists_entries_newest_first, choosing_an_entry_opens_its_diff, restore_replaces_the_text_as_one_undo and no_entries_shows_a_toast pass (src/local_history_view_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "history.show_local and history.restore_local are registered typed commands in the palette, and compare_saved_tests still passes unedited"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited"
    required: true
    manual: false
---

# Browse a file's local history from the palette, diff a past version against the buffer, and restore it as one undoable edit (Mac T1277)

## Problem

With local history recorded (previous two cards), the user needs to browse a file's past versions, diff one
against the buffer and restore it (user direction 2026-11-01; JetBrains' Local History > Show History).
Compare Active File with Saved (`crates/fe-workbench/src/compare_saved.rs`, `Workbench::finish_saved_diff` in
`crates/fe-workbench/src/git_diff.rs:1498`) already opens a read-only left side against the open buffer: the
same diff, with a history entry on the left.

## Change

1. New `crates/fe-workbench/src/local_history_view.rs`, declared in `crates/fe-workbench/src/workbench_commands.rs`
   next to `compare_saved`, with two commands registered there:
   - `history.show_local` "Local History: Show History" (context `editor`): lists the active file's entries
     (`fe_gpui::pane_item::local_history(cx)`, read on the background executor) in a picker built as
     `crates/fe-workbench/src/encoding_picker.rs` builds its one, newest first, each row
     `<when> · Saved|Changed on disk · <size>`, `<when>` relative under a day (`5 minutes ago`) and
     `YYYY-MM-DD HH:MM` after. No file: `E32: No file name`; no store or no entries: an info toast
     `No local history for <name>`. Choosing a row opens a diff tab: left `Local History <YYYY-MM-DD HH:MM>`,
     read-only, the entry's text; right the open buffer, as `finish_saved_diff` does it.
   - `history.restore_local` "Local History: Restore This Version" (enabled when the active tab is such a
     diff): replaces the buffer's whole text with the entry's as one undoable user edit (minimal edits, as a
     reload does, so carets and folds survive where text is unchanged); the buffer is left dirty, not saved,
     and focus moves to its editor. cmd-z undoes it.
2. `crates/fe-workbench/src/git_diff.rs`: `finish_saved_diff` takes the left side's label so both callers share
   it (`"Saved"` for compare_saved); a history diff is found again by its title as `open_saved_diff` finds one.
3. No default keybinding (JetBrains has none); both are in the palette.
4. Tests in new `crates/fe-workbench/src/local_history_view_tests.rs`, declared next to the module:
   `show_history_lists_entries_newest_first`, `choosing_an_entry_opens_its_diff`,
   `restore_replaces_the_text_as_one_undo` (restore, then undo gives the text before), and
   `no_entries_shows_a_toast`.

## Implementation notes

- The left side's label travels on `SavedAsk`, which `finish_saved_diff` takes, rather than as a parameter of
  its own: `compare_saved.rs` is out of this card's scope and its call stays as it was.
- A history diff is found by its title and its entry's id. The title has only the minute, which two entries can
  share, so the title alone would show one entry's tab for the other. `Compared::history` is also what says the
  active tab is such a diff, rather than parsing a title that a file's own name could imitate.
- kiln has no per-command enabled state, so `history.restore_local` has context `DiffView`: the palette lists it
  in any diff, and outside an entry's diff it answers with a toast and changes nothing.
- The restore's edits are by row, each cut to the bytes that differ, so carets and folds between two changed
  places survive too, which a reload's one prefix-and-suffix span would not give. They're diffed off the main
  thread; if the buffer moves meanwhile, nothing is restored and a toast says so. The edit goes in with any open
  transaction set aside, so undo takes back the restore alone during a vim insert session.
- With no buffer of the file open as the entry's read lands, a toast says so: `finish_saved_diff`'s fallback, the
  left text as the file, is right only for the saved text.
- Focus moves to the diff's right side, the buffer's editor in the active tab, so the result stays in view.
