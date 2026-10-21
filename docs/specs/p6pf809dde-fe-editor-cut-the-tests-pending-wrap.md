---
task_uid: "01m5fzznr6pq6k44p6pf809dde"
task: "T269"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "tests::pending_wrap::toggle_wraps_the_cursor_rows_first, tests::pending_wrap::narrowing_keeps_the_top_row_in_place, tests::pending_wrap::widening_from_a_continuation_row_keeps_its_text_in_place and tests::pending_wrap::every_slice_measures_at_most_a_slice_of_text each take under 5 s in a debug build on an idle machine (cargo nextest run -p fe-editor pending_wrap)"
    required: true
    manual: false
  - id: "C2"
    text: "Each still proves that more than one wrap slice is needed: the text is over 4 x WRAP_SLICE_BYTES (128 KiB), asserted in the test, and every_slice_measures_at_most_a_slice_of_text still asserts slices \u003e= text.len() / WRAP_SLICE_BYTES"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-editor passes"
    required: true
    manual: false
---

# fe-editor: cut the tests::pending_wrap tests (29 to 39 s each) with a smaller text and fewer lines

Test-only speed-up, user-approved 2026-10-21. The tests lay out 200,000 lines (~12 MB, ~100x WRAP_SLICE_BYTES) or 1,000,000 non-ASCII rows, then wrap to the end slice by slice, and assert_wrapped_as_in_one_go compares every row with a full reference map.

- LINES 200_000 -> 20_000 (~1.2 MB, about 9 slices) and move the "middle" and "far" rows with it (100_000 -> LINES / 2, 150_000 -> LINES * 3 / 4). 8 slices is plenty for "toggle wraps the cursor rows first, the rest finish in later slices".
- non_ascii_text 1_000_000 rows -> 120_000 rows (~1 MB, 8+ slices; wrap_pending(500_000) per call stays as is).
- Replace the row-by-row `assert_eq!(model.display().row(row), reference.row(row))` over all rows with every 11th row plus the 200 rows around the cursor/top and the last 200, plus the row_count equality.
- Keep the long 11-char x100 row in widening_from_a_continuation_row so there are still many continuation rows.
Pure fe-editor model tests, runs on Linux.
