---
task_uid: "01m4sfx3v9tygk2xjccteaff1b"
task: "T72"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-shell test picker_vim::normal_mode_moves_and_accepts passes: with the switch on, after typing 'ap', picker.dismiss leaves the picker open (no DismissEvent) in normal mode with the query kept; j/k move the selection clamped at the ends; g g selects the first match and G the last; picker.confirm accepts the selected match; in a fresh show, escape then q, and escape twice, each dismiss"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-shell test picker_vim::normal_mode_types_nothing passes: in normal mode, keystrokes x and j and text_input.backspace leave the query and matches unchanged; i, a, I and A each return to insert mode, where typing appends to the kept query; show() reopens in insert mode; the picker's key context has vim_mode=normal in normal mode, vim_mode=insert in insert mode, and neither with the switch off"
    required: true
    manual: false
  - id: "C3"
    text: "kiln-shell test picker_vim::switch_off_is_unchanged passes: with no switch installed or it returning false, escape dismisses at once and j types into the query; turning the switch off while in normal mode makes the next typed key go into the query"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test picker_vim (crates/fe-workbench/tests/picker_vim.rs) passes: after fe_workbench::init, kiln_shell::picker::vim_keys follows fe_gpui::set_vim_enabled on and off; recent_picker_tests::vim passes with escape pressed twice to close; every existing kiln-shell and fe-workbench test passes"
    required: true
    manual: false
  - id: "C5"
    text: "No keymap file changes; nothing outside the scope changes except this lane's docs/specs file; cargo clippy --workspace --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Vim keys in kiln-shell's pickers: with vim on, Escape enters a normal mode where j/k move, gg/G jump, Enter accepts and Esc/q close, as in telescope (Mac T596)

Filed on helium from Mac T596 (~/coordination/helium-cards/T596.json; Mac lead message ~/coordination/from-mac-lead-2026-10-13-helium-zi.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

Standing user rule: every feature is vi friendly. kiln-shell's picker
(`kiln/crates/kiln-shell/src/picker.rs`, `Picker<D>`) is the list under the command palette
(`kiln_shell::command_palette`), quick open, go to line, Open Recent, and fastedit's language, toolbar
and navigation pickers (`crates/fe-workbench/src/*picker*.rs`, `quick_open.rs`, `go_to_line.rs`).
Its keys are `picker.select_prev/next` (up, down, ctrl-p, ctrl-n), `picker.confirm` (enter) and
`picker.dismiss` (escape) in the `picker` context (`crates/fe-keymap/keymaps/*.toml`). With vim on, a vim user
can't move through the list with `j`/`k`: every unbound key types into the query, and one Escape closes the
picker. `crates/fe-workbench/src/recent_picker_tests.rs::vim` pins exactly that today.

nvim's pickers (telescope, and snacks.picker that LazyVim ships) all use one model: the prompt opens in
insert mode; Escape goes to normal mode, where `j`/`k` move, `gg`/`G` jump to the ends, Enter accepts, and
Escape (or `q`) closes; `i`/`a` go back to typing.

## Change

Telescope's model, in kiln-shell's `Picker`, so every picker gets it. No keymap file changes: the picker
reads the keys itself, as fastedit's editor reads vim keys the keymap leaves unbound
(`crates/fe-gpui/src/vim.rs` module docs).

1. **The switch.** kiln-shell knows nothing of fastedit's vim. Add
   `kiln_shell::picker::set_vim_keys(cx, fn(&App) -> bool)`: a global query the host installs, read live
   on each keystroke and render (unset = off). fe-workbench's `picker::init`
   (`crates/fe-workbench/src/picker.rs`) installs `fe_gpui::vim_enabled`, so Toggle Vim Mode and a settings
   reload apply to open pickers at once. Add `kiln_shell::picker::vim_keys(cx) -> bool` to read it.
2. **Modes.** `Picker` gets a mode, insert or normal. `show()` always opens in insert mode.
   - **Insert** (as today): typing filters; up/down/ctrl-p/ctrl-n move; enter accepts. With the switch on,
     `PickerOp::Dismiss` (escape) and an unbound `ctrl-[` enter normal mode instead of dismissing. With
     the switch off, nothing changes.
   - **Normal**: the query is kept and no longer takes text: `j` and `k` move the selection (as
     select_next/prev, clamped), `g g` selects the first match and `G` the last (a `g` followed by anything
     else is dropped), enter (`picker.confirm`) accepts, escape (`picker.dismiss`), `ctrl-[` and `q` dismiss,
     and `i`, `a`, `I`, `A` return to insert mode, typing appending to the kept query (TextInput has no caret
     movement, so all four append). The bound picker keys (arrows, ctrl-n/p) keep working. The input's edit
     commands (`PickerOp::Backspace`, `DeleteWordLeft`, `Clear`, `Paste`) do nothing in normal mode. No
     counts (telescope has none).
   - The normal-mode letters go through an `on_key_down` listener on the picker's element, reached only by
     keystrokes no binding matched; they act only in normal mode with the switch on.
   - Turning the switch off while in normal mode returns the picker to insert mode on its next keystroke
     or render, so typing works again.
3. **Focus stays on the input** in both modes (the modal layer dismisses a modal whose focus leaves it,
   `kiln/crates/kiln-shell/src/modal.rs` `show_modal`). Instead, kiln-ui's `TextInput`
   (`kiln/crates/kiln-ui/src/text_input.rs`) gets `set_typing(bool, cx)`: while false it registers no input
   handler (its canvas skips `window.handle_input`), so typed keys insert nothing, and it draws no bar
   caret, the mode's visible cue. The picker sets it on each mode change.
4. **Key context.** While the switch is on, the picker's element sets `vim_mode=insert` or
   `vim_mode=normal` beside `picker`, which fastedit's keymap reads as `vim.mode` (the `vim` context
   namespace `crates/fe-gpui/src/keymap.rs` registers), so a user keymap can bind more keys under
   `picker && vim.mode == normal`. With the switch off it sets neither.
5. **Existing test.** `recent_picker_tests.rs::vim` asserts one escape closes the picker with vim on:
   change it to escape twice (the first leaves the picker open in normal mode, query `j` kept), and keep its
   other assertions.

Update `picker.rs`'s module docs to describe the modes. kiln stays free of fastedit types.

## Tests

New `kiln/crates/kiln-shell/tests/picker_vim.rs`, modelled on `tests/picker.rs` (`open`, `run`, `Words`):
install the switch with a test flag, drive commands with `run(...)` and unbound letters with
`cx.simulate_keystrokes`. New `crates/fe-workbench/tests/picker_vim.rs` for the wiring.

## Implementation notes

- `kiln_shell::picker` gains `set_vim_keys`, `vim_keys`, `Mode` (`Picker::mode()`) and `VIM_MODE`
  (the key context key, `vim_mode`). fe-workbench's `picker::init` installs `fe_gpui::vim_enabled`.
- The key listener also treats an unbound `escape` as `picker.dismiss` does, so a host with no keymap
  (kiln-shell's tests) gets the same model. With the switch on, a plain keystroke in normal mode that
  isn't one of the picker's letters is swallowed (dropped), as `g` then another key is.
- Switched off in normal mode, the next keystroke returns to insert mode and inserts its `key_char`
  itself: the frame it arrived in registered no input handler. A frame drawn first does the same
  return in `render`.
- `TextInput::set_typing(false)` also drops an open IME composition; `TextInput::typing()` reads it.
