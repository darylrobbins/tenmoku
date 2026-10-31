---
task_uid: "01m67g1h9w34asc47z4cakag1t"
task: "T380"
rev: 5
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-gpui test diff_fold_tests::context: with 3 context rows, an 80-row file with one change at row 40 shows rows 37-43 and two 'N hidden lines' blocks (36 and 37); changes within 6 rows of each other merge into one visible run"
    required: true
    manual: false
  - id: "C2"
    text: "diff_fold_tests::expand: a chevron click reveals up to 20 rows from the nearer end of its block, a click on the label reveals all; both sides stay aligned and the scroll sync holds"
    required: true
    manual: false
  - id: "C3"
    text: "diff_fold_tests::crumb: a hidden block's label carries the enclosing symbol chain of the row below it (from fe-syntax) when the language has one, nothing otherwise"
    required: true
    manual: false
  - id: "C4"
    text: "diff_fold_tests::navigation: next/previous change onto a hidden hunk is never needed (hunks are never hidden); caret movement into a hidden block expands it Known limit: a lone caret at offset 0 that the user moved there may be refolded away, as the untouched initial one is (follow-up T381)"
    required: true
    manual: false
  - id: "C5"
    text: "diff_fold_tests::toggle: a diff.toggle_folding command (default on for commit and git diffs, off for compare-two-files) shows everything when off; inline mode folds the same"
    required: true
    manual: false
  - id: "C6"
    text: "cargo test -p fe-gpui passes, including the guard that kept both sides unfolded (diff_view.rs:435); clippy clean"
    required: true
    manual: false
---

# Fold a DiffView's unchanged regions into 'N hidden lines' rows with expand chevrons and the enclosing symbol's breadcrumb, the same rows hidden on both sides (Mac T1224)

## Problem
VS Code's diff folds unchanged regions to "2011 hidden lines" with an expand chevron and the symbol breadcrumb. DiffView keeps both sides unwrapped and unfolded on purpose (diff_view.rs ~:435) because fillers line up buffer rows. This is the larger design card: prefer hidden-row blocks computed in a new diff_fold.rs from TextDiff::hunks, drawn as filler-style blocks, over the editor's own folds. If it cannot be done in one session, split: model (diff_fold.rs + tests) then view, and tell the lead.

Known limit (C4, review F3): a lone caret at offset 0 that the user moved there can't be told from the untouched initial one, so it may be refolded away; tracking whether each side's initial caret was touched across frames and refolds is helium T381.

## Implementation notes

- The model is `crates/fe-gpui/src/diff_fold.rs` (a child module of `diff_view`): `hidden_blocks` finds the
  unchanged runs between `TextDiff::hunks`, less `CONTEXT_ROWS` (3) next to each hunk and the rows already
  shown, as `HiddenBlock { left, right }` (as long on both sides). Rows shown are kept in left rows, which a
  diff view never edits, so they stay shown through edits to the right side. No hunks (same texts, or too
  large to compare) hides nothing.
- Hiding: the display layer has no header-less hidden rows, and blocks only add rows, so each block is hidden
  by an editor fold on each side whose header is the row above it, set with `fold_all_exact`; the label is a
  one-row `HostBlockKind::Reserve` block below it, which the view draws over (chevrons, `N hidden lines`,
  crumb) from the side's `reserved_blocks`, as the workbench's peek does. Both sides fold the same rows, so
  the fillers and the scroll sync hold unchanged.
- A fold can't hide a text's first row, so a block starting the text folds rows 1.. behind row 0 and draws its
  label over row 0 itself (no reserve block): C1's 80-row case shows the label, rows 37–43, the label. Only the
  untouched caret a view opens with (one empty selection at offset 0) may stay under that label; any other
  caret on row 0, or that one once it moves (`gg`), shows the block.
- The fold header row shows the editor's `⋯` marker and folded chevron; opening that fold (marker, `zo`,
  `select_and_reveal`) shows the block on both sides, as does any caret, primary or not, that lands in hidden
  rows (vim `G`, a model `set_cursor`); turning folding on shows the blocks carets are in rather than letting
  the folds move them. Any other fold is still undone (the old guard), and soft wrap stays off.
- Chevrons: `⌄` shows the block's first 20 rows (below the run above it), `⌃` its last 20 (above the run
  below); the label shows all of it.
- The crumb is from `fe_syntax::outline` (found off the main thread per language and syntax version, per side;
  a job for a language or version the side no longer has is dropped): the items
  containing the first non-blank char of the row below the block, or of its last row at the text's end.
- `diff.toggle_folding` ("Toggle Unchanged Regions") is in the diff registry; `DiffView::with_folding` /
  `set_folding` let a host choose. Folding starts on; the workbench's `diff.compare_file` / `:diffsplit` turns
  it off. Conflict compare (`git.compare_conflict`) keeps it on.
- `set_inline` keeps the top row through the fold-aware `ScreenRows` mapping (fillers or removed rows, label
  rows, folded rows) instead of `TextDiff::screen_row`.
- Four `diff_view_tests` that scroll or navigate over every row of long texts turn folding off first.
- The workbench's change-bar reveal is `DiffView::reveal_right_row`: it shows the block holding the row and
  scrolls to its fold-aware screen row, and holds the request, applying it again as each diff lands, until the
  diff of the texts as they are has landed (texts over 64 KiB diff off the main thread) or a side is scrolled.
- Refolding catches both models up with their buffer (`sync_display`) before reading carets, so a caret moved by an
  edit through another split keeps its block shown. Label rows block clicks but not the wheel
  (`block_mouse_except_scroll`): scrolling over one scrolls the side beneath, and the other follows.
