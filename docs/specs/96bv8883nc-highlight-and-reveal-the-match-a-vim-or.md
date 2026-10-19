---
task_uid: "01m569n2k0gzzcbn96bv8883nc"
task: "T199"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test search_preview_finds_the_landing_match (top level, in crates/fe-vim/tests/search_preview.rs) passes: VimState::search_preview gives the range submitting the line would land on for /, ?, a count, an e offset and a wrap, and None for an empty or invalid pattern, no match, incsearch off, and a slow pattern past its 100 ms deadline."
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test status_bar::vim_line::history_tests::incsearch_tests::typing_a_search_reveals_and_escape_restores (new crates/fe-workbench/src/vim_line/incsearch_tests.rs, declared in crates/fe-workbench/src/vim_line/history_tests.rs) passes: in a real workbench, typing /wor shows the match below the viewport as the current match and scrolls to it, escape restores the scroll and the earlier highlight, and enter lands there."
    required: true
    manual: false
---

# Highlight and reveal the match a vim / or ? search would land on while it is typed, as nvim's 'incsearch' does (Mac T896)

## Problem

At Mac main 6f032b12 typing a `/` or `?` search shows nothing until `Enter` ('incsearch' is listed in
`crates/fe-vim/src/lib.rs`'s Known limits). nvim (default `incsearch` on), VSCodeVim and IdeaVim highlight
the match the search would land on, and move the view to it, while the pattern is typed. The user approved
it in the "Vim parity" track on 2026-10-17; the search-settings card adds `vim.incsearch` (default on) and
the shared flag.

## Change

- fe-vim (`crates/fe-vim/src/cmdline.rs`): `VimState::search_preview(&self, model, line: &str) ->
  Option<Range<usize>>`, pure: the match a submit of `line` would land on now (the line's kind, count and
  offset as submit takes them), or `None` (an empty or invalid pattern, no match, incsearch off). It runs
  under a short deadline of its own (100 ms), giving `None` when interrupted, so typing never waits on a
  slow pattern.
- fe-workbench (`crates/fe-workbench/src/vim_line.rs`): after each edit of a `/` or `?` line, it asks for
  the preview and hands it to the editor view, which shows it as the current match (the find bar's current
  match colour) with every other match of the pattern as search highlights, and scrolls to reveal it.
  Cancelling restores the view's scroll and the previous highlight; submitting lands as today.
- fe-gpui (`crates/fe-gpui/src/vim.rs`): the view keeps the preview and the scroll to restore, and paints it.

Tests: a new `crates/fe-vim/tests/search_preview.rs` (forward, backward, count, offset, no match, invalid
pattern, incsearch off, a slow pattern interrupted), and a new
`crates/fe-workbench/src/vim_line/incsearch_tests.rs`, declared in the existing
`crates/fe-workbench/src/vim_line/history_tests.rs` by `#[path = "incsearch_tests.rs"] mod incsearch_tests;`,
driving the real line in a workbench: typing `/wor` highlights and reveals the match below the viewport,
`escape` scrolls back.

vim_line.rs is changed by T832 (lent) and T247: this card depends on T832.

## Implementation notes

- Scope was widened (respec) to `crates/fe-vim/src/state.rs` (the 'incsearch' flag, `incsearch_off`, so the
  default `VimState` has it on), `crates/fe-vim/src/lib.rs` (the Known limits entry, and the search section),
  `crates/fe-gpui/src/view.rs` (the view's preview field; its rows first in view read from vim.rs) and
  `crates/fe-gpui/src/element.rs` (painting), plus two new fe-gpui test files.
- `VimState::search_preview` gives the match itself (as submit's search finds it, the line's offset taken
  as submit takes it: an `e` offset picks matches by their last character), not where the offset moves the
  caret. It searches from the caret as a move or an operator takes it (on its row's last character at most;
  the vim cursor in a visual mode), and walks matches with `cmdline::offset::nth_match` under
  `fe_vim::SEARCH_PREVIEW_LIMIT` (100 ms, or the process-wide search limit if shorter). It doesn't intern
  the typed pattern, so typing doesn't grow `SearchPattern`'s interned set. An empty pattern (the last
  search's) previews nothing. `VimState::set_incsearch` turns it off; the search-settings card wires
  `vim.incsearch` to it.
- The view keeps where it was before the line opened (captured as the opening command runs, before its
  autoscroll). Each `EditorView::preview_search` first puts that view and any folds it opened back, then
  opens the folds hiding the new match and has the next frame scroll to it as the layout's autoscroll does
  a caret (its match start in place of the caret: both axes, block rows counted, the caret left alone).
  The match paints in the selection colour (the find bar's current match, with vim off, is the selection)
  over the line's pattern's matches, which paint in place of the find bar's and vim's
  (`EditorView::match_pattern`), under the preview's 100 ms limit. A cancel (any path: `escape`, a click
  outside, blur, a tab switch, vim turned off) puts back the view, its pending autoscroll and the folds. A
  submit keeps the previewed view and open folds, the caret landing on the match.
- Carried findings fixed: T47 F5 (a line cancelled without a key records its text and `<C-c>`, which a
  replay cancels with), T198 F1 (a new `VimState` resets the yank serial seen and ends the flash), T198 F2
  (a command whose dispatch also edited the buffer flashes nothing, since its yank ranges may be stale).
