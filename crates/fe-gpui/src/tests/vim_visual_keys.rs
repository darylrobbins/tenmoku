//! Vim's visual modes in the editor: the arrows move the vim cursor, and
//! the block paints on it.

use std::ops::Range;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext, px};

use crate::{EditorTheme, EditorView, init, set_vim_enabled};

fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    caret: usize,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(caret);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

/// The primary selection's range and whether its head is at the start.
fn selection(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Range<usize>, bool) {
    view.read_with(cx, |view, _| {
        let selection = view.model().primary_selection();
        (selection.range, selection.reversed)
    })
}

fn mode(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.vim_mode())
}

fn block(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.block_cursor())
}

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| view.painted_carets.clone())
}

fn bar_at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        view.layout.as_mut().unwrap().caret_bounds(offset).unwrap()
    })
}

#[gpui::test]
fn arrows_extend_the_visual_selection(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "abcdef\nxyz", 3);
    cx.simulate_keystrokes("v");
    assert_eq!(selection(&view, cx), (3..4, false));
    cx.simulate_keystrokes("left left left");
    assert_eq!(selection(&view, cx), (0..4, true));
    assert_eq!(mode(&view, cx), Some("visual"));
    cx.simulate_keystrokes("left");
    assert_eq!(selection(&view, cx), (0..4, true));
    cx.simulate_keystrokes("right right right right");
    assert_eq!(selection(&view, cx), (3..5, false));
    assert_eq!(mode(&view, cx), Some("visual"));
}

#[gpui::test]
fn visual_modes_paint_a_block_on_the_vim_cursor(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "abcdef\nxyz", 3);

    cx.simulate_keystrokes("v");
    assert!(block(&view, cx));
    cx.simulate_keystrokes("escape V");
    assert!(block(&view, cx));
    cx.simulate_keystrokes("escape");

    // The block runs from the vim cursor (byte 4) to the next grapheme,
    // not from the head (byte 5).
    cx.simulate_keystrokes("v l");
    let (at_4, at_5) = (bar_at(&view, 4, cx), bar_at(&view, 5, cx));
    let blocks = painted(&view, cx);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].origin.x, at_4.origin.x);
    assert_eq!(blocks[0].size.width, at_5.origin.x - at_4.origin.x);
    cx.simulate_keystrokes("escape");

    // At the row's end: one space wide.
    cx.simulate_keystrokes("v $");
    let end = bar_at(&view, 6, cx);
    let blocks = painted(&view, cx);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].origin.x, end.origin.x);
    let space = cx.update(|_, cx| {
        let theme = view.read(cx).theme.clone();
        let text_system = cx.text_system();
        let font = text_system.resolve_font(&gpui::font(theme.font_family));
        text_system.layout_width(font, theme.font_size, ' ')
    });
    assert!(
        (blocks[0].size.width - space).abs() < px(0.01),
        "{blocks:?}"
    );
    assert!(space > px(2.));
    cx.simulate_keystrokes("escape");

    // Insert mode, and vim off: the bar.
    cx.simulate_keystrokes("i");
    assert!(!block(&view, cx));
    assert_eq!(painted(&view, cx)[0].size.width, px(2.));
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    assert!(!block(&view, cx));
    assert_eq!(painted(&view, cx)[0].size.width, px(2.));
}
