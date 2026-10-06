---
task_uid: "01m46tzw20k38ns79b1bk2bnmh"
task: "T74"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test view::reveal_tests::reveals_an_offscreen_range: in a 500-row file scrolled to the top, select_and_reveal of a range on row 400 leaves exactly that one selection (head at its end) and, after the next frame, row 400 is among the laid-out visible rows"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::reveal_tests::clamps_stale_offsets: a range past the buffer end becomes a caret at the end, an offset inside a multi-byte char moves to the char boundary on its left, and a reversed range is normalised, with no panic"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::reveal_tests::unfolds_a_hidden_target: with rows 10-40 folded, select_and_reveal of an offset on row 20 unfolds that fold and the caret row is visible after the next frame; folds elsewhere stay folded"
    required: true
    manual: false
  - id: "C4"
    text: "The only change to crates/fe-gpui/src/view.rs besides select_and_reveal and its doc comment is the cfg(test) mod reveal_tests line; existing fe-gpui tests pass unchanged"
    required: true
    manual: false
---

# Let a host select a range in an EditorView and scroll it into view

# Let a host select a range in an EditorView and scroll it into view

Milestone M3 "Browse", third batch. A small prerequisite split out of the search panel and outline panel
cards. Both need to put the caret at a position in an open editor and bring it into view: a search match,
or an outline symbol. Today only `EditorView`'s own actions can do that. `EditorView::changed`, which
sets `autoscroll`, is private. Tests reach in through `view.model.set_cursor(..)` plus
`view.changed(cx)` (`crates/fe-gpui/src/view/window_tests.rs`).

## Problem

`fe-workbench` can call `EditorView::model_mut().set_selections(..)`, but nothing scrolls the editor
to the new selection, and nothing redraws it. A target inside a fold stays hidden. A byte offset that
came from disk can be past the end of the buffer or inside a UTF-8 char. That happens when the file
was edited after a search, or for an outline built from an older version.

## Change

`crates/fe-gpui/src/view.rs`:

- `pub fn select_and_reveal(&mut self, range: Range<usize>, cx: &mut Context<Self>)`:
  - Clamps both ends to the buffer's length and moves them to char boundaries with
    `Text::clip_offset(.., Bias::Left)`. A reversed range (`start > end`) is normalised.
  - Unfolds any fold that hides the row of `range.start` (`EditorModel::unfold_rows`).
  - Sets a single selection over the clamped range, with its head at the end. An empty range is a
    caret.
  - Sets `autoscroll` and calls `cx.notify()`. This is the same path the movement actions take
    through `changed`, so the target row is scrolled into view on the next frame the way keyboard
    movement does it.
- It doesn't focus the view. The caller decides.
- Doc comment: this is the host API for "go to" jumps (search results, outline, later go-to-def).

Tests go in a new file, `crates/fe-gpui/src/view/reveal_tests.rs`, declared next to `window_tests`
with `#[cfg(test)] mod reveal_tests;`. The only edit to `view.rs` besides the method is that line. The
tests run in headless GPUI on the pattern of `window_tests.rs`.

## Commands

This card adds no command or key binding. It's a host API.

## Out of scope

- Centering the target in the viewport.
- Highlighting every match of a search in the editor.
- Selecting more than one range.

## Decided (lead defaults; the user may revisit)

- The jump scrolls the way caret movement does (the minimum to show the row), not centred. Centring
  can be a flag later if jumps feel disorienting.
- A clamped offset lands on the char boundary to its left.
