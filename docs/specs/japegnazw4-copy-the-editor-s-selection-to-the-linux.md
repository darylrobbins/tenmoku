---
task_uid: "01m4pzqawwz54tx1japegnazw4"
task: "T64"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test view::primary_selection_tests::selections_are_written: with set_primary_selection(true), dragging over hello writes hello to the primary selection and leaves the clipboard unchanged; a later shift-right selection writes its new text"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::primary_selection_tests::middle_click_pastes: with xyz in the primary selection, a middle click at row 2 column 3 inserts xyz there with the caret after it, and one undo removes it"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::primary_selection_tests::off_and_large: with it off, nothing is written and a middle click does nothing; with it on, a selection over 1 MiB is not written"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-gpui and cargo clippy -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Copy the editor's selection to the Linux primary selection and paste it with a middle click (Mac T455)

## Problem

On Linux, selecting text and middle-clicking to paste it (the X11/Wayland primary selection) is how people move text
between windows, and every Linux editor and terminal supports it. The editor does neither half: `EditorView` only
handles the left button (`crates/fe-gpui/src/view.rs` :1023-1026) and writes only the clipboard
(`ViewHost::write_clipboard`, `crates/fe-gpui/src/commands.rs` :193). GPUI already has the platform half:
`App::write_to_primary` and `App::read_from_primary` (gpui-pre 0.3.7 `src/app.rs` :1556-1563), implemented by the
Linux platforms and the test platform. Neon runs the Linux port.

## Change

In `crates/fe-gpui/src/view.rs`:

- `EditorView` gets `primary_selection: bool`, `cfg!(target_os = "linux")` by default, with
  `set_primary_selection(bool)` for hosts and tests.
- While it's on: after a mouse-up that ends a drag or a double- or triple-click, and after each dispatched command,
  if the primary selection (the model's primary, not the others) is non-empty and its text differs from what this
  view last wrote, write it with `cx.write_to_primary`. A selection over 1 MiB isn't written. The clipboard is
  never touched.
- A middle-button press in the text area (not the gutter) moves to a single caret at the pressed position (as a
  plain left click places it, with vim's mapping when vim owns the mouse) and inserts `cx.read_from_primary()`'s text
  there as one user edit, with the caret after it. Nothing happens if the primary selection is empty or unreadable.
  Vim stays in its mode.
- While it's off (the macOS default), neither happens.

## Tests

New file `crates/fe-gpui/src/view/primary_selection_tests.rs`, declared from view.rs beside its other test modules,
on `TestAppContext` (whose platform keeps a primary selection).
