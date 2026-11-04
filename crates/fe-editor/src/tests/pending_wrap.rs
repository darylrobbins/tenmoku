use std::{cell::Cell, rc::Rc};

use fe_buffer::Buffer;
use fe_display::{CharWidth, DisplayMap, Monospace, WRAP_SLICE_BYTES};

use crate::*;

const LINES: u32 = 20_000;
const WIDTH: f32 = 10.;

/// Lines of varying length, so wrapped rows differ from one to the next.
fn text() -> String {
    (0..LINES)
        .map(|row| format!("{}\n", "the quick brown fox ".repeat(1 + row as usize % 5)))
        .collect()
}

/// Wrapping `text` takes more than a few slices.
fn assert_many_slices(text: &str) {
    assert!(text.len() > 4 * WRAP_SLICE_BYTES, "{} bytes", text.len());
}

#[test]
fn toggle_wraps_the_cursor_rows_first() {
    let text = text();
    assert_many_slices(&text);
    let mut model = EditorModel::new(Buffer::new(&text), None);
    model.set_wrap_width(WIDTH);
    let mid = model.text().row_start(LINES / 2);
    model.set_selections(vec![caret(mid)]);
    model.toggle_soft_wrap();

    // One slice, around the cursor: its rows are wrapped, the rest pending.
    assert!(model.display().wrap_is_pending());
    let cursor_row = model.display().row_for_buffer_row(LINES / 2);
    assert!(!model.display().row(cursor_row).last, "cursor row wrapped");
    assert!(model.display().row(0).last, "first row still pending");
    assert!(model.display().row_count() < 2 * LINES, "most rows pending");

    // Further slices finish, and match wrapping everything in one go.
    while model.wrap_pending(100_000) {}
    assert_wrapped_as_in_one_go(&model, WIDTH, model.display().row_for_buffer_row(LINES / 2));
}

/// The model's rows are those of a display map wrapped at `width` in one go.
/// Samples every 11th row, plus 200 rows around the cursor, around `top`
/// (the viewport top) and at the tail.
fn assert_wrapped_as_in_one_go(model: &EditorModel, width: f32, top: u32) {
    let mut reference = DisplayMap::new(model.snapshot(), model.tab_size());
    reference.set_wrap_width(Some(width), &mut Monospace);
    assert!(!reference.wrap_pending(0, usize::MAX, &mut Monospace));
    assert_eq!(model.display().row_count(), reference.row_count());
    let count = reference.row_count();
    let cursor = model.display().row_for_offset(model.selections()[0].head());
    let around = |centre: u32| centre.saturating_sub(100)..(centre + 100).min(count);
    let rows = (0..count)
        .step_by(11)
        .chain(around(cursor))
        .chain(around(top))
        .chain(count.saturating_sub(200)..count);
    for row in rows {
        assert_eq!(model.display().row(row), reference.row(row), "row {row}");
    }
}

/// Wraps through to the end from a viewport whose top is `top`, checking
/// after every slice that the top row still shows `anchor`. Returns the top.
fn wrap_keeping(model: &mut EditorModel, mut top: f32, anchor: usize) -> f32 {
    let fraction = top.fract();
    loop {
        let (pending, next) = model.wrap_pending_at(top);
        top = next;
        assert_eq!(top.fract(), fraction, "same height within the row");
        let row = model.display().row(top as u32);
        assert!(row.range.contains(&anchor), "{anchor} not in {row:?}");
        if !pending {
            return top;
        }
    }
}

#[test]
fn narrowing_keeps_the_top_row_in_place() {
    let text = text();
    assert_many_slices(&text);
    let mut model = EditorModel::new(Buffer::new(&text), None);
    let anchor = model.text().row_start(LINES * 3 / 4);
    model.set_soft_wrap(true);
    while model.wrap_pending(0) {}
    // Halfway into the top row, the cursor far above.
    let top = model.display().row_for_offset(anchor) as f32 + 0.5;
    assert!(model.fit_wrap_width(WIDTH));
    let top = wrap_keeping(&mut model, top, anchor);
    assert_wrapped_as_in_one_go(&model, WIDTH, top as u32);
}

#[test]
fn widening_from_a_continuation_row_keeps_its_text_in_place() {
    // A row wrapped into many segments at the narrow width, and two at
    // the wide one, in the middle of the file.
    let half = text();
    assert_many_slices(&half);
    let long_start = half.len();
    let text = format!("{half}{}\n{half}", "0123456789 ".repeat(100));
    let mut model = EditorModel::new(Buffer::new(&text), None);
    model.set_wrap_width(WIDTH);
    model.set_soft_wrap(true);
    while model.wrap_pending(0) {}

    // The top row is the long row's ninth segment.
    let first = model.display().row_for_offset(long_start);
    let top_row = first + 8;
    let segment = model.display().row(top_row);
    assert!(!segment.first && !segment.last, "{segment:?}");
    let anchor = segment.range.start;

    let wide = 600.;
    assert!(model.fit_wrap_width(wide));
    let top = wrap_keeping(&mut model, top_row as f32 + 0.25, anchor);
    assert_wrapped_as_in_one_go(&model, wide, top as u32);
    let first_now = model.display().row_for_offset(long_start);
    assert!(
        model.display().row(first_now + 1).last,
        "the long row wraps into two segments"
    );
    assert!((top as u32) <= first_now + 1);
}

#[test]
fn width_changes_wrap_nothing_until_asked() {
    let text = text();
    let mut model = EditorModel::new(Buffer::new(&text), None);
    model.set_soft_wrap(true);
    while model.wrap_pending(0) {}
    let rows = model.display().row_count();
    assert!(model.fit_wrap_width(WIDTH));
    model.sync_display();
    assert!(model.display().wrap_is_pending());
    assert_eq!(model.display().row_count(), rows, "no row re-wrapped yet");
}

/// [`Monospace`], counting the bytes of text it measures.
struct Counting(Rc<Cell<usize>>);

impl CharWidth for Counting {
    fn char_width(&mut self, ch: char) -> f32 {
        self.0.set(self.0.get() + ch.len_utf8());
        1.
    }
}

/// 120k rows of 1 to 8 non-ASCII chars, which the wrap measures one by one
/// (ASCII widths are cached).
fn non_ascii_text() -> String {
    (0..120_000)
        .map(|row| format!("{}\n", "é".repeat(1 + row % 8)))
        .collect()
}

#[test]
fn every_slice_measures_at_most_a_slice_of_text() {
    let text = non_ascii_text();
    assert_many_slices(&text);
    // A slice stops at the first row that reaches its budget.
    let longest_row = "é".repeat(8).len();
    let measured = Rc::new(Cell::new(0));
    let mut model = EditorModel::new(Buffer::new(&text), None);
    model.set_char_widths(Box::new(Counting(measured.clone())));
    model.set_wrap_width(4.);

    // Turning wrap on, then widening by a column, each wrapped to the end.
    for widen in [false, true] {
        if widen {
            model.set_wrap_width(5.);
        } else {
            model.toggle_soft_wrap();
        }
        let mut slices = 0;
        loop {
            measured.set(0);
            let pending = model.wrap_pending(500_000);
            let bytes = measured.get();
            assert!(bytes <= WRAP_SLICE_BYTES + longest_row, "{bytes} bytes");
            assert!(bytes > 0, "a slice measures something");
            slices += 1;
            if !pending {
                break;
            }
        }
        // About 1 MB, wrapped across that many slices.
        assert!(slices >= text.len() / WRAP_SLICE_BYTES, "{slices} slices");
    }
}
