---
task_uid: "01m6w2wf669y897nb2thfa7ppr"
task: "T303"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-editor --test up_down_visible_columns passes, with down_keeps_the_visible_column_over_wide_chars, down_lands_on_cluster_boundaries and viewer_down_keeps_the_visible_column_and_cluster_boundaries"
    required: true
    manual: false
  - id: "C2"
    text: "The visible-column rule (a grapheme cluster counts 2 when its first char is wide by the VS Code isFullWidthCharacter table now in minimap.rs is_wide, else 1; expanded tabs count their spaces; a column inside a cluster snaps to the nearer boundary, a tie to the one before) is one pub ExpandedRow helper pair in crates/fe-display/src/lib.rs, used by both EditorModel::vertical and ViewerModel::vertical, with unit tests beside the existing ExpandedRow tests (CJK, combining mark, ZWJ emoji, tab before a wide char, the tie rule)"
    required: true
    manual: false
  - id: "C3"
    text: "In crates/fe-gpui/tests/non_latin_text.rs, cjk_up_down_keep_the_char_column_not_the_x becomes cjk_up_down_keep_the_visible_column and grapheme_up_down_can_land_inside_a_cluster becomes grapheme_up_down_land_on_cluster_boundaries, asserting the new offsets, and cargo nextest run -p fe-gpui --test non_latin_text passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-editor passes, including the viewer test moves_like_the_editor, and a soft-wrapped row still ends before its last grapheme as today"
    required: true
    manual: false
---

# Keep Up/Down on the visible column and on grapheme boundaries: a wide char counts two columns, and no caret lands inside a cluster (T1431 G5, G6) (Mac T1501)

Follow-up of T1431 (docs/decisions/yzegn08wk2-non-latin-text-audit.md, G5 and G6). EditorModel::vertical (crates/fe-editor/src/lib.rs, around :1112) keeps a char column as its goal and turns it back into an offset with no grapheme snapping. ViewerModel::vertical (crates/fe-editor/src/viewer.rs, around :290, char_column and offset_at_char_column) copies it.

Defects:
- G5: buffer `abcdefghij\n一二三四五六七八\nabcdefghij`, caret at 6. Down lands at 29: after 六, the sixth char, drawn about 1.67 times as far right. Down again goes to 42, so the caret zigzags. Expected (VS Code CursorColumns, nvim j): keep the visible column, a wide char counting two. Down lands at 20 (after 三), Down again at 42, Up Up back at 6.
- G6: buffer `abcdef\ne\u{301}xyz\n👨\u{200d}👩\u{200d}👧xyz`. From caret 1, Down lands at 8, between e and U+0301. From caret 3, Down Down lands at 11 and then 25, inside the family emoji. Typing there splits the cluster. Expected: carets only on grapheme boundaries: 10; and 12 then 33 (the family is one cluster, 2 columns wide).

Fix:
- Add to ExpandedRow (crates/fe-display/src/lib.rs, beside char_column and display_col_for_char_column) a pub pair of helpers: the visible column of a display offset, and the display offset for a visible column. Walk grapheme clusters. A cluster counts 2 when its first char is wide by the VS Code isFullWidthCharacter table fe-display already has (minimap.rs is_wide; move it somewhere shared and keep minimap using it), else 1. Expanded tabs count their spaces. A visible column inside a cluster snaps to the nearer boundary, a tie to the one before (VS Code columnFromVisibleColumn). No new crate dependency.
- EditorModel::vertical and ViewerModel::vertical keep that visible column as their goal and land through the reverse mapping, so every landing offset is a grapheme start. Keep today's soft-wrap end rule and fold skipping. Other char_column users (rows.rs, long_row.rs, column selection) are out of scope; a follow-up card for column selection reuses these helpers.

Tests:
- Regression tests go in a new integration test file, crates/fe-editor/tests/up_down_visible_columns.rs. The tests/** overlay carries it to base, so it may use base APIs only: EditorModel::new, set_selections, move_cursors(Movement::Down / Up, false), primary_selection; ViewerModel::new over fe_buffer::Text, set_selection, move_cursor, selection. The tests: down_keeps_the_visible_column_over_wide_chars (the G5 offsets above, Up included), down_lands_on_cluster_boundaries (the G6 offsets above), viewer_down_keeps_the_visible_column_and_cluster_boundaries (both repros through ViewerModel). The first check that fails at base must be assert_eq / assert_ne or a bare assert(cond) with no custom message.
- crates/fe-gpui/tests/non_latin_text.rs pins the old behaviour. Turn its two pins around, as the criteria name, and update their doc comments.
