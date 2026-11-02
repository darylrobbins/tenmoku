---
task_uid: "01m6c61wzy86n47enn3hzj9z6p"
task: "T393"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim integration tests expand_from_normal_enters_visual_and_grows, shrink_steps_back_to_normal_mode, a_motion_forgets_the_history and then_d_deletes_the_node pass (crates/fe-vim/tests/expand_selection.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-keymap integration test vim_expand_and_shrink_keys_in_normal_and_visual passes on Os::MacOs and Os::Linux"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim -p fe-keymap -p fe-editor passes; the only existing test edited is crates/fe-keymap/tests/expand_selection_keys.rs, whose vim-modes-resolve-to-None half contradicts the new vim bindings (it now expects the vim commands there); crates/fe-vim/src/state.rs is unchanged"
    required: true
    manual: false
---

# Expand and shrink selection in vim normal and visual mode: the syntax-node keys grow vim's visual selection and step back to normal mode (Mac T1274)

## Problem

Structural selection landed as Mac T855 (fe08250c7): `editor.expand_selection` / `editor.shrink_selection`
(`crates/fe-editor/src/smart_select.rs`, `EditorModel::expand_selection`, `shrink_selection`, `ExpandHistory`),
bound to ctrl-shift-cmd-right/left (macOS) and shift-alt-right/left in `crates/fe-keymap/keymaps/fastedit.toml`
lines 390-398, context `editor && !vim.enabled`. With vim on, the keys do nothing and the palette command
leaves vim's visual state (`VimState::visual`, `crates/fe-vim/src/visual.rs`) out of step with the model's
selection. The user (2026-11-01) wants it "working with vim visual mode if cheap"; it is: the model already
mirrors vim's visual extent (`visual.rs` `select`, "the model's selection mirroring it").

## Change

1. New `crates/fe-vim/src/smart_select.rs` (declared in `crates/fe-vim/src/lib.rs`), two commands registered in
   `crates/fe-vim/src/commands.rs`'s `registry()`: `vim.expand_selection` and `vim.shrink_selection`.
   - In normal mode or a visual mode, expand calls `EditorModel::expand_selection(host)` on the model (whose
     selection is the caret, or the visual extent's mirror), then enters charwise visual with the model's new
     primary selection: anchor at its start, cursor on its last grapheme, through `visual.rs`'s `select`
     (made `pub(crate)` if it is not). Nothing grew: nothing changes, `Effect::Nothing`.
   - Shrink calls `EditorModel::shrink_selection()` and adopts the result the same way; a restored empty
     selection (the caret the first expand started from) returns to normal mode with the caret there.
   - No new `VimState` field: the history is the model's `ExpandHistory`. The selection `select` mirrors must
     equal the one the model set, so the history survives the round trip; any other key that moves the visual
     extent drops it, as it does for the plain editor.
   - Visual mode holds one extent (several carets collapse into it), so this works on the primary selection.
   - Blockwise or linewise visual becomes charwise on expand.
2. `crates/fe-keymap/keymaps/fastedit.toml`: the same keys bound to `vim.expand_selection` /
   `vim.shrink_selection` in contexts `editor && vim.mode == normal` and `editor && vim.visual`. The plain
   editor's bindings stay as they are.
3. Tests, both new files:
   - `crates/fe-vim/tests/expand_selection.rs`, with a host whose `larger_syntax_node` grows through a fixed
     list of nested ranges (as `crates/fe-editor/tests/expand_selection.rs` does) and the harness of
     `crates/fe-vim/tests/definition_objects.rs`: `expand_from_normal_enters_visual_and_grows`,
     `shrink_steps_back_to_normal_mode` (three expands then three shrinks give the exact earlier visual extents,
     the last one normal mode at the original caret; a fourth shrink does nothing), `a_motion_forgets_the_history`
     (expand, `l`, shrink is `Effect::Nothing`), and `then_d_deletes_the_node` (expand twice then `d` deletes
     exactly the second range).
   - `crates/fe-keymap/tests/vim_expand_selection_keys.rs`, `vim_expand_and_shrink_keys_in_normal_and_visual`,
     resolving keys as `crates/fe-keymap/tests/expand_selection_keys.rs` does, on `Os::MacOs` and `Os::Linux`.
