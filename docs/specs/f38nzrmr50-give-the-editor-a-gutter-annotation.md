---
task_uid: "01m55b6tc7rg8w3hf38nzrmr50"
task: "T816"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests fe-display layout::fold_gutter_tests::annotation_column_tests::a_zero_width_column_lays_out_as_before (with annotation width 0, text_left, every line number origin, fold_column, change_lane and the wrap width equal layout_frame_with_inlay's and wrap_width's) and layout::fold_gutter_tests::annotation_column_tests::the_column_comes_before_the_line_numbers (with width w, annotation_column is x 0..w over the frame's height, and text_left, every line number origin, fold_column and change_lane are exactly w further right, the wrap width w less) pass"
    required: true
    manual: false
  - id: "C2"
    text: "Test fe-gpui fold_gutter_tests::annotation_column_tests::entries_paint_once_per_run_tinted_by_heat passes: with entries over rows 0..3 (heat 1.0) and 4..5 (heat 0.0) and row 3 in none, each entry's text is painted only on its run's first row, every row of an entry is tinted, the heat-1.0 tint's alpha exceeds the heat-0.0 one's, row 3 has no text or tint, and the column is the widest entry's shaped width plus 1 em, capped at 32 em for an entry longer than that"
    required: true
    manual: false
  - id: "C3"
    text: "Test fe-gpui fold_gutter_tests::annotation_column_tests::hovering_and_clicking_an_entry passes: the pointer resting on row 1's annotation_bounds shows hover(0, 1)'s text as popover_text() after HOVER_DELAY, and moving into the text hides it; a left click on row 4's annotation_bounds calls on_click(1, 4) once and leaves the selections and folds unchanged; a click on row 3's calls nothing"
    required: true
    manual: false
  - id: "C4"
    text: "Test fe-gpui fold_gutter_tests::annotation_column_tests::edited_rows_go_blank_and_later_rows_follow passes: inserting a row inside the first entry leaves the inserted row with no text or tint and the second entry painted one row lower; set_gutter_annotations(None) removes the column, text_left returning to the gutter's width"
    required: true
    manual: false
  - id: "C5"
    text: "cargo run -p fe-gpui --example screenshots passes on the Mac with the new annotated_gutter scene's golden (golden/annotated_gutter.png, made with UPDATE_SCREENSHOTS=1) and every other scene's golden unchanged; the existing fe-display and fe-gpui test suites pass unchanged"
    required: true
    manual: false
---

# Give the editor a gutter annotation column left of the line numbers: per-row host text and tint, sized to its widest entry, with hover and click callbacks

## Problem

The user, 2026-10-17 (Mac main 92c57c57): "I don't like the git blame view. Can we do something closer to
jetbrains with a column getting added before the line." Today blame is the caret row's after-line text
(`Annotation`, crates/fe-workbench/src/git_blame.rs:121, a `DecorationKind::AfterLine`) with a popover
(`EditorView::set_after_line_popover`, crates/fe-gpui/src/hover.rs:414). JetBrains' "Annotate with Git
Blame" instead adds a column to the gutter with every row's commit date and author, tinted by age.

The editor has no way to show per-row host text in the gutter. The gutter is laid out in
crates/fe-display/src/layout.rs: `gutter_width` (:525: diagnostics 2 em, digits, gap, fold column, change
lane), `wrap_width` (:533), and `layout_frame_with_inlay` (:593), which puts the text at
`text_left = gutter_width(..)` (:622). crates/fe-gpui/src/element.rs paints line numbers, marks, gutter
decorations and chevrons (~:1202-1219) and registers the fold column's hitbox (:825); a click in the gutter
goes through `EditorView::on_mouse_down` (crates/fe-gpui/src/view.rs:641: `toggle_fold_at`,
`click_change_bar`, then caret placement).

This card is the generic half: an annotation column API on the editor view. The blame column that uses it
is the next card.

T795 (active) adds a gutter-less layout entry point to layout.rs and `EditorView::set_gutter` in view.rs
and element.rs. This card starts after it lands and builds on its entry point and wrap width.

## Change

1. **fe-display** (crates/fe-display/src/layout.rs). Beside T795's gutter option, add a leading annotation
   column `annotation_width: f32` (0 = none) that its entry point and its gutter-aware wrap width take. Do
   not change `FrameInput`'s fields or any existing signature: fe-display's tests build `FrameInput`
   literally. With a width `w > 0` the frame starts with the column at x 0..w. The diagnostics marks,
   line numbers, fold column, change lane and `text_left` all move right by exactly `w`, and the wrap width
   is `w` less (still whole ems). `Frame` gains `annotation_column: Rect` (the column, the frame's height;
   zero width when none). With `w = 0` every frame equals today's.

2. **fe-gpui types**: a new module crates/fe-gpui/src/gutter_annotations.rs, declared and re-exported from
   crates/fe-gpui/src/lib.rs:
   ```rust
   pub struct GutterAnnotation {
       /// The rows' bytes, anchored in the buffer at `GutterAnnotations::version`.
       pub rows: Range<fe_buffer::Anchor>,
       pub text: SharedString,
       /// 0.0 oldest to 1.0 newest; None paints no tint.
       pub heat: Option<f32>,
   }
   pub struct GutterAnnotations {
       pub version: fe_buffer::Version,
       pub entries: Vec<GutterAnnotation>,
       /// The popover for entry `ix`, hovered at buffer row `row`.
       pub hover: Rc<dyn Fn(usize, u32) -> Option<HoverContent>>,
       /// Entry `ix` clicked at buffer row `row`.
       pub on_click: Rc<dyn Fn(usize, u32, &mut Window, &mut App)>,
   }
   ```
   `EditorView::set_gutter_annotations(&mut self, Option<GutterAnnotations>, cx)` sets them or takes them
   away. `EditorView::gutter_annotations(&self) -> Option<&GutterAnnotations>` reads them.
   `EditorView::annotation_bounds(&self, buffer_row: u32) -> Option<Bounds<Pixels>>` gives the window bounds
   of that row's slot in the column in the last frame, so hosts' tests can point at it.

3. **Width**: the widest entry's shaped text (the editor's font and size) plus 1 em (0.5 em each side),
   capped at 32 em. Text past the cap is clipped to the column. Measure on entry changes, not every frame.

4. **Painting** (crates/fe-gpui/src/element.rs; put the column's prepaint and paint in a new
   crates/fe-gpui/src/element/annotation_column.rs, declared from element.rs):
   - An entry's text is painted once, on the first display row of the first buffer row of its run, in
     `theme.gutter_foreground`. The rows below it in the run, and soft-wrapped continuation rows, are blank.
   - Every display row of an entry with `heat: Some(h)` gets a full-row-height quad across the column in
     `theme.gutter_active_foreground.opacity(0.04 + 0.16 * h)`. No new theme token.
   - A row in no entry is blank and untinted.
   - Rows resolve from the anchors each frame. A row touched by an edit since `version` (any
     `BufferSnapshot::edits_since(version)` new range on it) paints no text or tint, until the host gives
     new annotations. Rows after it move with the anchors.
   - Record what was painted for tests, as `painted_popover_blocks` does: `#[cfg(test)]` per painted row,
     `(buffer_row, Option<text>, Option<Hsla>)`.

5. **Pointer**: a hitbox over the column.
   - The pointer resting over an entry's row (any row of the run, after `HOVER_DELAY`) shows `hover(ix, row)`
     in the editor's popover below that row. This works as `hover_after_line` (crates/fe-gpui/src/hover.rs:366)
     shows the host's after-line popover. Moving off the entry hides it.
   - The pointing-hand cursor shows over entries.
   - A left click with no modifiers on an entry's row calls `on_click(ix, row, ..)` from
     `EditorView::on_mouse_down`, before `toggle_fold_at`. It doesn't move the caret, select, drag or toggle a
     fold. A click on a blank row of the column does nothing.

6. **Screenshot golden**: a scene `annotated_gutter` in crates/fe-gpui/examples/screenshots/scenes.rs, with
   its golden crates/fe-gpui/examples/screenshots/golden/annotated_gutter.png made on the Mac with
   `UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots`. The scene is the highlighted Rust
   sample with three entries: `2026-10-11 Ann Example` over rows 0-2 at heat 1.0,
   `2025-03-02 Bob Example` over rows 3-4 at heat 0.0, then one row with no entry, then the first entry's
   commit again. The `Scene` struct gains a field for it; the other scenes pass none and keep their goldens
   unchanged.

Tests:
- crates/fe-display/src/layout/annotation_column_tests.rs, declared in the existing
  crates/fe-display/src/layout/fold_gutter_tests.rs by
  `#[path = "annotation_column_tests.rs"] mod annotation_column_tests;`, using `super::tests`' `Mono`
  measurer.
- crates/fe-gpui/src/annotation_column_tests.rs, declared in the existing
  crates/fe-gpui/src/fold_gutter_tests.rs by `#[path = "annotation_column_tests.rs"] mod annotation_column_tests;`,
  in a headless window as its parent's tests are.
