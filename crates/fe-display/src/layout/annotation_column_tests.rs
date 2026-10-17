//! The annotation column left of the gutter, against the fake monospace
//! measurer of [`crate::layout::tests`].

use fe_buffer::Buffer;

use crate::layout::{
    tests::{CELL, LINE, Line, Mono, WIDTH, rect},
    *,
};

const TEXT: &str = "a {\n  x\n}\nb {\n  y\n}\nz";

/// Lays out [`TEXT`] in a 400 × 200 editor, with an annotation column
/// `annotation` wide, or through [`layout_frame_with_inlay`] for `None`.
fn lay_out(annotation: Option<f32>) -> Frame<Line> {
    lay_out_selected(annotation, WIDTH, 0..0)
}

/// [`lay_out`] in an editor `width` wide, with `selected` selected.
fn lay_out_selected(annotation: Option<f32>, width: f32, selected: Range<usize>) -> Frame<Line> {
    let buffer = Buffer::new(TEXT);
    let display = DisplayMap::new(buffer.current(), 4);
    let selections = [ResolvedSelection {
        range: selected.clone(),
        reversed: false,
    }];
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: selected.end,
        scroll: Vec2::default(),
        autoscroll: false,
        width,
        height: 200.,
        line_height: LINE,
        em: CELL,
        styles: FrameStyles {
            text: "text",
            line_number: "number",
            active_line_number: "active",
        },
    };
    let (blocks, mut long_rows) = (BlockMap::default(), LongRows::default());
    let candidates = |rows: Range<u32>| rows.filter(|row| [0, 3].contains(row)).collect();
    match annotation {
        Some(width) => layout_frame_with_annotation_column(
            &input,
            width,
            &blocks,
            None,
            &mut Mono,
            &mut long_rows,
            &mut Mono,
            |_| Vec::new(),
            candidates,
        ),
        None => layout_frame_with_inlay(
            &input,
            &blocks,
            None,
            &mut Mono,
            &mut long_rows,
            &mut Mono,
            |_| Vec::new(),
            candidates,
        ),
    }
}

fn origins(frame: &Frame<Line>) -> Vec<Vec2> {
    frame
        .line_numbers
        .iter()
        .map(|(origin, _)| *origin)
        .collect()
}

#[test]
fn a_zero_width_column_lays_out_as_before() {
    let (before, frame) = (lay_out(None), lay_out(Some(0.)));
    assert_eq!(frame.text_left, before.text_left);
    assert_eq!(origins(&frame), origins(&before));
    assert_eq!(frame.line_numbers.len(), 7);
    assert_eq!(frame.fold_column, before.fold_column);
    assert_eq!(frame.change_lane, before.change_lane);
    assert_eq!(frame.fold_toggles, before.fold_toggles);
    assert_eq!(frame.annotation_column.width, 0.);
    for width in [0., 95., 400., 1000.] {
        assert_eq!(
            wrap_width_with_annotation_column(width, 7, CELL, 0.),
            wrap_width(width, 7, CELL)
        );
    }
}

#[test]
fn the_column_comes_before_the_line_numbers() {
    let w = 120.;
    let (before, frame) = (lay_out(None), lay_out(Some(w)));
    assert_eq!(frame.annotation_column, rect(0., 0., w, 200.));
    assert_eq!(frame.text_left, before.text_left + w);
    let shifted: Vec<_> = origins(&before)
        .into_iter()
        .map(|origin| Vec2 {
            x: origin.x + w,
            ..origin
        })
        .collect();
    assert_eq!(origins(&frame), shifted);
    let right = |r: Rect| Rect { x: r.x + w, ..r };
    assert_eq!(frame.fold_column, right(before.fold_column));
    assert_eq!(frame.change_lane, right(before.change_lane));
    assert!(frame.line_numbers[0].0.x >= w);
    assert_eq!(frame.text_bounds().x, frame.text_left);
    // The wrap width is `w` less, in whole ems.
    assert_eq!(
        wrap_width_with_annotation_column(WIDTH, 7, CELL, w),
        wrap_width(WIDTH, 7, CELL) - w
    );
    let odd = wrap_width_with_annotation_column(WIDTH, 7, CELL, 125.);
    assert_eq!(odd % CELL, 0.);
    assert!(odd <= wrap_width(WIDTH, 7, CELL) - 125.);
}

#[test]
fn a_column_wider_than_the_editor_leaves_no_text_area() {
    // A selection on screen, and the column alone wider than the editor.
    let width = 30. * CELL;
    let frame = lay_out_selected(Some(32. * CELL), width, 0..8);
    assert!(frame.text_left > width);
    assert_eq!(frame.text_bounds().width, 0.);
    assert!(!frame.selections.is_empty());
    for rect in &frame.selections {
        assert_eq!(rect.width, 0., "{rect:?}");
    }
}
