//! Scrolling to a fractional buffer line ([`EditorView::scroll_to_buffer_line`]),
//! as a Markdown preview puts its source: within a soft-wrapped last row too.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{EditorTheme, EditorView, init};

/// A focused editor showing `text`, soft wrap on, every row wrapped.
fn wrapped<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.simulate_keystrokes("alt-z");
    let pending = |cx: &mut VisualTestContext| {
        view.update(cx, |view, _| view.model().display().wrap_is_pending())
    };
    while pending(cx) {
        draw(cx);
    }
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn top_line(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| view.top_buffer_line())
        .expect("drawn")
}

/// A document that is one long soft-wrapped paragraph, with no line break
/// after it, scrolls through its wrapped rows rather than back to its top.
#[gpui::test]
fn scrolls_within_a_wrapped_last_row(cx: &mut TestAppContext) {
    let (view, cx) = wrapped(cx, &"word ".repeat(4000));
    let rows = view.update(cx, |view, _| view.model().display().row_count());
    assert!(rows > 50, "wrapped into {rows} rows");

    view.update(cx, |view, cx| view.scroll_to_buffer_line(0.5, cx));
    draw(cx);
    let top = top_line(&view, cx);
    assert!((top - 0.5).abs() < 0.05, "top at {top}");

    // Past the end, it stops where the frame's clamp lets it.
    view.update(cx, |view, cx| view.scroll_to_buffer_line(3., cx));
    draw(cx);
    let end = top_line(&view, cx);
    assert!(end > 0.9 && end <= 1., "top at {end}");
}
