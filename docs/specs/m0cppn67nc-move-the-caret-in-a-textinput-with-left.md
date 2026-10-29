---
task_uid: "01m62t2bcgn53n32m0cppn67nc"
task: "T1230"
rev: 6
type: "bugfix"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::settings_caret::left_arrow_moves_the_caret_in_a_setting_input (in the new file src/tests/settings_caret.rs, declared from src/tests/mod.rs and using only test APIs that exist at base, so the regression overlay runs it at base): with the Settings tab open and the editor.tab_size input focused holding '4', pressing left then typing '1' leaves the input text '14' (at the base it is '41': the input has no caret)"
    required: true
    manual: false
  - id: "C2"
    text: "tests::settings_caret::left_arrow_moves_the_caret_in_the_search_field (same file and rules as C1): the same in the search field: type 'abc', left, left, type 'X' gives 'aXbc'; the list still filters on the new text"
    required: true
    manual: false
  - id: "C3"
    text: "settings_editor::caret_tests::home_end_and_word_motions: Home, End, alt-left and alt-right (ctrl on non-mac) move the caret; backspace and delete (forward) delete at the caret, not at the end"
    required: true
    manual: false
  - id: "C4"
    text: "settings_editor::caret_tests::arrows_still_move_the_row_selection: Up and Down in an input do not leave it by moving the row selection; Up/Down keep working in the list and in the search as before (existing settings_editor_tests unchanged)"
    required: true
    manual: false
  - id: "C5"
    text: "kiln-ui test text_input::caret_tests:: (declared from text_input.rs via #[path = \"tests/text_input_caret.rs\"], not from tests/mod.rs, so the regression base never compiles it) caret_is_drawn_at_the_insertion_point: set_text 'ab', move_left once, the caret quad sits between 'a' and 'b'; typing inserts there; a selection from select_all is replaced by the next Left (caret goes to the start)"
    required: true
    manual: false
  - id: "C6"
    text: "kiln-ui test text_input::caret_tests:: (declared from text_input.rs via #[path = \"tests/text_input_caret.rs\"], not from tests/mod.rs, so the regression base never compiles it) ime_composition_at_the_caret: a marked composition is inserted at the caret, not at the end; existing text_input and text_input_ime tests pass unchanged"
    required: true
    manual: false
  - id: "C7"
    text: "cargo test -p kiln-ui -p kiln-shell -p fe-workbench settings_editor and cargo test -p fe-keymap pass; keymap binds left/right/home/end/alt-left/alt-right/delete in context text_input to new text_input.move_left etc. commands, registered in kiln-shell picker.rs registry() next to text_input.backspace"
    required: true
    manual: false
---

# Move the caret in a TextInput with Left, Right, Home, End and word motions, so the Settings tab's inputs and search field take arrow keys

## Problem
In the Settings tab, number and text boxes (and the search field) ignore Left and Right. Root cause: kiln-ui `TextInput` (kiln/crates/kiln-ui/src/text_input.rs) has no caret by design: its doc says "Typed text goes at the end: there's no caret movement", and its only selection is all of the text. The editing keys are commands (`text_input.backspace`, `delete_word_left`, `clear`, `paste`) registered in kiln-shell picker.rs `registry()` as `PickerOp`s, bound in crates/fe-keymap/keymaps/fastedit.toml (context `text_input`), and applied by each host (settings_editor.rs `on_input_command`). Nothing handles left/right, so the key goes nowhere. kiln-ui also has a full `TextField` (text_field.rs, caret + selection + undo via kiln_core::TextEdit/Motion) but it is not wired to kiln's keymap, so switching the settings page to it is the larger, worse option.
## Change
1. TextInput: keep a caret byte offset (grapheme-aligned) in place of "always at end"; add `move_left`, `move_right`, `move_word_left`, `move_word_right`, `move_home`, `move_end`, `delete_forward`; backspace/delete_word_left/insert/paste/IME composition act at the caret; draw the caret at the insertion point. Reuse kiln_core::text_edit::Motion for grapheme and word rules. Selecting all then Left/Right collapses to the start/end.
2. kiln-shell picker.rs: add PickerOp variants and registry entries `text_input.move_left|move_right|move_word_left|move_word_right|move_home|move_end|delete_forward`; `PickerOp::edit` maps them to the new methods.
3. fastedit.toml: bind left, right, home, end, alt-left/alt-right (mac) and ctrl-left/ctrl-right (default), delete in context `settings_editor && text_input` only (lead call, rev 2): a plain `text_input` context shadowed the project panel's TextFields and gave the find bar, vim_line, search_panel and inline_edit inputs moves they don't handle (Left went dead there). Other hosts keep their current keys until the op.edit() follow-up. Add the new context string to the exact-context lists the binding tests pin (crates/fe-gpui/src/command_tests.rs, keymap_tests.rs, vim/tests.rs: one line each, as T76 did) and keep those tests passing.
4. settings_editor.rs `on_input_command`: it matches PickerOp explicitly and `propagate`s everything else; use `op.edit()` so the search field and row inputs get every input command.
Out of scope: other hosts (find bar, picker, git panel) keep their explicit matches; a follow-up can switch them to `op.edit()` (they get the keys for free only where they already call it). Keep tests small.
