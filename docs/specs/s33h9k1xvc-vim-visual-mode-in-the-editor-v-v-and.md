---
task_uid: "01m4bq06jr887ekws33h9k1xvc"
task: "T175"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::visual_tests::visual_keys_in_the_editor: with vim on, on a focused editor over \"one two\\nthree\\nfour\", v e y yanks \"one\" into the unnamed register and leaves vim_mode() normal; V j d deletes the first two rows in one undo step; ctrl-v j l d deletes a 2x2 block; v then right right extends the selection by two graphemes; escape leaves visual mode with the selection collapsed; simulate_input(\"QK\") in each visual mode changes no text (keys vim ignores in visual mode; \"abc\" would run the ab text object and c, as in nvim); v then c registers the input handler (simulate_input then types); cmd-c in visual mode puts the selection's text on the test clipboard and vim_mode() is still visual"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::visual_tests::cursor_paints_on_the_last_grapheme: on \"a漢b\" with vim on, after v l the selection paints over bytes 0..4 and the block caret spans from the caret x at byte 1 to the caret x at byte 4 (on 漢, not on b); after V the block caret is at the vim cursor's column; in visual_block mode it is on the cursor corner; an empty row inside a V selection paints a selection one space wide"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test vim::visual_tests::mouse_drags_and_clicks: with vim on, a mouse drag from byte 0 to byte 3 in normal mode enters vim_mode() visual with the vim cursor on byte 3's grapheme; a triple click enters visual_line; a click in a visual mode leaves it with the caret at the click; with vim off a drag makes a plain selection and vim_mode() is None"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test vim::visual_tests::visual_layer_of_the_default_keymap: the default keymap has exactly 10 bindings with context editor \u0026\u0026 vim.visual: T199's left, right, up and down run vim.move with motion left, right, up and down, and this card's enter, tab, backspace, shift-backspace, delete and alt-backspace run vim.key; each of those 10 resolves against fe_gpui::commands::command_catalog() with no diagnostic, and fe-workbench test tests::outline_panel::the_default_keymap_binds_outline_show (zero diagnostics against fe_workbench::command_catalog()) passes unchanged; the counts pinned in command_tests.rs and keymap_tests.rs are their base values plus 6 (T199 already added 4) and those files change only in those counts and command_tests.rs's per-source count comment; fe-gpui test vim::tests::vim_layer_of_the_default_keymap passes unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test status_bar::visual_tests::shows_the_visual_modes: with vim on, after v, V and ctrl-v and a redraw the vim_mode item reads VISUAL, VISUAL LINE and VISUAL BLOCK; after escape it reads NORMAL"
    required: true
    manual: false
  - id: "C6"
    text: "Scope: nothing under crates/fe-vim changes; T161's vim::tests and T162's vim::insert_tests pass unchanged"
    required: true
    manual: false
---

# Vim visual mode in the editor: v, V and ctrl-v keys, the selection painted with the cursor on its last grapheme, mouse drags, and VISUAL in the status bar

# Vim visual mode in the editor: v, V and ctrl-v keys, the selection painted with the cursor on its last grapheme, mouse drags, and VISUAL in the status bar

Milestone M7 "Vim", second batch, card 3 of 11: the GPUI wiring of cards 1 and 2 (visual modes in
`fe-vim`). Read T161's and T162's specs (`docs/specs/`) for how the editor routes vim keys today,
and `docs/decisions/vim-engine.md` §9 item 7 and §10 ("Unmatched, in normal, visual or
operator-pending mode: the keystroke goes to `fe-vim`'s `step()`"; "No text in normal or visual
mode"; "Ctrl keys in normal and visual mode: vim wins").

**Clean room** as every M7 card: no Vim/Neovim source, no hjkl, no Zed `vim`/`editor` crate, no Helix.

## Change

### Keys (`crates/fe-gpui/src/vim.rs`, `crates/fe-keymap/keymaps/default-macos.toml`)

- In the visual modes the reducer gets every key the keymap doesn't match, as in normal mode
  (`vim_editor` already treats any non-insert mode as vim's; check it end to end, fix what's
  missing). No input handler is registered in a visual mode; `c`, `s`, `I`, `A` switch to insert
  and register it.
- The vim layer ends with 10 bindings with context `editor && vim.visual`, mirroring T161's 10
  normal ones. T199 (landed) already added the first 4: `left right up down` run `vim.move`, and
  added the `editor && vim.visual` context to `table_is_the_only_binding_source`'s allowlist. This
  card adds the other 6: `enter`, `tab`, `backspace`, `shift-backspace`, `delete`, `alt-backspace`
  run `vim.key`. T161's binding that pins exactly 10 `editor && vim.mode == normal` bindings stays
  true. The pinned counts in `command_tests.rs` and `keymap_tests.rs` rise by exactly 6 (and the
  "16 for vim" comment there becomes 22).
- `ctrl-v` and `ctrl-q` reach the reducer in normal and visual mode (no default `ctrl-` binding
  matches there, per T161); check it with a test.
- `cmd-c` in a visual mode runs `editor.copy` on the model's selection (cards 1–2 keep it equal to
  the visual extent) and stays in visual mode.

### Painting (`crates/fe-gpui/src/element.rs`, `crates/fe-gpui/src/view.rs`)

- The selection paints as today from the model's `SelectionSet` (it already includes the grapheme
  under the vim cursor).
- The block cursor paints on the **vim cursor**'s grapheme (`VimState`'s cursor), not on the model
  selection's head, which for a forward charwise selection is one grapheme further on. T199
  (landed) already does this in every visual mode: `element.rs` paints the block from
  `VimState::visual_cursor` and the view's `block_cursor()` is true in visual modes. Build on that
  rather than adding a second path: check, and fix where needed, that in linewise mode it sits at
  the vim cursor's column on its row and in block mode on the cursor corner, and that it lands on
  a wide grapheme's whole cell (C2).
- An empty row inside a linewise or block selection paints a one-space-wide selection mark, as
  nvim shows it.

### Mouse (`crates/fe-gpui/src/view.rs`)

- With vim on, a mouse drag in normal mode enters charwise visual mode over the dragged extent
  (nvim's default `mouse=nvi`); a double-click-drag selects by words in charwise mode, a triple-click
  enters linewise mode. A plain click in a visual mode leaves it and puts the cursor there. Through
  card 1's `VimState::select_visual` and `vim.visual_exit`; no fe-vim change here (block if the API
  lacks something).

### Status bar (`crates/fe-workbench/src/status_bar.rs`)

`vim_mode_label` shows `VISUAL`, `VISUAL LINE` and `VISUAL BLOCK`.

## Tests (new files)

- `crates/fe-gpui/src/vim/visual_tests.rs`, declared from `vim.rs` next to `tests` and T162's
  `insert_tests` (`#[cfg(test)] mod visual_tests;`), `#[gpui::test]`s in a headless window as
  T161's `vim/tests.rs`.
- `crates/fe-workbench/src/status_bar_visual_tests.rs`, declared from `status_bar.rs` with
  `#[path = "status_bar_visual_tests.rs"] mod visual_tests;` as T161's `vim_tests`.

## Out of scope

- `:` from visual mode (card 6), search in visual mode (card 4 adds it in fe-vim; card 6 wires the
  line), visual-mode `ctrl-a` (card 9), select mode, alt-drag block selection.
- Any fe-vim change.

## Decided (lead defaults; the user may revisit)

- **Mouse drags enter visual mode** with vim on, as nvim's default `mouse=nvi` does. The other
  choice is to leave drags as a plain editor selection, which normal-mode keys then collapse.
- **`cmd-c` copies and stays in visual mode** (macOS habit); `y` is vim's way and leaves it.
