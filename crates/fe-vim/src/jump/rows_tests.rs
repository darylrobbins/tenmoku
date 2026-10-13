//! The rows a jump labels in a view ([`view_rows`]): as many display rows
//! as the view has from its top row, a fold one row.

use fe_buffer::Buffer;
use fe_editor::{EditorModel, commands::Viewport};

use super::view_rows;

fn model(rows: usize) -> EditorModel {
    EditorModel::new(Buffer::new(&vec!["ab"; rows].join("\n")), None)
}

fn view(top_row: u32, rows: u32) -> Viewport {
    Viewport { top_row, rows }
}

#[test]
fn whole_rows_from_the_top() {
    let model = model(40);
    assert_eq!(view_rows(&model, view(0, 10), None), 0..10);
    assert_eq!(view_rows(&model, view(5, 10), None), 5..15);
    assert_eq!(view_rows(&model, view(35, 10), None), 35..40);
    assert_eq!(view_rows(&model, view(5, 0), None), 5..5);
    assert_eq!(view_rows(&model, view(50, 10), None), 40..40);
}

#[test]
fn a_fold_takes_one_row() {
    let mut model = model(40);
    // Rows 4 to 12 behind row 3.
    model.fold_rows(3..=12);
    assert_eq!(view_rows(&model, view(0, 10), None), 0..19);
    // A fold past the view takes nothing from it.
    assert_eq!(view_rows(&model, view(20, 10), None), 20..30);
}

#[test]
fn from_partway_down_a_wrapped_row() {
    // Row 0 wraps to three display rows, ten chars wide.
    let text = format!("{}\n{}", "abcdefghij".repeat(3), vec!["ab"; 20].join("\n"));
    let mut model = EditorModel::new(Buffer::new(&text), None);
    model.set_soft_wrap(true);
    model.set_wrap_width(10.);
    while model.wrap_pending(0) {}
    let starts: Vec<u32> = (0..5)
        .map(|row| model.display().row(row).buffer_row)
        .collect();
    assert_eq!(starts, [0, 0, 0, 1, 2]);
    // From row 0's first display row, five reach row 2; from its third,
    // row 4.
    assert_eq!(view_rows(&model, view(0, 5), None), 0..3);
    assert_eq!(view_rows(&model, view(0, 5), Some(0)), 0..3);
    assert_eq!(view_rows(&model, view(0, 5), Some(2)), 0..5);
    // A display row not on the top row is taken as its first.
    assert_eq!(view_rows(&model, view(0, 5), Some(4)), 0..3);
}
