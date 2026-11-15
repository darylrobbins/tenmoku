# Breakpoint store: design calls (T656, debugger D5)

The store is `crates/fe-workbench/src/debug_breakpoints.rs`; its module docs
are the reference, and `fe_dap::Breakpoint` (`crates/fe-dap/src/breakpoint.rs`)
is the data. These are the calls §5 of `r1b9dw3g0g-debugger.md` left open.

- **One store per canonical root, app-wide.** `BreakpointStore::for_root`
  keeps one entity per root in a global, so two windows on one folder share a
  set and one writer. "Per window" would have had two stores writing one file.
  A window with no root has no store (`Workbench::breakpoint_store` is `None`).
  The state file is keyed by `layout_key` of the canonical root. The global
  holds the only lasting strong handles: a window's hand-off state lives in an
  observer closure and holds its store weakly, since a test app never closes
  its windows, and those closures outlive the app. A strong handle there was
  reported as a leak by every workbench test.
- **No call from the workbench.** `debug_breakpoints::init` watches each
  window's `file_editors` (tabs and diff views' working sides), as
  `lane_marks` does, and hands editors to the store as they come and go. The
  store watches each editor itself (notify and release), and subscribes to each
  open buffer's events, so `workbench.rs` isn't touched.
- **Two places per breakpoint (review round 1, F1, F3, F5).** Each entry has its
  row in the file on disk, the only thing persisted, and a place (a row and an
  anchor) in every open buffer of the file. Unsaved edits move only places, so
  a write never holds unsaved rows, and a close that discards edits just drops
  the buffer's places: the lines on disk were never moved. Two buffers of one
  file (two windows that opened it apart) each have their own places; the
  first open buffer's rows are what `breakpoints` returns, and
  `breakpoints_in`, `toggle_in`, `set_enabled_in` and `remove_in` take a
  buffer. A breakpoint added in one buffer is mapped to disk, and from there
  to the others, by line diff against each buffer's text on disk. The session
  side (`enabled`, `all`, `on_disk`) reads the lines on disk, which the
  running program matches, whatever any buffer holds unsaved (round 2, F8).
- **Saves set the lines on disk.** A `BufferEvent::Saved { version }` makes the
  rows of that version the lines on disk. Each buffer keeps, since its last
  save, its texts by version (from its `Edited` events) and the rows it had at
  each version the store saw (up to 1024, then the file is read). When edits
  came after the version saved (a save that lands after more typing), the rows
  recorded at that version are used, so a row deleted or joined since still
  lands on its own text (round 2, F3); a breakpoint set since is mapped back
  by line diff. The saved text becomes the file on disk for every open buffer
  of it; one whose own save it wasn't maps through it by line diff even while
  clean (round 2, F7). A buffer first seen dirty (a
  restored backup) reads the file on disk once, to place the lines in its
  text. A save under a new path is not a save of the old path (F2): its events
  are taken in without their saves, and the old path's lines stay.
- **Rows and lines.** The store's API takes rows from 0, as editors and the
  gutter count them. `Breakpoint.line` counts from 1, as `setBreakpoints` with
  `linesStartAt1` does, and the state file stores lines.
- **Anchors stay where they are.** Each anchor is set at its row's start with
  `Bias::Right` when the buffer opens or the breakpoint is set, and is only
  refreshed onto newer versions after that, not moved back to the row's start.
  So text typed at the row's start and then split with Enter leaves the
  breakpoint on the code it was set on.
- **Merging.** After every edit, a buffer's places are stably sorted by their
  new rows, so of two on one row the one that was higher up shows, with its
  settings. They merge for good when that text is saved; until then a
  discard parts them again, each on its own line on disk. Enabling or
  disabling a row sets every breakpoint shown there, so they agree.
- **Rows replaced by rows, by a line diff (F4).** `Buffer::reload` is one edit
  from the first byte that differs to the last, and so are its undo and redo and
  a formatter's rewrite; anchors inside it would all be swept to one end. When
  any batch since the last sync replaced text spanning rows with text spanning
  rows (read from the buffer's `Edited` events and the text before each), rows
  are mapped by an imara-diff line diff of the text before and after instead
  (as `git_blame` maps rows): kept rows follow their text, a replaced row keeps
  its place in the replacement, and a deleted row goes to the row that took its
  place. Typed edits (inserting or deleting whole rows, joins) never replace
  rows with rows, so they keep the anchors' behaviour, undo included.
- **When the file is written.** When the set changes, and when a save or reload
  moves lines on disk. Edits alone don't write it, as §5 says ("on close and on
  save, rows are written back"). A breakpoint set on text not saved yet is
  written on the line on disk its text would go to; if another is written there,
  the older one wins until a save.
- **Writes and quit (F6).** Changes are counted; a write records the count it
  held only once it succeeded. A failed write leaves the count behind, so the
  next change and quit write again; if changes came while it ran, it's tried
  again after the rest at once, but never again for the same count alone
  (round 2, F9). Quit writes whatever isn't known written,
  a write still running included. Writes are numbered, and one older than the
  last written is dropped, so the background write and the quit write can't
  land out of order.
- **Debounce.** A write waits `WRITE_REST` (500 ms) after the last change, and
  goes through `<file>.<pid>.tmp`, synced, then renamed over the old file.
- **Reading.** The file is read synchronously when the store is made. It is one
  small file, read once per root, before any editor opens a file in it, so no
  change made meanwhile has to be merged with what arrives. A file that can't be
  read or parsed is reported in a toast and replaced by the next change, as
  harpoon's list is. A breakpoint entry that can't be read, or that is on line
  0, is dropped with a log line. `watches`, whatever it holds, and every other
  top-level key are written back as read.
- **Known limits.**
  - A breakpoint on an empty row moves down with `o` on the row above, as §5
    says.
  - A breakpoint past the end of the file when it opens (the file got shorter
    while it was closed, or it's gone and opens empty) shows on the last row,
    where it may show as one with another; its line on disk stays until a
    save.
  - A path that isn't UTF-8 can't be held in JSON. Its breakpoints work for the
    session but aren't persisted, and a log line says so.
  - Two processes on one root: the last writer wins.
  - A file saved under a new path (Save As) keeps its breakpoints on the old
    path, and the editor follows the new one. A rename in the project panel
    doesn't carry breakpoints yet.
