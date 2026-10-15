---
task_uid: "01m50ew2fyhvdhek2gr2f5grvf"
task: "T114"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test pane::large_file_tests::large_files_open_read_only: with the threshold lowered, a file above it opened from the project panel, quick open or Pane::open shows a FileViewer tab with its first rows and the read-only banner before its index completes; a file below it opens as an editor as before"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test pane::large_file_tests::viewer_and_editor_tabs_together: in a pane with a viewer tab and editor tabs, tab cycling, closing, splitting the viewer, a settings reload, LSP sync, a vim host command and reopening the viewer's path all work, and none of them treats the viewer as an editor or panics"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test pane::large_file_tests::launch_file_and_restore: a Launch whose file is Opened::Large shows it as a viewer tab, and a saved layout with a viewer tab restores it"
    required: true
    manual: false
  - id: "C4"
    text: "loader::read and crates/fe-workbench/src/loader_encoding_tests.rs are unchanged"
    required: true
    manual: false
---

# Open files of 512 MiB and more in a read-only viewer tab, from the workbench and from fastedit <file> (Mac T398)

## Problem

docs/decisions/mmap-read-only.md decided that files of **512 MiB and more** open as a read-only
`pread`-backed view and smaller files keep the rope path (§Decision). fe-fileview, `ViewerModel`
(T273) and `FileViewer` (T274) exist, but neither the workbench nor `fastedit <file>` uses them:
every open goes through `loader::read` (`crates/fe-workbench/src/loader.rs:24`), which reads the
whole file into a rope. The decision's table: a 1 GB file costs ~1 s and 2.6 GB that way, a 4 GB
file 8.4 s and 8 GB, against well under a millisecond for the view's first screen.

The pane also assumes every tab is an editor: `Tab::of` (`crates/fe-workbench/src/pane.rs:55`)
`expect`s "fastedit's pane items are editors", and `Tab` holds an `Entity<EditorView>`.

## Change

- **Loader.** In `loader.rs` add `LARGE_FILE_BYTES` (512 MiB) and
  `pub enum Opened { Text(Option<Buffer>), Large { view: Arc<FileView>, index: Arc<LineIndex> } }`,
  and `pub fn open(path) -> io::Result<Opened>`: stat the file; at or over the threshold,
  `FileView::open` and `LineIndex::build` (which indexes on its own thread); otherwise `read` as
  today. `start`/`Reading`/`Read` carry `io::Result<Opened>`. Keep `read` as it is. A test-only
  setter lowers the threshold.
- **Pane.** `Tab` holds its item as either an editor or a viewer (`Entity<FileViewer>`), with
  `Tab::editor() -> Option<&Entity<EditorView>>`; `Tab::of` stops panicking. `Pane::open_at` and
  `Pane::file_read` / `Workbench::file_read` take `io::Result<Opened>`; `Large` pushes a
  `FileViewer` whose banner reads "Read-only: <size> is over 512 MiB." A `reveal` on a viewer
  selects its row. Everything that walks tabs as editors skips viewers: settings reloads, LSP sync
  (`Workbench::open_editors`), vim host commands (`vim_host.rs:68`), peer buffers. Opening a path a
  viewer tab already shows activates that tab.
- **Launch.** `Launch::file` carries `Option<Opened>`; `crates/fastedit/src/main.rs` reads the
  launch file with the loader's `Opened` and hands it over as it does a buffer today.
- **Layout.** Register `FileViewer` for layout restore in `fe_workbench::init`
  (`kiln_shell::layout::register_item::<FileViewer>`).
- Add `fe-fileview` to `crates/fe-workbench/Cargo.toml`.

"Edit this file" is the next card. Tests in the new file
`crates/fe-workbench/src/large_file_tests.rs`, declared from `pane.rs` with `#[path]`.

## Implementation notes

- `Tab` holds its item as a `Box<dyn ItemHandle>` with `Tab::editor()` and `Tab::viewer()`
  downcasts, rather than a two-variant enum: `Tab::of` then takes any pane item without a
  panic, and the path comes from `PaneItem::path`. The old public `editor` field is gone, so
  the tests and the screenshots example that read `tab.editor` now call `tab.editor()`, and the
  `Launch`s and `file_read`s in tests pass `Buffer::into()` (`impl From<Buffer> for Opened`).
  These are mechanical edits outside the listed scope.
- `Pane::active_item` returns a viewer too, so the status bar follows it (its `Read-only` item),
  `focus_target` focuses it, and `pane.move_*` moves its tab.
- A reveal on a viewer puts its caret at the reveal's start via `PaneItem::apply_state`
  (`head`), clamped to the rows the index has found when it's applied.
- `peer_model` only shares an editor's buffer; a file a peer pane shows in a viewer opens as
  its own viewer.
- `fastedit`'s `main` now knows the launch read was still running from `try_finish` returning
  `None`, instead of asking whether the window shows an editor, which a viewer launch doesn't.
- The test-only setter is `loader::test_support::lower_threshold(dir, bytes, held)`, scoped to
  a directory so tests running in one process don't see each other's threshold; `held` builds
  the indexes with `LineIndex::build_stepped`, whose steppers `take_steps` hands over.
