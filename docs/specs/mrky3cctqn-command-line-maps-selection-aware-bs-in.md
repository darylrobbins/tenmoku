---
task_uid: "01m6aknec3ry332cmrky3cctqn"
task: "T261"
rev: 8
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "A map's \u003cBS\u003e takes out the selection, or a grapheme before a caret at the end of the line, and cancels on an empty line as the input's own does, in the platform-settle path and in a \u003cCmd\u003e continuation too; documented limit: a caret moved off the end (F12, see the spec's documented limits)"
    required: true
    manual: false
  - id: "C2"
    text: "Held edits settle in order for the covered cases: keystrokes, action-dispatched edits (a paste, read when it comes) and platform text commits that reach the line while a \u003cCmd\u003e continuation waits go in after the continuation, in order, none lost and none reading its remap tags. All other cases are the documented limits (F1 F2 F7 F8 F9 F10 F11 F12 in the spec's documented limits, follow-up card): platform edits that carry a range, queued actions other than text once the line is closed, an earlier unresolved map prefix overtaken by held text or a paste, and held text lost when that prefix's resolution suspends or closes the line"
    required: true
    manual: false
  - id: "C3"
    text: "Platform text that arrives after a held prefix goes in after the keys the map gave, the editor-side tail included, live and across a \u003cCmd\u003e suspension; documented limits: unrecorded tail (F7), a still-pending recursive prefix (F8), a final host-command key (F9), a failed or cancelled suspended replay (F11)"
    required: true
    manual: false
  - id: "C4"
    text: "Existing tests pass unedited; clippy passes"
    required: true
    manual: false
---

# Command-line maps: selection-aware <BS> in settle and <Cmd> continuations, all line edits held while a continuation waits, platform text after a map's tail (T222 review F1 F2 F6)

Follow-up to T222's review round 2 (medium findings F1, F2, F6; T222's C1 and C3 narrowed to documented limits). Needs T222's code (`LineKeys::then`, `remap_line_more`, `live_lines`, the bar's keystroke hold).

- F1 (`crates/fe-workbench/src/vim_line.rs` `line_settle`, `crates/fe-gpui/src/vim.rs` `replay_line_key`): a map's `<BS>` is applied with the input's semantics only on the direct path (`type_line_keys` applies it to the input, then `remap_line_more` with the actual text). The platform-settle path simulates deletion on a string (graphemes, no selection), and a `<Cmd>` continuation's hidden line has no selection: select `ab`, hold a prefix whose map gives `<BS><BS>x`, then commit several characters (or run it after a `<Cmd>`): the first `<BS>` should take the selection and the second cancel. Carry the input's selection/caret into these paths, or apply each edit to the actual input before matching the next keys. Regressions for settle and for a `<Cmd>` continuation with a selection.
- F2 (`vim_line.rs` `hold_line_keys`): while a `<Cmd>` continuation waits for a command (`EditorView::line_continuing`), only keystrokes are held. Action-dispatched edits (a menu paste) and platform commits still edit the input; the continuation's text then replaces them, and their Changed sync can read the continuation's remap tags. Queue or safely merge every line edit during the wait. Tests that hold the completion (a write), perform each edit, resume.
- F6 (`vim_line.rs` `type_line_keys`): the platform payload (`after`) is typed before `rest`, though `rest` holds earlier map keys. With `j -> <Cmd>s/a/b/<CR>x/`, a longer `jk`, visible `s/a/` and a multi-character commit `yz` after a held `j`, the line becomes `s/a/yzx/` instead of `s/a/x/yz`; when the map closes the line, the payload lands before the map's editor-side tail. Run the whole tail first, across suspension too. Overlaps T258's F1 (terminating-key ordering with IME); coordinate or merge.

## Implementation notes

- Selection: the settle hook is told whether the input is selected (`TextInput::set_before_platform_input`'s hook takes it), and a continuation is told through `EditorView::select_line_text(bool)`, set on every hand-off; both simulate a `<BS>`/character on a selection as the input does.
- Held edits (F2): keystrokes, single platform characters, multi-character commits and the input's commands (a paste is read from the clipboard when it comes) queue while a `<Cmd>` waits, and are made one per deferred step, so work an edit defers runs before the next. A held edit whose line the continuation closed goes to the editor as keys. Limit: a platform deletion or composition start during the wait carries no range here (the hook gives none), so it is dropped; the text it named is the continuation's to replace.
- Platform text (F6): handed to the editor as a tail (`EditorView::line_platform_text`) and consumed when the replay ends, across suspension: into the line if it is still open, unmapped, else as plain keys. No marker characters are involved.
- T253 F4: replacement ranges move through each settled edit (`TextInput::through_edit`).

### Documented limits (review round 2 mediums, left for follow-up cards)

- Held platform edits keep only their text: a platform deletion, a range replacement (inserted as plain text, its range lost) or a composition start during a `<Cmd>` wait has no range through the hook and is dropped (F1).
- A held action the continuation's end leaves no line for (other than text and a paste) is dropped (F2); a held paste is captured as text and inserted without resolving an earlier ambiguous map prefix first (F10).
- The platform tail is appended to the hidden line without being recorded as literal input (F7); it is delivered once the replay's queue drains, so it can precede a still-pending recursive map prefix (F8) and is discarded when the last key submits a host command the replay then suspends for (F9).
- The tail field is cleared at the replay's natural end, not on every suspended-replay termination path, so a failed or cancelled suspended replay can leave it for a later one (F11).
- Only the selection is carried into settlement and a `<Cmd>` continuation, not a caret moved off the end of the line: with the caret in the middle or at the start, a map's `<BS>` or characters are predicted at the end while the input applies them at its caret (review F12). C1 holds for a selection and for a caret at the end.
