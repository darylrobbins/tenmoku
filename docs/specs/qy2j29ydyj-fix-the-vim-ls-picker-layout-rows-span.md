---
task_uid: "01m6ezbr3czmmaatqy2j29ydyj"
task: "T1319"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "User report with screenshot: with one file open, the :ls picker's selected row hangs below the popover's bottom edge with its text clipped at the top, its highlight ends ~150 px short of the popover's right edge, and 'Buffers' shows both as the placeholder and as a status line under it. Fix the row width and the doubled Buffers label. The vertical clipping does not reproduce headless: it is out of this task's tests and the user checks it in the app after landing. The vim pickers sharing VimPicker (:ls, :marks, :registers) may all take the full-width rows and minimum-width rule; non-vim pickers must lay out as before. Whether :marks and :registers should keep that sizing is a follow-up task, not this one."
    required: true
    manual: false
  - id: "C2"
    text: "tests::vim_ls_layout::selected_row_spans_the_list_width: with one open file whose path is wider than the popover, :ls puts the selected row's bounds (Picker::row_bounds(0)) at the full Picker::list_bounds() width"
    required: true
    manual: false
  - id: "C3"
    text: "tests::vim_ls_layout::list_holds_every_row: the list's height is at least rows x ROW_HEIGHT and every visible row's bounds lie inside list_bounds(), with one and with three open files. This is a plain test, not a regression test: it is expected to pass at the base, because the vertical clipping could not be reproduced headless; it needs no red evidence."
    required: true
    manual: false
  - id: "C4"
    text: "tests::vim_ls_layout::buffers_label_shows_once: the placeholder is Buffers and the status line is not also the bare word Buffers (the header becomes the column heads, or the status is None)"
    required: true
    manual: false
  - id: "C5"
    text: "The regression tests (only selected_row_spans_the_list_width and buffers_label_shows_once) live in the new crates/fe-workbench/src/tests/vim_ls_layout.rs declared from src/tests/mod.rs, use only APIs that exist at the base commit, fail at the base, and make their first failing check an assert_eq!/assert_ne! or a bare assert!(cond) (no custom-message assert!)"
    required: true
    manual: false
  - id: "C6"
    text: "Existing tests still pass: fe-workbench vim_ls_tests, fe-vim --test ex_ls, and the existing :marks and :registers tests"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy -p fe-workbench -p fe-vim -p kiln-shell --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Fix the vim :ls picker layout: rows span the popover, the list fits its rows, and Buffers shows once

## Implementation notes

- Cause: the Picker's row wrapper hugged its content, so a long path made the row far wider than the list and a short one narrower. `PickerDelegate::rows_fill_width` (default off) opts a delegate into a full-width wrapper; only `VimPicker` opts in, so other pickers lay out as before. `:marks` and `:registers` share `VimPicker` and so fill the list too.
- `Picker::row_bounds` now reports each row's painted bounds (recorded by a zero-cost canvas) instead of a rectangle derived from the list.
- The vertical clipping from the report did not reproduce headlessly; `list_holds_every_row` guards the layout but passes at base. To be checked in the running app.
