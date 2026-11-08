---
task_uid: "01m6y59dsez2rc5bk5dsqrk4t7"
task: "T1617"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "A helper in crates/fe-gpui/tests/non_latin_text.rs measures, through the editor view, the shaped advance of a CJK char and of one ASCII column and returns n, the CJK chars that fit 20 columns, deciding an exact fit as the wrap code does (named in a comment)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test cjk_soft_wrap_at_a_column_counts_the_shaped_width asserts n \u003e 10 and row starts [0, 3n, 6n] with no hard-coded 33, and its doc comment gives 11 (macOS 27) and 12 (macOS 15) instead of one count"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test cjk_vim_gj_gk_move_by_display_row_over_wrapped_wide_text asserts heads [9, 3n + 9, 6n + 9, 3n + 9, 9] with the same helper"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-gpui --test non_latin_text passes; only crates/fe-gpui/tests/non_latin_text.rs changes; cargo clippy -p fe-gpui --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# CI (macos-15): fe-gpui's CJK soft-wrap tests measure how many wide chars fit 20 columns instead of pinning 11 (macOS 15 fits 12)

# CI (macos-15): fe-gpui's CJK soft-wrap tests pin 11 chars to a row, but macOS 15 shapes 12

## Evidence

GitHub CI's `check` job (macos-15-arm64, macOS 15.8.2) fails two fe-gpui tests on every push to main,
including the Windows-port merge aa9e4988b (run 42503331207). Both pass on this Mac (macOS 27). They are
`#[cfg(target_os = "macos")]`, so the Linux and Windows jobs skip them.

    FAIL fe-gpui::non_latin_text cjk_soft_wrap_at_a_column_counts_the_shaped_width
      crates/fe-gpui/tests/non_latin_text.rs:487:9   left: [0, 36, 72]   right: [0, 33, 66]
    FAIL fe-gpui::non_latin_text cjk_vim_gj_gk_move_by_display_row_over_wrapped_wide_text
      crates/fe-gpui/tests/non_latin_text.rs:583:13  left: [9, 45, 81, 45, 9]   right: [9, 42, 75, 42, 9]

Both came in with T1431 (a074deb60, the non-Latin text audit).

## Cause

Limitation G8: soft wrap at a column counts a wide char by its shaped width. On macOS 27 the CJK fallback
shapes a char at a little over 1.67 editor columns, so 11 fit `wrap_column` 20; on macOS 15 at no more
than 20/12, so 12 fit. The tests hard-code 11 (33 bytes). The behaviour under test (a wide char counts
its shaped width, not 2 columns; rows break only between chars) holds on both; only the pinned count is
font-version specific. No production bug.

## Change (tests only, crates/fe-gpui/tests/non_latin_text.rs)

1. Add a helper that measures, through the same editor view and text system, the shaped advance of one
   CJK char of the row (e.g. `bounds` of `一` vs the next char's x) and of one ASCII column, and returns
   how many CJK chars fit 20 columns: `floor(20 * column / cjk_advance)`, with a small epsilon so an
   exact fit counts as fitting the way the wrap code decides it (read the wrap code and match its
   comparison; name it in a comment).
2. `cjk_soft_wrap_at_a_column_counts_the_shaped_width`: assert the helper's count `n` is greater than 10
   (G8: more than VS Code's and nvim's 10) and that the row starts are `[0, 3n, 6n]`. Keep the doc
   comment's limitation text but say 11 on macOS 27 and 12 on macOS 15 rather than one count.
3. `cjk_vim_gj_gk_move_by_display_row_over_wrapped_wide_text`: expect `[9, 3n + 9, 6n + 9, 3n + 9, 9]`
   with the same `n`, and update its comment.

## Do not touch

Production code, the wrap code, the other tests in the file, fonts, or `wrap_at_20`.
