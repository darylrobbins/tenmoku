//! A ghost row of one grapheme cluster longer than a row is shaped at once
//! is painted in bounded pieces.

use fe_display::layout::MAX_SHAPED_ROW_BYTES;
use gpui::TestAppContext;

use super::{draw, editor, scroll_right, suggest, texts};

#[gpui::test]
fn oversized_cluster_is_shaped_in_bounded_pieces(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    draw(cx);
    let row = format!("a{}tail!", "\u{301}".repeat(10_000));
    suggest(&view, &format!("bar(\n{row}\n)"), cx);
    let (_, rows) = texts(&view, cx).expect("shown");
    assert!(
        rows[0].len() <= MAX_SHAPED_ROW_BYTES && rows[0].starts_with('a'),
        "{} bytes",
        rows[0].len()
    );

    scroll_right(&view, 10_000_000., cx);
    let (_, rows) = texts(&view, cx).expect("shown");
    assert!(
        rows[0].len() <= MAX_SHAPED_ROW_BYTES,
        "{} bytes",
        rows[0].len()
    );
}
