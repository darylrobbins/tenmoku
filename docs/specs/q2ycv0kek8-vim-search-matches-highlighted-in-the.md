---
task_uid: "01m4ewtyzaqe08kqq2ycv0kek8"
task: "T220"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::search_tests::matches_are_highlighted: with vim on, after a search for ab dispatched as vim.command_line_submit on \"ab ab\\nxab\", paint fills three search-match rectangles at bytes 0..2, 3..5 and 7..9 under the selection; :noh dispatched as vim.command_line_submit removes them and n shows them again; deleting a match removes its rectangle on the next frame; a buffer with 10,000 matches in view paints at most the per-frame cap; with vim off nothing is painted"
    required: true
    manual: false
  - id: "C2"
    text: "Scope: nothing under crates/fe-vim changes; fe-gpui's lib.rs and EditorTheme don't change; T161's vim::tests, T162's vim::insert_tests and T175's vim::visual_tests pass unchanged; the screenshot golden images are unchanged"
    required: true
    manual: false
---

# Vim search matches highlighted in the editor, capped per frame, cleared by :noh

# Vim search matches highlighted in the editor, cleared by :noh

Milestone M7 "Vim", second batch (resized), card 06b: the highlight part of the old card 6. Read
04a (`fe_vim::search::matches_in`), 04c (`VimState::highlight()`), 05a (`:noh`) and T175 (visual
painting in `element.rs`).

**Clean room** as every M7 card: no Vim/Neovim source, no hjkl, no Zed, no Helix.

## Change (`crates/fe-gpui/src/element.rs`, `crates/fe-gpui/src/view.rs`, `crates/fe-gpui/src/vim.rs`)

- While the editor's `VimState::highlight()` is set, prepaint asks `fe_vim::search::matches_in` for
  the visible rows, capped (1,000 ranges per frame), and paint fills each match, under the
  selection and carets, with a search-match colour derived in `element.rs` from the theme
  (`EditorTheme` gains no field: `fe-gpui/src/lib.rs` and the screenshot examples that build it are
  held by LSP cards).
- `:noh` clears them; the next search shows them again; an edit recomputes them on the next frame.
- With vim off nothing is painted and nothing is computed.
- `vim.rs` only declares the test module (and, if needed, a getter for the highlight); no key
  routing changes.

## Tests (new files)

`crates/fe-gpui/src/vim/search_tests.rs`, declared from `vim.rs` (`#[cfg(test)] mod
search_tests;`) next to T161's `tests`, T162's `insert_tests` and T175's `visual_tests`; fe-gpui has
no `src/tests/mod.rs`.

## Size

About 150 non-test lines.

## Out of scope

The current-match colour (nvim's `CurSearch`), 'incsearch', a theme slot for the colour. Any
fe-vim change.

## Decided (lead defaults; the user may revisit)

- **The highlight colour is derived from the theme in `element.rs`** until the theme has a slot for
  it (a later theming card).
