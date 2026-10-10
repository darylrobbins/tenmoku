---
task_uid: "01m4fkss8p5zcz51kg60t3zqrc"
task: "T274"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test file_viewer::tests::paints_the_first_rows_and_the_end: a FileViewer over a generated 8 MiB file lays out rows whose text equals the file's first rows, under the banner, and after scrolling to the end, its last rows"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test file_viewer::tests::selects_and_copies: click then shift-click selects that range, cmd-c puts its text on the test platform's clipboard, and typing a char or cmd-v emits FileViewerEvent::ReadOnly with the text unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test file_viewer::tests::rows_grow_while_indexing: under build_stepped, the view's row count grows after each step and poll, and polling stops once the index is Complete"
    required: true
    manual: false
  - id: "C4"
    text: "view.rs and element.rs are unchanged, and lib.rs changes only by a mod line and a pub use line"
    required: true
    manual: false
---

# Add FileViewer to fe-gpui: paint a ViewerModel under a banner strip, with mouse and keyboard selection, copy, and rows that grow while the line index builds

## Problem

<T-viewer-model> adds `ViewerModel`, a read-only editor over a `TextSource`, but nothing paints it. `EditorView`
and `EditorElement` (`crates/fe-gpui/src/view.rs`, `element.rs`) are built on `EditorModel`, and about ten ready
cards change those two files (T145–T147, T220, T230, T231, T255, T258, T264). A separate view in new files keeps
this out of their way.

## Change

Add `FileViewer`, a GPUI view, in the new files `crates/fe-gpui/src/file_viewer.rs` and
`crates/fe-gpui/src/file_viewer/element.rs`, exported from `crates/fe-gpui/src/lib.rs` (a `mod` line and a
`pub use` line only). Add `fe-fileview` to `crates/fe-gpui/Cargo.toml`. Don't change `view.rs` or `element.rs`. Use
their existing `pub(crate)` items (the text measurer, glyph widths and theme colours) as they are.

- `FileViewer::new(view: Arc<FileView>, index: Arc<LineIndex>, banner: SharedString, theme: EditorTheme, cx)`
  builds a `ViewerModel` over a `FileText` snapshot.
- Paint it with `fe_display::layout::layout_frame`: the gutter with line numbers, the rows, the selection and the
  caret, with no syntax highlighting, no folds and no soft wrap.
- `banner` is a one-line strip above the text, in the theme's muted colours, and it doesn't scroll. The workbench
  card supplies its wording.
- Input: the scroll wheel; click, drag and shift-click selection through `ViewerModel::set_selection`; and the
  editor's actions, run through `ViewerModel::run`. An action that returns `Outcome::ReadOnly` emits
  `FileViewerEvent::ReadOnly` and changes nothing. `editor.copy` writes `copy_text()` to the clipboard, and
  `CopyError::TooLarge` emits `FileViewerEvent::CopyTooLarge`. It has its own `FocusHandle` (`Focusable`) and the
  editor's key context, so the default keymap's movement, select-all and copy keys reach it.
- While `LineIndex::progress().state` is `Building`, poll every 100 ms on the foreground executor. Each poll takes
  a new `FileText` snapshot into `ViewerModel::refresh` and notifies. Stop polling once the state is anything
  else. `FileText::error()` or a non-`Complete` final state emits `FileViewerEvent::Changed`. The workbench only
  logs it for now (the reload banner isn't in this chain).

Hosting it in a workbench tab is the next card.

## Tests

In the new file `crates/fe-gpui/src/file_viewer/tests.rs` (declared from `file_viewer.rs`), with GPUI's
`TestAppContext` and generated files in `tempfile` directories. Use <T-index>'s `LineIndex::build_stepped` for the
building case.
