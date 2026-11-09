---
task_uid: "01m6w2wfep60g381w9st1saq7g"
task: "T304"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-editor --test column_select_visible_columns passes, with keyboard_column_select_counts_visible_columns and select_box_takes_visible_columns"
    required: true
    manual: false
  - id: "C2"
    text: "ColumnBox, EditorModel::select_box, EditorView::box_point_at and FrameLayout::display_column_at work in visible columns through the ExpandedRow helpers from T1501, with no char-column conversion left on those paths; columns past a row end still count one per space width"
    required: true
    manual: false
  - id: "C3"
    text: "In crates/fe-gpui/tests/non_latin_text.rs, cjk_keyboard_column_select_counts_chars becomes cjk_keyboard_column_select_counts_visible_columns and cjk_alt_shift_drag_selects_a_box_of_chars becomes cjk_alt_shift_drag_selects_a_box_of_visible_columns (both rows span the same visible columns), and cargo nextest run -p fe-gpui --test non_latin_text passes on macOS"
    required: true
    manual: false
  - id: "C4"
    text: "Existing column select and mouse box tests pass unchanged on ASCII and tab rows (cargo nextest run -p fe-editor and -p fe-gpui)"
    required: true
    manual: false
---

# Column selection by visible column: cmd-shift-alt-arrows, select_box and alt-shift-drag take the same visible columns on rows with wide chars (T1431 G7) (Mac T1502)

Follow-up of T1431 (docs/decisions/yzegn08wk2-non-latin-text-audit.md, G7). Column (box) selection keeps char columns everywhere:
- ColumnBox in crates/fe-editor/src/column_select.rs (its doc says char column)
- EditorModel::select_box (crates/fe-editor/src/multi_cursor.rs:431, whose doc already says display column)
- EditorView::box_point_at (crates/fe-gpui/src/view.rs:1590)
- FrameLayout::display_column_at (crates/fe-gpui/src/element.rs:204)

All four convert through ExpandedRow::char_column or display_col_for_char_column, so on rows with wide chars the box covers different visible columns on each row.

Defects:
- Keyboard: buffer `ab一二cd\nabcdefgh`, caret at 1, editor.column_select_right twice, then editor.column_select_down. Actual: [1..5, 12..14], b一 above and bc below. Expected (per the audit): [1..5, 12..15], bcd below, visible columns 1 to 3.
- select_box: buffer `abcdefgh\nab一二cd`, select_box((0, 1), (1, 4)). Actual: [1..4, 10..17], bcd and b一二 (char columns). Expected: [1..4, 10..14], bcd and b一 (visible columns 1 to 3 on both rows).
- Mouse: alt-shift-drag from column 1 of `abcdefgh` to the x of its column 5 on `ab一二cd` selects bcd and b一二, whose right edges are a third of a column apart (pinned by cjk_alt_shift_drag_selects_a_box_of_chars).

Fix: the four places use visible columns through the ExpandedRow helpers T1501 adds. A wide cluster counts 2, tabs count their spaces, and an edge inside a wide char snaps to the nearer boundary, a tie to the one before. column_select_left and column_select_right move the head to the previous or next grapheme boundary on the head row, in visible columns; past the row end they still move one column at a time (today's virtual space). column_select_up and column_select_down keep the head's visible column. display_column_at and box_point_at return the visible column of the boundary that was hit, and columns past a row end still add one per space width.

Tests:
- Regression tests go in a new integration test file, crates/fe-editor/tests/column_select_visible_columns.rs, carried to base by the tests/** overlay, so base APIs only: commands::registry().dispatch with EditorTarget and HeadlessHost, as crates/fe-editor/tests/viewer.rs does; EditorModel::select_box; selections. The tests: keyboard_column_select_counts_visible_columns and select_box_takes_visible_columns, with the offsets above. The first check that fails at base must be assert_eq.
- Turn the two G7 pins in crates/fe-gpui/tests/non_latin_text.rs around, as the criteria name. They are macOS-only, so this card is not linux-ok.
