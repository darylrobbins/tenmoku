---
task_uid: "01m4rwgetdjkdz24gjckjxh9ea"
task: "T54"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "search_replace_tests::confirm_counts: search.replace_all asks \"Replace 5 occurrences across 2 files with “bar”?\"; Cancel changes nothing"
    required: true
    manual: false
  - id: "C2"
    text: "search_replace_tests::open_and_unopened: with a.rs open (dirty or not) and b.rs not open, Replace All edits a.rs's buffer as one transaction left unsaved (one cmd-z restores it, and with vim on one u does) and writes b.rs on disk with its encoding and line endings kept"
    required: true
    manual: false
  - id: "C3"
    text: "search_replace_tests::per_file_and_dismiss: Replace in File touches only that file, and a dismissed file is skipped by Replace All; the status line then reads \"Replaced N occurrences in M files\" and the search runs again"
    required: true
    manual: false
  - id: "C4"
    text: "search_replace_tests::stale_and_errors: text changed since the search is re-matched (no stale ranges applied), and an unreadable file is skipped and named in the status line while the others apply"
    required: true
    manual: false
  - id: "C5"
    text: "search_replace_tests::results_keys: search.focus_results, select_next/select_previous and open_selected move through match rows and open the match in the active pane; with vim on the open editor is in normal mode and the results element carries the vim flag"
    required: true
    manual: false
  - id: "C6"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Replace across the project from the search panel: all matched files or one file, with counts first, one undo step per open file, unopened files saved (Mac T507)

Filed on argon from Mac T507 (card ~/coordination/argon-cards/T507.json rev 2). Mac task uid 01m4rhe05wb5mtx0sr4b3zxvnc. Its dependency Mac T506 is argon T50 (9310506), already on argon main, so it isn't recorded here. Title the host commit "... (Mac T507)".

## Problem

The project search panel finds across the project but can't replace (day-one audit 2026-10-12: search_panel.rs
"has no replace"; T460 adds include/exclude fields only). The previous card adds `fe_project::Replacer`.

## Change

`crates/fe-workbench/src/search_panel.rs`, with the apply logic in a new `crates/fe-workbench/src/search_replace.rs`
declared from it:

- A replace input under the query (shown by a chevron or `search.toggle_replace`), a Replace All button, and on
  each file row a "Replace in File" button and a "Dismiss" button (drops the file from the results, so Replace All
  skips it: the "selected results" of VS Code).
- `search.show_replace` "Replace in Files…" shows the panel with the replace input shown and focused (query seeded
  from a one-row selection as `search.show` does).
- `search.replace_all` "Replace All in Files" (in the palette) asks first, with the counts: "Replace 12 occurrences
  across 3 files with “bar”?" (Replace / Cancel). The counts are the panel's current results.
- Applying, per file, re-matching the file's current text with `Replacer` (never the possibly stale results):
  - a file open in any editor of any window (`Workbench::open_editors`): one transaction on its buffer, left
    unsaved, so cmd-z (or vim `u`) in that buffer undoes the whole file's replacement in one step;
  - a file not open: read off the UI thread through `crate::loader::read` (encoding and line endings kept), edited
    as one transaction on a headless `EditorModel`, saved off the UI thread (`prepare_save(false)`, refusing a file
    changed since it was read). Known limit, stated in the module docs: this change can't be undone from the UI.
  - Unreadable, binary or refused files are skipped and listed in the panel's status line; the rest still apply.
- Afterwards the status line reads "Replaced 12 occurrences in 3 files" and the search runs again.
- Keyboard: the results list takes focus (`search.focus_results`, and down-arrow from the query input), with
  `search.select_next`, `search.select_previous` and `search.open_selected` on the selected match row. The results
  list's element sets the key context `search_results` and adds the `vim` flag to its key context while `fe_gpui::vim_enabled`, for the keys card's j/k. The
  inputs are `text_input`, so vim never sees keys typed there.

## Tests

New file `crates/fe-workbench/src/search_replace_tests.rs`, declared from search_replace.rs, one search thread as
`tests/search_panel.rs` uses.
## Rework round (lead, after review rounds 1-3 on candidate 2c0f23a)

Keep everything already built; C1, C5 and C6 are met. Fix exactly these three findings in the save path of
crates/fe-workbench/src/search_replace.rs. Add one test per finding to search_replace_tests.rs, and make sure each
test fails on 2c0f23a:

- F2 (high, :236): the project boundary must hold through the write, not only at still_as_read. Don't follow a path
  that has been swapped since the check: re-check the destination's resolved parent (canonicalized) is still inside the
  project root right before the write, and have the save refuse with a reported error, not write, if it isn't.
  Test: replace a parent directory with a symlink to a directory outside the project between the read and the save
  (hold the save job as the existing tests do), and assert nothing outside the project is written and the file is
  reported as refused.
- F10 (high, :237): keep the stamp the save job returns and record that one. Never re-read it from the pathname
  afterwards. Test: another writer changes the file right after the save completes and before reconciliation; the
  buffer's recorded stamp must be the save's, so the next save of that buffer detects the external change instead of
  overwriting it.
- F11 (high, :264): reconciliation must update saved_text together with disk_stamp, so an editor's clean/dirty state
  and its undo history agree with the disk. Test: a clean editor opened during the save, then one undo, must show
  dirty (its text differs from disk), and closing it must warn rather than discard silently.

F12 and F13 (medium) may be fixed in passing if they're on the same lines; they aren't required this round.
Nothing else changes. Stay in scope; dock.rs's registration stays as justified before.

## Implementation notes

- **One change outside the scope: `crates/fe-workbench/src/dock.rs`.** The new commands (`search.show_replace`,
  `search.replace_all`, `search.toggle_replace`, `search.focus_results`, `search.select_next`,
  `search.select_previous`, `search.open_selected`) are registered in the dock's registry, beside the other `search.*`
  commands. That registry is the only one `command_catalog()` and the palette's `command_list()` read for search
  commands, so without it the palette couldn't list "Replace All in Files" and a keymap binding to them (the keys
  card's j/k) would be refused as unknown. `Dock::run` also does `search.show_replace` and `search.replace_all` from
  the workbench's context (an editor focused), as it does `search.show`. Nothing else outside the scope changes
  (no `lib.rs`, `workbench.rs` or keymap edits).
- `search_replace.rs` is declared from `search_panel.rs` with `#[path]`, so it is `search_panel::search_replace`.
- `search.show_replace` from outside the panel seeds the query from the active editor's selection when it's the only
  one, nonempty and within a row, and runs it (argon's `search.show` doesn't seed, so this is the new command's own).
- The project folder is fixed when the search runs (`Boundary`: its resolved path, device and inode). Every file is
  opened beneath it a component at a time with `openat` and `O_NOFOLLOW` (the last with `O_NONBLOCK`), to check it,
  to read it and to write it, so a folder or file swapped since for a symlink, FIFO or another folder is skipped, as
  is the whole run if the folder itself moved. Reading is `Buffer::from_file` on that file, as `loader::read` does.
- Rework round (F2, F10): a file not open is no longer saved through `prepare_save`/`SaveJob`, which takes a path and
  follows it. `write_beneath` does what that save does (a new file beside it, written from the buffer's
  `SaveSnapshot`, synced, given the old file's permissions, renamed over it) through the folder's handle opened beneath
  the project, so it writes in that folder or nowhere. Right before the rename the folder is opened from the project
  root again and must be the same folder (device, inode), and the file must still have the stamp it was read with;
  otherwise the new file is removed and the file is reported as refused. The stamp recorded afterwards is the written
  file's own (taken from its handle), never looked up again by path.
- The project folder itself is opened from `/` a component at a time with `O_NOFOLLOW` too (its resolved path has no
  symlink), then checked by device and inode, so a folder above the project moved and replaced by a symlink to it is
  refused ("the project folder has moved"), not followed.
- Folders on the way to a file are opened only to look things up in them (`O_SEARCH` on macOS, as fe-project's walk
  opens them; `O_PATH` on Linux), so a project beneath a folder that can be searched but not listed is still
  replaced in. The file's own folder is opened again to read only to sync the rename.
- Unopened files are saved one at a time. Before each save, a pane still reading the file is waited for (no time
  limit) and a file open by then is replaced in its editor instead, checked before the file's read is looked at (so
  a read error or no match on disk doesn't skip an editor that has one: F12). After the save, reads that started
  meanwhile are waited for too, and an editor that loaded the file as it was read (its stamp) gets the replacement on
  its current text. Rework round (F11): an unedited one then holds exactly what the save wrote, so it is marked saved
  (`did_save_snapshot` of its own snapshot, with the written file's stamp): clean, and one undo makes it dirty. An
  edited one holds neither the file it loaded nor the file on disk, and fe-buffer has no way (in this scope) to set
  its saved text to the disk's, so it keeps both the saved text and the stamp it loaded: they stay together, and
  saving it asks before overwriting, as for any file changed on disk. If its buffer is binary its editor is left as
  it is and the file is named ("saved, but not in its editor"); matches it has beyond the file's are counted (F13).
  Tests hold a save with a test-only gate (`hold_save`) and act inside the write with a test-only hook (`on_write`).
- A byte-oriented regex can match inside a char: each match is widened to whole chars and overlapping matches are
  merged into one replacement of their expansions, for open and unopened files alike. Open buffers with a NUL are
  skipped as binary too. A file open in two windows (two buffers) is replaced in both and counted once.
- Down from the query input is the input wrapper's key-down listener (no keymap edit); the other results keys are
  commands for the keys card to bind.
- The replacement re-matches with the query the results were searched with, not the query field's text if edited
  since. "Replaced N occurrences in M files" (and any skipped files, shown as an error) stays the status line through
  the rerun, until the user runs another search.
- Binary files are those whose text has a NUL, as the search's binary detection quits on NUL.
