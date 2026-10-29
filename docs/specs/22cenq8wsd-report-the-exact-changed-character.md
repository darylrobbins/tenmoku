---
task_uid: "01m62s3ewwsh0cyk22cenq8wsd"
task: "T375"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test inline_diff_tests::one_word: rows 'let x = foo(1);' and 'let x = bar(1);' report only 'foo' on the left and 'bar' on the right, as byte ranges within each row"
    required: true
    manual: false
  - id: "C2"
    text: "inline_diff_tests::pure_rows: an added or removed hunk's rows report no inner spans (callers tint the whole row); an unchanged row reports none"
    required: true
    manual: false
  - id: "C3"
    text: "inline_diff_tests::pairing: a 2-row left run replaced by 3 right rows pairs rows in order and leaves the extra right row whole"
    required: true
    manual: false
  - id: "C4"
    text: "inline_diff_tests::bounds: a row over MAX_INLINE_BYTES or a hunk over MAX_PAIRED_ROWS reports no spans, in milliseconds; multibyte graphemes are never split (spans fall on char boundaries)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-editor passes with text_diff and row_changes tests unchanged; no file outside the scope changes"
    required: true
    manual: false
---

# Report the exact changed character ranges inside a modified row pair in fe-editor's TextDiff, for word-level diff highlights (Mac T1216)

## Problem
VS Code tints a modified line then darkens the exact changed characters. fastedit's DiffView tints whole rows only (fe-gpui diff_view.rs `tinted_rows`). TextDiff (crates/fe-editor/src/text_diff.rs) has hunks with row ranges and nothing inside a row. Keep tests small: row diff is quadratic on many interleaved changes (imara Histogram), so tokenise words, not chars, and cap sizes.
## Change
New inline_diff.rs: `TextDiff::changed_spans(side, row) -> &[Range<usize>]` (lazy, cached per hunk) using imara-diff on word tokens (alnum runs, punctuation, whitespace) of paired rows of a Modified hunk.

## Implementation notes

- `TextDiff` keeps no texts and `text_diff.rs` is outside the scope, so the
  spans come from a borrowed view: `TextDiff::inline(left, right) ->
  InlineDiff`, whose `changed_spans(side, row) -> &[Range<usize>]` works out
  a hunk's pairs on first ask and caches them per hunk (`OnceCell`). Rows
  are split lazily, per side, on first need.
- Tokens: graphemes grouped into runs of word characters and runs of
  whitespace (by `CharClass` of each grapheme's first char); every other
  grapheme is a token alone. Spans are therefore grapheme-aligned. Pairs go
  through `row_changes::row_diff` (Histogram under its work bound, else
  Myers) and `postprocess_no_heuristic`.
- Caps: `MAX_INLINE_BYTES` = 1000 per row of a pair, `MAX_PAIRED_ROWS` = 200
  rows on either side of a hunk.
- `lib.rs` only gains `pub mod inline_diff;` and `pub use
  inline_diff::InlineDiff;`. The carried T372 F1 (caret mapping across an
  undone covering deletion, in `History`) is unrelated to this change and
  is left for its own card.
