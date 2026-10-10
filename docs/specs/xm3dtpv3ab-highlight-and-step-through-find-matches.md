---
task_uid: "01m4fhv84e8m78v8xm3dtpv3ab"
task: "T264"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::find_tests::highlights_visible_rows: with a foo pattern set on a 3,000-row buffer with foo on every row, a drawn frame computes and paints matches only on the visible rows and never more than 1,000; set_find(None) paints and computes nothing; an edit that adds a foo shows on the next frame"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::find_tests::next_previous_and_from: find_next and find_previous select and reveal the next and previous match, wrapping past either end; find_from(offset) selects a match starting at offset; with no match or no pattern they return false and leave the selection; with vim on the caret lands on the match's start with an empty selection"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::find_tests::count_off_thread: find_count is None while counting, then reports total 12 with current as the selected match's 1-based index; 20,000 matches report total 10,000 with capped set; an edit during a count drops the stale result and the recount includes the edit"
    required: true
    manual: false
  - id: "C4"
    text: "Scope: nothing under crates/fe-vim changes; fe-gpui's lib.rs and EditorTheme don't change; every earlier fe-gpui test passes with its files unchanged, screenshot goldens included"
    required: true
    manual: false
---

# Highlight and step through find matches in the editor, with a match count computed off the main thread

## Problem

Card 2 of 4 for in-file find (cmd-F), asked by the user on 2026-10-09: "In-file find (cmd-F) reusing T213's
row matcher". Card 1 gives `fe_vim::search::FindQuery`, `Pattern::from_query` and `Pattern::find_from`. Nothing
in `fe-gpui` paints matches or steps between them: `EditorView::select_and_reveal`
(`crates/fe-gpui/src/view.rs`) is the only "go to this range", and T220 (ready) will paint Vim's search
highlight in `element.rs` but depends on the T214–T216 chain.

## Change

New `crates/fe-gpui/src/view/find.rs`, declared from `view.rs` (`mod find;`), holding an `impl EditorView` and
the view's find state (one new field on `EditorView` in `view.rs`).

- `pub fn set_find(&mut self, pattern: Option<fe_vim::search::Pattern>, cx)`: the pattern to highlight and
  step through; `None` clears it. With none set, nothing is computed or painted.
- Highlight: prepaint calls `fe_vim::search::matches_in` for the visible rows only, capped at 1,000 ranges per
  frame; paint fills each match under the selections and carets with a find-match colour derived in
  `element.rs` from the `EditorTheme` (no new `EditorTheme` field: `fe-gpui/src/lib.rs` and the screenshot
  examples that build it are held by LSP and theme cards). An edit shows on the next frame.
- `pub fn find_next(&mut self, cx) -> bool` and `find_previous`: `Pattern::find` from the primary selection's
  start, forward or back (so a selected match is stepped past), wrapping; `pub fn find_from(&mut self, offset,
  cx) -> bool`: `Pattern::find_from(offset)`, for type-ahead. Each selects the match through
  `select_and_reveal`; with vim on (`vim_mode().is_some()`) the caret goes to the match's start with an empty
  selection, as `/` leaves it. No match, or no pattern: `false`, nothing changes.
- `pub fn find_count(&self) -> Option<FindCount>`, `pub struct FindCount { pub total: usize, pub capped: bool,
  pub current: Option<usize> }` (`current` is 1-based: the match equal to the primary selection, or starting at
  the caret with vim on). Counted off the main thread: `set_find` and every edit start a count on the
  background executor over a clone of the buffer's `Text`, in chunks of 4,096 rows through `matches_in`,
  keeping at most 10,000 match ranges (`capped` past that). A count started before a later edit or `set_find`
  stops at its next chunk and is dropped; `find_count` is `None` while counting; the view notifies when a
  count lands.

## Tests (new file)

`crates/fe-gpui/src/view/find_tests.rs`, declared `#[cfg(test)] mod find_tests;` from `view.rs` next to
`reveal_tests`.

## Size

About 250 non-test lines.

## Out of scope

The find bar and its commands (card 3), keys (card 4), a distinct current-match colour, a theme slot for the
colour, any `crates/fe-vim` change. T220 may later paint Vim's highlight through `set_find`'s path (the lead's
call).
