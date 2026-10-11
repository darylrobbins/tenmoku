---
task_uid: "01m4p6c3xre10y25a230gaggzf"
task: "T15"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test file_viewer::pane_item_tests::is_a_read_only_pane_item: a FileViewer over a generated file has its file name as tab title and its path, is never dirty, save gives None, closing never asks, its status items are Read-only and Ln/Col, and split gives a viewer of the same file at the same selection"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test file_viewer::pane_item_tests::serialize_and_restore: serialize then restore reopens the same path with the head at the same offset; restoring a deleted file gives None"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test file_viewer::pane_item_tests::changed_on_disk_shows_reload: after the index completes, truncating or rewriting the file and advancing the clock 2 s shows the 'changed on disk.' banner with Reload and emits Changed once"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test file_viewer::pane_item_tests::reload_swaps_in_the_new_file: Reload shows the rewritten file's rows with the selection clipped and the scroll row kept, restores the host's banner and emits Reloaded; when the reopen fails the banner says 'Cannot read' and keeps Reload"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fe-gpui/src/view.rs, crates/fe-gpui/src/element.rs and crates/fe-gpui/src/lib.rs are unchanged"
    required: true
    manual: false
---

# Make FileViewer a pane item that notices its file changing on disk and reloads from a banner (Mac T397)

Filed on helium from Mac T397 (~/coordination/helium-cards/T397.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-c.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

`FileViewer` (`crates/fe-gpui/src/file_viewer.rs`, T274) paints a read-only `ViewerModel` over a
`pread`-backed `FileView` for files of 512 MiB and more (docs/decisions/mmap-read-only.md), but
nothing hosts it: `rg FileViewer crates/fe-workbench crates/fastedit` finds nothing. T274's spec:
"Hosting it in a workbench tab is the next card", and "the workbench only logs it for now (the
reload banner isn't in this chain)". The card that would have made it a pane item (T290) was
abandoned for kiln. Two pieces are missing before the workbench can host it:

- it isn't a `kiln_shell::PaneItem`, so a pane can't hold it as a tab;
- when its file changes on disk it emits `FileViewerEvent::Changed` once and nothing else, and it
  only notices while the line index is still building. The decision's draft card asks for a
  "file changed, reload" banner (mmap-read-only.md:154-160).

This card is the GPUI-side half; the workbench card that routes large files here depends on it.

## Change

All in fe-gpui's file viewer; `view.rs`, `element.rs` (the editor's) and `lib.rs` don't change.

- **Pane item.** In the new file `crates/fe-gpui/src/file_viewer/pane_item.rs`, implement
  `kiln_shell::PaneItem` for `FileViewer`, following `EditorView`'s (`crates/fe-gpui/src/pane_item.rs`):
  tab and window titles from the file name and path, `path` its `FileView::path()`, never dirty,
  `save` `None`, closing never asks, `shares_document` with a viewer of the same path, `split` a new
  viewer over the same `Arc<FileView>` and `Arc<LineIndex>` at the same selection. Status items: a
  `Read-only` item, and the cursor item (`Ln 12, Col 5`) under the editor's built-in id so it shows
  in the same place. `serialize` stores the path and the head offset; `restore` opens the path with
  `FileView::open` and `LineIndex::build` on the background executor and puts the head back,
  clipped (or `None` if it can't be opened). Registering it for layout restore is the workbench's.
- **Staleness.** Once the index is complete, the viewer checks `FileView::check()` every 2 s on
  the foreground executor while it is in a window, and when it gets focus. A stale result, a read
  error (`FileText::error()`) or an index that stopped short of `Complete` switches the banner to
  "<file name> changed on disk." with a **Reload** button, and still emits
  `FileViewerEvent::Changed` once per change.
- **Reload.** The button (and `FileViewer::reload`) reopens the path (`FileView::open` +
  `LineIndex::build`) off the main thread, then swaps the new view and index in through
  `ViewerModel::refresh`, keeping the selection (clipped) and the scroll row, restores the host's
  banner, polls the new index as `new` does, and emits a new `FileViewerEvent::Reloaded`. If the
  reopen fails the banner says "Cannot read <file name>: <error>" and keeps the Reload button.

Tests in the new file `crates/fe-gpui/src/file_viewer/pane_item_tests.rs` (declared from
`file_viewer.rs`), with `TestAppContext` and generated files in tempdirs, as `file_viewer/tests.rs`
does.

## Implementation notes

- **Reload builds a new `ViewerModel`** rather than going through `ViewerModel::refresh`.
  `refresh` (and `DisplayMap::replace_text` under it) is for a later snapshot of the *same*
  source: it debug-asserts the row count never shrinks, and the display map keeps its
  `SourceId`, so `LongRows` would key a reloaded file with the same row count and length as
  the old one and reuse stale row indexes. A new model gets a new source id. fe-editor and
  fe-display are out of this card's scope, so the swap happens in `FileViewer::reopened`.
- **Selection and scroll are put back as the new index grows.** A reload (and a layout
  restore) starts a fresh `LineIndex::build`, whose first snapshot holds only the rows found
  so far, so clipping at swap time would pull the selection and scroll back towards row 0.
  The viewer keeps a pending `Restore` and re-applies it after each poll until the index is
  done; the user clicking, moving or scrolling drops it.
- **Staleness checks** start from `render` once the index is `Complete` (`cx.spawn_in` the
  window, so they stop with the window), with a `cx.on_focus` subscription for the focus check;
  they stop after the first change and restart after a reload. GPUI only fires focus listeners
  in an active window, so returning to the window also checks.
- **Restore** uses the editor's restore theme: `crate::pane_item::restore_style` became
  `pub(crate)` (its only change). The saved state also keeps the host's banner (`"banner"`),
  so a restored viewer shows the text the host opened it with; without it, "Read only: too
  large to edit".
- **Status bar**: `Read-only` (id `read_only`, right side, priority 50) and the cursor under
  `status::CURSOR`. Finding the head's row and its start can read many long rows, so the cursor
  text is worked out on the background executor when the selection or text snapshot changes,
  cached by (snapshot, anchor, head); `status_items` only reads the cache and shows the last
  text (or `…`) until the new one lands. Columns and ` (N selected)` count chars, as the
  editor's do, up to 1 MiB of text, and bytes past it.
- A failed reload counts as a change (emitting `Changed` if no check had yet), so later checks
  leave its "Cannot read" banner be. A view already focused when its watch starts is checked
  at once (the focus listener only starts after that frame). Each timed check goes through its
  window, so it stops when the window closes even while the view is still held.
- Extra tests: `focus_checks_the_file`, `the_cursor_is_worked_out_off_the_main_thread_once_per_move`,
  `a_failed_reload_keeps_its_banner`, `a_viewer_focused_on_its_first_frame_checks_at_once` and
  `closing_the_window_stops_the_checks`.
