//! Rulers drawn in a headless window: a line at each column, a column's
//! monospace advances right of column 0, moving with horizontal scroll.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

use super::*;
use crate::{EditorSettings, EditorTheme, EditorView, init};

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let text = format!("{}\nshort\n", "x".repeat(400));
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Sets `editor.rulers`, small enough that 120 columns fit, as a settings
/// reload does, and draws.
fn set(rulers: &[i32], view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let settings = EditorSettings {
        font_size: 8.,
        rulers: rulers.to_vec(),
        ..EditorSettings::default()
    };
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    draw(cx);
}

/// The last frame's rulers as laid out, after checking it painted just
/// those.
fn rulers(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    let placed = view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").rulers.clone()
    });
    assert_eq!(PAINTED.with(|painted| painted.borrow().clone()), placed);
    placed
}

/// A ruler at `column` in the last frame: down the whole text area.
fn expected(view: &Entity<EditorView>, column: u32, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let scale = cx.update(|window, _| window.scale_factor());
    view.update(cx, |view, _| {
        let layout = view.layout.as_mut().expect("drawn");
        let space = f64::from(layout.glyph_widths.char_width(' '));
        let frame = &layout.frame;
        let text = frame.text_bounds();
        layout.bounds(Rect {
            x: frame.view_x(f64::from(column) * space),
            y: text.y,
            width: (1. * scale).round().max(1.) / scale,
            height: text.height,
        })
    })
}

#[gpui::test]
fn columns(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    set(&[80, 120], &view, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.theme.rulers.clone()),
        [80, 120]
    );
    let placed = rulers(&view, cx);
    assert_eq!(placed, [expected(&view, 80, cx), expected(&view, 120, cx)]);
    // 80 and 120 advances right of column 0, which is the text area's left
    // edge unscrolled.
    let (left, space) = view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let left = layout.bounds(layout.frame.text_bounds()).origin.x;
        (left, layout.space_width())
    });
    assert_eq!(placed[0].origin.x - left, space * 80.);
    assert!((placed[1].origin.x - left - space * 120.).abs() < px(0.01));
    let height = view.read_with(cx, |view, _| view.layout.as_ref().unwrap().frame.height);
    assert_eq!(placed[0].size.height, px(height));

    // Scrolled right by ten columns, they move left by as much.
    view.update(cx, |view, cx| {
        view.scroll.x = space * 10.;
        cx.notify();
    });
    draw(cx);
    let scrolled = rulers(&view, cx);
    assert_eq!(
        scrolled,
        [expected(&view, 80, cx), expected(&view, 120, cx)]
    );
    for (scrolled, placed) in scrolled.iter().zip(&placed) {
        assert!((placed.origin.x - scrolled.origin.x - space * 10.).abs() < px(0.01));
    }

    // Columns of 0 or less are ignored, and none is no rulers.
    set(&[0, -4], &view, cx);
    assert_eq!(rulers(&view, cx), []);
    set(&[], &view, cx);
    assert_eq!(rulers(&view, cx), []);
}
