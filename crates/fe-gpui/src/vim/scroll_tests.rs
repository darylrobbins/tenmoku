//! Vim's scrolling keys through the editor's view: it follows `ctrl-d`,
//! `zt` and the rest, and autoscroll doesn't undo them.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, px, size};

use super::*;
use crate::{EditorTheme, EditorView, init};

/// An editor of `rows` rows, 10 of them tall, drawn, with vim `on`.
pub(super) fn editor(
    cx: &mut TestAppContext,
    rows: u32,
    on: bool,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let text = (0..rows)
        .map(|row| format!("row {row}"))
        .collect::<Vec<_>>()
        .join("\n");
    editor_of(cx, text, on)
}

/// An editor of `text`, 10 rows tall, drawn, with vim `on`.
fn editor_of(
    cx: &mut TestAppContext,
    text: String,
    on: bool,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, on));
    cx.run_until_parked();
    let line_height = draw(&view, cx).1;
    cx.simulate_resize(size(px(600.), px(line_height * 10.)));
    assert_eq!(draw(&view, cx).2, 10);
    (view, cx)
}

/// Draws a frame: the first visible row, the line height and the rows that fit.
pub(super) fn draw(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, f32, u32) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().unwrap().frame;
        (frame.first_row, frame.line_height, frame.visible_rows())
    })
}

fn top(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    draw(view, cx).0
}

pub(super) fn caret_row(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    view.read_with(cx, |view, _| {
        let model = view.model();
        model
            .text()
            .offset_to_point(model.primary_selection().head())
            .row
    })
}

pub(super) fn go_to_row(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let offset = view
            .model()
            .text()
            .point_to_offset(fe_buffer::Point::new(row, 0));
        view.model_mut().set_cursor(offset);
        view.changed(cx);
    });
    draw(view, cx);
}

#[gpui::test]
fn the_view_follows(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, 100, true);
    cx.simulate_keystrokes("ctrl-d");
    assert_eq!((top(&view, cx), caret_row(&view, cx)), (5, 5));

    go_to_row(&view, 30, cx);
    cx.simulate_keystrokes("z t");
    assert_eq!(top(&view, cx), 30);
    // Autoscroll stays off for later frames until the caret moves.
    assert_eq!(top(&view, cx), 30);
    assert_eq!(caret_row(&view, cx), 30);

    cx.simulate_keystrokes("z b");
    assert_eq!(top(&view, cx), 21);

    cx.simulate_keystrokes("ctrl-e");
    assert_eq!(top(&view, cx), 22);

    // With vim off ctrl-d is the default keymap's, which doesn't scroll.
    let (view, cx) = editor(cx, 100, false);
    cx.simulate_keystrokes("ctrl-d");
    assert_eq!((top(&view, cx), caret_row(&view, cx)), (0, 0));
}

#[gpui::test]
fn scrolls_before_a_draw_add_up(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, 100, true);
    cx.simulate_keystrokes("ctrl-d ctrl-d");
    assert_eq!((top(&view, cx), caret_row(&view, cx)), (10, 10));
    cx.simulate_keystrokes("ctrl-e ctrl-e");
    assert_eq!(top(&view, cx), 12);
}

#[gpui::test]
fn a_scroll_that_keeps_the_top_keeps_the_view(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, 100, true);
    go_to_row(&view, 30, cx);
    cx.simulate_keystrokes("z t");
    cx.simulate_keystrokes("5 j");
    assert_eq!((top(&view, cx), caret_row(&view, cx)), (30, 35));
    // The count moves the caret to row 30, where the top already is.
    cx.simulate_keystrokes("3 1 z t");
    assert_eq!((top(&view, cx), caret_row(&view, cx)), (30, 30));
}

#[gpui::test]
fn folded_rows_count_as_one(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, 100, true);
    view.update(cx, |view, _| {
        view.model_mut().fold_rows(10..=19);
    });
    go_to_row(&view, 30, cx);
    cx.simulate_keystrokes("z t");
    let top_buffer_row = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        let first = top(view, cx);
        view.read_with(cx, |view, _| view.model().display().row(first).buffer_row)
    };
    assert_eq!(top_buffer_row(&view, cx), 30);
    assert_eq!(caret_row(&view, cx), 30);
    cx.simulate_keystrokes("z b");
    // Nine display rows of rows 21..=29 and the caret's, 30 above.
    assert_eq!(top_buffer_row(&view, cx), 21);
}

#[gpui::test]
fn an_unchanged_top_keeps_a_view_inside_a_wrapped_row(cx: &mut TestAppContext) {
    let long = "word ".repeat(2000);
    let (view, cx) = editor_of(cx, format!("{long}\nlast"), true);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(true));
    let line_height = draw(&view, cx).1;
    view.update(cx, |view, _| view.scroll.y = px(line_height * 3.));
    draw(&view, cx);
    let scrolled = view.read_with(cx, |view, _| view.scroll.y);
    // The top buffer row is 0 and stays, so ctrl-y asks for no new top.
    cx.simulate_keystrokes("ctrl-y");
    draw(&view, cx);
    assert_eq!(view.read_with(cx, |view, _| view.scroll.y), scrolled);
}
