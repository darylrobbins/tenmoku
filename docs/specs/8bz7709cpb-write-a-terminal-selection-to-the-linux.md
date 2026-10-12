---
task_uid: "01m4s3rjhhexf8xp8bz7709cpb"
task: "T92"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-terminal-view test primary_selection::selecting_writes_the_primary_selection (Linux): a drag, a double click and a shift-click that extends each write the selected text to the primary selection and leave the clipboard alone; after set_primary_selection(false) nothing is written; a drag reported to a program (mouse reporting on, no shift) writes nothing"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal-view test primary_selection::middle_click_pastes_the_primary_selection (Linux): a middle click with mouse reporting off writes the primary selection's text to the pty as the paste key does (line breaks as returns, bracketed after ESC[?2004h); with reporting on it is reported to the program as before and pastes nothing; with shift held it pastes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-terminal-view passes on macOS with tests/mouse.rs and tests/keyboard.rs unchanged, and the macOS build reads and writes no primary selection"
    required: true
    manual: false
---

# Write a terminal selection to the Linux primary selection and paste it with a middle click, as the editor does (Mac T567)

## Problem

On Linux every terminal writes what you select to the primary selection and pastes it with a middle click. The
editor does both since Mac T455 (`EditorView::set_primary_selection`, `write_primary` and `on_middle_down` in
`crates/fe-gpui/src/view.rs` :237-300, tests in `crates/fe-gpui/src/view/primary_selection_tests.rs`). The
terminal does neither. `docs/specs/a10j260te0-select-scroll-back-and-report-the-mouse.md` :60 left it as "middle-click
paste of the primary selection on Linux is a Neon follow-up".

In `crates/fe-terminal-view/src/mouse.rs`, `mouse_down` returns `false` for any button but the left one when the
program isn't reporting the mouse (:337), so a middle click does nothing. A selection made by a drag or a double or
triple click (`mouse_down`, `mouse_move`, `mouse_up`) is only copied by the copy key (`KeyInput::Copy` in
`crates/fe-terminal-view/src/input.rs` :200).

## Change

- `crates/fe-terminal-view/src/view.rs`: add `primary_selection: bool` to `TerminalView`, defaulting to
  `cfg!(any(target_os = "linux", target_os = "freebsd"))`, and `pub fn set_primary_selection(&mut self, on: bool)`,
  documented like the editor's. Add `last_primary: Option<String>`, the text this view last wrote.
- `crates/fe-terminal-view/src/mouse.rs`:
  - When a selecting gesture ends (the left button's `mouse_up` after a selecting `mouse_down`, which covers a drag
    and a double or triple click), write `terminal.selected_text()` to the primary selection, unless the setting is
    off, the text is empty, over 1 MiB (the editor's `PRIMARY_SELECTION_MAX`), or the same as `last_primary`. A
    shift-click that extends the selection writes it too. A gesture reported to the program writes nothing.
  - A middle press that isn't reported to the program (reporting off, or shift held) focuses the view and pastes the
    primary selection's text through the same path as the paste key (`paste_text` in `input.rs`, made
    `pub(crate)`), so it is bracketed when the program asked for bracketed paste and scrolls to the bottom as a paste
    does. It returns `true` so the event stops there. With reporting on and no shift, the middle press is reported
    to the program exactly as today.
  - GPUI has `write_to_primary`/`read_from_primary` on Linux and FreeBSD only: wrap them in two small cfg'd helpers
    as `write_primary_text`/`read_primary_text` in `crates/fe-gpui/src/view.rs` :1211-1227 do (a no-op and `None`
    elsewhere), so macOS builds and behaves as before.
- Copy (the copy key) still writes the clipboard only, and paste reads the clipboard only, as now.

No keys are added or changed: the middle button is not a key, so nothing collides with vim or nvim conventions, and
a program that asked for mouse reports (nvim with `mouse=a`, tmux) still hears the middle button as before.

## Tests

New file `crates/fe-terminal-view/tests/primary_selection.rs`, whole file under
`#![cfg(any(target_os = "linux", target_os = "freebsd"))]`, over a real pty as `tests/mouse.rs` and
`tests/keyboard.rs` do (copy their small helpers; don't edit those files).
