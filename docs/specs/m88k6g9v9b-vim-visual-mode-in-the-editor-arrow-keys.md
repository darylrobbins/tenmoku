---
task_uid: "01m4e9b73xf5bqx7m88k6g9v9b"
task: "T199"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Regression test tests::vim_visual_keys::arrows_extend_the_visual_selection (fails at base): with vim on over \"abcdef\\nxyz\", caret at byte 3, v then left left left selects 0..4 reversed with vim_mode() visual, a fourth left keeps 0..4, then right x4 selects 3..5 forward"
    required: true
    manual: false
  - id: "C2"
    text: "Regression test tests::vim_visual_keys::visual_modes_paint_a_block_on_the_vim_cursor (fails at base): after v and after V, block_cursor() is true; after v l on \"abcdef\" from byte 3 the painted block spans from the caret x at byte 4 to the caret x at byte 5 (the vim cursor, not the head at 5); after v $ it is one space wide at the row end; in insert mode and with vim off it is the 2px bar"
    required: true
    manual: false
  - id: "C3"
    text: "The default keymap gains exactly 4 bindings with context editor \u0026\u0026 vim.visual (left/right/up/down run vim.move with that motion); the pinned counts in command_tests.rs and keymap_tests.rs rise by exactly 4; besides those counts, command_tests.rs may change only by adding the editor \u0026\u0026 vim.visual context to table_is_the_only_binding_source's allowlist and updating its per-source count comment (12 for vim becomes 16), and keymap_tests.rs changes only in its count; vim::tests and vim::insert_tests pass unchanged; nothing under crates/fe-vim changes"
    required: true
    manual: false
---

# Vim visual mode in the editor: arrow keys extend the selection, and the cursor is a block on the vim cursor

# Vim visual mode in the editor: arrow keys extend the selection, and the cursor is a block on the vim cursor

## Symptom

The user, 2026-10-08, in the running app built from main: "visual mode doesn't work with the arrow
keys and it changes to the insertion cursor instead of the block. It also didn't let me select
backwards on a line to the first character on the line".

Reproduced in a headless window (`EditorView`, vim on, "abcdef\nxyz", caret at byte 3):

- `v` selects 3..4. `left left left` then leaves a bare caret at 1..1 while `vim_mode()` is still
  `visual`: the selection is gone and the vim cursor never moved. `right right` gives 5..5.
- `view.block_cursor()` is false after `v` (and `V`): the caret paints as the insert-mode bar.
- `v h h h`, `v 0` and `v b` do reach column 0 (0..4, reversed), in fe-vim's `HeadlessVim` and in
  the view alike. The "can't select backwards to the first character" report is the arrow-key bug
  seen going left: arrows move a bare caret instead of the vim cursor, and the next `h` jumps back
  to the old visual extent, because `VimState`'s cursor never moved.

## Cause

- **Arrows**: `crates/fe-keymap/keymaps/default-macos.toml` binds `left right up down` to
  `vim.move` only for `editor && vim.mode == normal` (the vim layer, about line 511). In a visual
  mode the plain `editor` bindings at the top of the file match instead, the keymap runs before the
  reducer, and `editor.move_left` collapses the selection to a caret (`fe-editor` `lib.rs` ~623,
  `Movement::Left if collapsed`). The reducer already reads the arrows as `h l k j` in a visual
  mode (`fe-vim` `reducer.rs` ~258), but the keystroke never gets there.
- **Bar cursor**: `EditorView::block_cursor` (`crates/fe-gpui/src/vim.rs` ~331) is
  `matches!(self.vim_mode(), Some("normal" | "operator_pending"))`, so `visual` and
  `visual_line` paint the bar. And `element.rs` (~519) paints the block at each model selection's
  head, which for a forward charwise selection is one grapheme past the vim cursor (`v` at byte 3
  selects 3..4, head 4), so flipping `block_cursor` alone would put the block on the wrong
  grapheme.

T173 built visual mode in fe-vim only; T175 (ready, waiting on T174) is the full GPUI wiring. This
card pulls the two pieces the user hit forward, with no fe-vim change, so they don't wait on T174.

## Fix

- In the vim layer of `default-macos.toml`, after T161's normal-mode arrows, four bindings with
  context `editor && vim.visual`: `left right up down` run `vim.move` with `motion` `left`, `right`,
  `up`, `down`, as the normal-mode ones do (`vim.move` in a visual mode already moves the vim
  cursor and keeps the anchor: `fe-vim` `motion::apply` → `visual::move_cursor`). Raise the pinned
  binding counts in `command_tests.rs` and `keymap_tests.rs` by exactly 4; those files change
  only in those counts.
- `block_cursor()` is true in every mode but insert while vim is on (`normal`, `operator_pending`,
  `visual`, `visual_line`, and `visual_block` once T174 adds it). Update its doc comment.
- In `element.rs`, in a visual mode the block paints at the vim cursor
  (`VimState::visual_cursor(&model)`), not at the model selection's head; in normal and
  operator-pending mode it stays at the heads. At a row's end (after `$` or `l` past the last
  grapheme) it is one space wide, as `block_carets` already does for a line end.

## Tests

New file `crates/fe-gpui/src/tests/vim_visual_keys.rs`, declared from `crates/fe-gpui/src/tests.rs`
with a plain `mod vim_visual_keys;` (not `#[path]`, and not from `vim.rs`, so Regatta's base overlay
compiles it at base). `#[gpui::test]`s in a headless window, as `vim/tests.rs` builds its editor
(copy the few helpers you need; don't make `vim::tests`' helpers public). Use only base API, so the
tests compile and fail at base:

- `arrows_extend_the_visual_selection`
- `visual_modes_paint_a_block_on_the_vim_cursor`

## What not to do

- Don't change anything under `crates/fe-vim` (T174 holds visual.rs, reducer.rs, state.rs and the
  rest; T188 and T182 queue behind it). In particular don't add a `sync_visual` call, or a fix in
  `visual::move_cursor`: backward selection to column 0 is already right there.
- Don't bind the arrows to `vim.key` or to `editor.select_*`, and don't touch `step()`: `vim.move`
  through the keymap is how T161 does it in normal mode and how T175 specifies it.
- Don't add T175's other six visual bindings (`enter tab backspace shift-backspace delete
  alt-backspace`), the mouse drags, the empty-row selection mark, the status-bar labels or `cmd-c`:
  they stay T175's. Leave `cmd-left`, `alt-left`, `pageup` and the shift-arrows as they are (they
  still run editor commands in a visual mode); note that in the hand-in for T175.
- Don't paint the block from the selection's head in a visual mode, and don't change the bar with
  vim off or in insert mode.

## Implementation notes

- `command_tests.rs` also gained one allowlist line (`"editor && vim.visual"`) and its "12 for vim" comment became 16: `table_is_the_only_binding_source` filters bindings by context, and the new context fails it otherwise. Beyond the count, this is the only change to that file.
- T175: `cmd-left`, `alt-left`, `pageup` and the shift-arrows still run editor commands in a visual mode, untouched here.
- `tests::injected_languages_compiled_meanwhile_draw_again` fails in a full parallel `cargo test -p fe-gpui --lib` run (a global "lua" language loaded by another test) and passes alone; unrelated to this change.
