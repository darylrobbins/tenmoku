---
task_uid: "01m64wg5041jdn2ggd75g2t4ka"
task: "T1240"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::caret_keys_hosts::find_bar_takes_caret_keys (new file, declared from tests/mod.rs): with the find bar's query focused, typing abc, Left, then X gives abXc; Home then Y gives YabXc; End then Z appends; alt-Left (ctrl on non-mac) steps back a word; Delete removes the char after the caret. Real key presses, not direct method calls."
    required: true
    manual: false
  - id: "C2"
    text: "Same file: terminal_find_takes_caret_keys, search_panel_takes_caret_keys (query and the include/exclude filter inputs), shortcuts_editor_filter_takes_caret_keys, inline_edit_takes_caret_keys; each presses Left/Home/End/word-left/Delete and asserts the input text after typing at the caret."
    required: true
    manual: false
  - id: "C3"
    text: "Each host's explicit 4-arm match on PaletteOp is replaced by picker::op(run, cx).and_then(PaletteOp::edit) with cx.propagate() on None; existing Backspace, DeleteWordLeft, Clear and Paste behaviour is unchanged, and Enter, Escape and replace-field Tab behaviour in each host is unchanged."
    required: true
    manual: false
  - id: "C4"
    text: "Existing tests pass unedited: cargo test -p fe-workbench (find_bar, terminal_find, search_panel, shortcuts_editor, inline_edit modules) and cargo test -p fe-keymap. cargo clippy --all-targets and cargo fmt --check pass in the root workspace; kiln/ is its own workspace, so run clippy and fmt there too (no kiln edits expected)."
    required: true
    manual: false
---

# Find bar, terminal find, search panel, Keyboard Shortcuts filter and inline edit inputs take the TextInput caret keys (Left, Right, Home, End, word motions, Delete) through PickerOp::edit
