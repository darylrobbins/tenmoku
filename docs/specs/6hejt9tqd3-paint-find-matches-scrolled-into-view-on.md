---
task_uid: "01m4nqf1sp1cfckx6hejt9tqd3"
task: "T32"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "view::find_tests::scroll_tests::dense_row_scrolled_right_paints_its_matches: one row of \"foo \" x 20,000, soft wrap off, foo set, scroll.x = 150,000 px with autoscroll off: at least one match is painted, every painted rect is inside the text area, and found() \u003c= painted + 64"
    required: true
    manual: false
  - id: "C2"
    text: "view::find_tests::scroll_tests::dense_wrapped_row_scrolled_down_paints_its_matches: the same row with soft wrap on, scroll.y = 20,000 px: the frame shows only buffer row 0, every displayed row has its foo matches painted, and found() \u003c= painted + 64"
    required: true
    manual: false
  - id: "C3"
    text: "view::find_tests::scroll_tests::rows_after_a_dense_row_paint_their_matches: the dense row followed by 30 rows of foo, soft wrap off, unscrolled: every displayed row after the first has exactly one painted match"
    required: true
    manual: false
  - id: "C4"
    text: "The find budget stays at most MAX_MATCHES found ranges per frame. vim's highlight paints as before: crates/fe-gpui/src/vim/search_tests.rs and find_tests.rs pass, with only the dense-row found() assertion in highlights_visible_rows changed as the spec says. In crates/fe-vim only search.rs changes (one new pub fn)"
    required: true
    manual: false
---

# Paint find matches scrolled into view on a row dense with them, spending the 1,000-match budget only on what's in view (Mac T320)

## Problem

T264's review (2026-10-10, codex, F3 medium; filed in T318): in `match_rects` (`crates/fe-gpui/src/element.rs`
~1229–1300), the find pattern's 1,000-range budget goes to `fe_vim::search::matches_in`, which counts from each
row's start. That happens before any filtering for visibility. On a row of "foo " x 20,000 only offsets up to
~3,999 are found. Scrolling right, or down within the wrapped row, paints nothing even though fewer than 1,000
matches are in view. The used-up budget also leaves later visible rows unpainted. vim's path, from T220,
enumerates the whole row with no cap and filters afterwards, so it doesn't have this bug.

## Change

- `crates/fe-vim/src/search.rs`: add `pub fn matches_reaching(text: &Text, row: u32, window: Range<usize>,
  pattern: &Pattern, cap: usize) -> Vec<Range<usize>>`. It returns the matches on `row` exactly as
  `matches_in` finds them (searched from the row's start with `Pattern::matches`, so whole-row semantics
  don't change), keeping only those that reach `window`. A non-empty match reaches it if
  `end > window.start && start < window.end`; an empty one if `window.start <= start <= window.end`. It keeps
  at most `cap` and stops at the first match that starts past `window.end`. Nothing else in fe-vim changes.
- `match_rects`: keep T220's structure (one enumeration per displayed buffer row, `partition_point` per
  displayed row). Each buffer row's window runs from the offset at the text area's left edge on its first
  displayed row to the offset at the right edge on its last displayed row (as `Frame::offset_at` finds them).
  Get that row's matches with `matches_reaching(text, row, window, pattern, budget)`. The find pattern keeps
  `budget = MAX_MATCHES` for the whole frame; vim keeps `usize::MAX`. Update the doc comment.
- `crates/fe-gpui/src/view/find_tests.rs`: in `highlights_visible_rows`'s dense single-row block, replace
  `assert_eq!(found(), crate::element::MAX_MATCHES)` with `assert!(found() <= shown.len() + 64)`.

## Tests (new file)

`crates/fe-gpui/src/view/find_tests/scroll_tests.rs`, declared from `find_tests.rs` as
`#[cfg(test)] mod scroll_tests;` (commit that line). Use `super::*` helpers, and set `scroll` and `autoscroll`
as T220's `vim/search_tests.rs` does.

## Out of scope

Per-frame scan cost of the row up to the window on very long lines, for both vim and find: that's T312.
