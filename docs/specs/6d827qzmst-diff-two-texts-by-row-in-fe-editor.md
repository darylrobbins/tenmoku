---
task_uid: "01m4rhpf4njfpzx86d827qzmst"
task: "T533"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test text_diff::hunks_and_kinds: for a left/right pair with one added, one removed and one modified run, hunks() returns three hunks with the exact left and right row ranges and kinds Added, Removed, Modified; identical texts give none; a side over MAX_DIFF_BYTES gives too_large() and no hunks"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test text_diff::alignment: a 3-row left run replaced by 5 right rows gives fillers(Left) == [(left.end, 2)] and fillers(Right) empty; across a file with an addition, a removal and a modification every pair of unchanged rows has equal screen_row on both sides, and row_at_screen(side, screen_row(side, r)) == (r, false) for every row"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test text_diff::navigation: next_hunk/prev_hunk/hunk_at on both sides, including a deletion's insertion row, the first and last rows, and None past the last hunk (no wrap); removed_above lists each hunk with left rows at its right.start"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test text_diff::apply: apply with all hunks equals right, with none equals left, with the middle one of three changes only that one; apply_rows on part of a 3-into-3 modification replaces only the paired rows chosen, on a 2-into-4 hunk inserts unpaired chosen rows after the last left row, removes unpaired left rows only when the last right row is chosen, and stages a Removed hunk from the row above it; CRLF left text keeps CRLF and the final-newline rule holds"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-editor passes with row_changes tests unchanged; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Diff two texts by row in fe-editor: TextDiff hunks, side-by-side filler alignment, inline removed rows, hunk navigation, and applying chosen hunks or rows

## Problem

The user (2026-10-12) asked for git diff views, hunk staging and reverting, and a "compare two files" view. Each of
those needs the same row diff of two texts with more than `row_changes.rs` gives: `RowChange`
(`crates/fe-editor/src/row_changes.rs` :35) has only the current text's rows, no base rows, so nothing can align two
sides, show removed rows inline, or rebuild a text with some hunks applied (what staging a hunk writes to the index).
This card is the GPUI-free engine every diff, staging and revert card builds on.

## Change

New file `crates/fe-editor/src/text_diff.rs`, `pub mod text_diff` and `pub use text_diff::{TextDiff, Hunk, HunkKind,
Side}` from `crates/fe-editor/src/lib.rs`. It reuses `row_changes.rs`'s row token source (`Rows`, `rows`), made
`pub(crate)` there with no behaviour change, and imara-diff's Histogram algorithm as row_changes does.

- `TextDiff::new(left: &str, right: &str) -> TextDiff`; over `MAX_DIFF_BYTES` on either side, `too_large()` is true
  and there are no hunks.
- `hunks() -> &[Hunk]`, `Hunk { left: Range<u32>, right: Range<u32> }` in rows, sorted; an empty range is an
  insertion point (the row the other side's rows sit above). `Hunk::kind()`: `Added` (left empty), `Removed` (right
  empty), `Modified`.
- Side by side (VS Code and Zed align this way): `fillers(side) -> Vec<(u32 /* above row */, u32 /* rows */)>`. For
  each hunk the shorter side gets `|left.len() - right.len()|` filler rows above its row `range.end`, so the k-th
  rows after every hunk line up. `screen_row(side, row) -> u32` (row plus fillers above it) and
  `row_at_screen(side, screen) -> (u32, bool /* in a filler */)`, for synced scrolling.
- Inline: `removed_above() -> Vec<(u32 /* right row */, Range<u32> /* left rows */)>`: each hunk with left rows shows
  them above right row `hunk.right.start`.
- Navigation, no wrapping (callers wrap): `next_hunk(side, row)`: the first hunk whose start row on `side` is after
  `row`; `prev_hunk(side, row)`: the last one starting before `row`; `hunk_at(side, row)`: the hunk containing `row`,
  or, for an empty range, the hunk whose insertion row is `row`.
- Applying, for staging: `apply(left, right, &[usize]) -> String`: `left` with the chosen hunks' left rows replaced by
  their right rows. `apply_rows(left, right, right_rows: Range<u32>) -> String`, for a visual line range: in each
  hunk meeting `right_rows`, the hunk's i-th right row pairs with its i-th left row; a chosen right row (inside
  `right_rows`) replaces its paired left row, or, unpaired, is inserted after the hunk's last left row; left rows
  paired with unchosen right rows stay; unpaired left rows (left longer) are removed only when `right_rows` holds the
  hunk's last right row. A `Removed` hunk is chosen when `right_rows` holds its insertion row or the row above it
  (gitsigns stages a deletion from the row next to it). Kept rows keep `left`'s line endings; inserted rows take
  `left`'s first line ending (`\n` if none); whether the text ends with a newline follows `right` when a chosen hunk
  touches the last row.

Not in this card: word-level (intra-row) highlights.

## Tests

New file `crates/fe-editor/tests/text_diff.rs`.
