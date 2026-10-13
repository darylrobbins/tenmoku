//! A jump in a view that starts partway down a soft-wrapped row: its labels
//! reach the last row in view, and the letters after `{c1}` are the labels
//! after `{c2}`.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Focusable as _, TestAppContext, VisualTestContext};

use super::{jump_style_tests::draw, *};
use crate::{init, vim::set_vim_enabled};

/// `text` soft-wrapped in an editor 600 wide and 10 rows tall, drawn, with
/// vim on.
fn editor(cx: &mut TestAppContext, text: String) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_soft_wrap(true);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    let line_height = draw(&view, cx);
    cx.simulate_resize(size(px(600.), px(line_height * 10.)));
    draw(&view, cx);
    (view, cx)
}

/// The jump's labels laid out: offset, text and state.
fn labels(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<(usize, String, JumpLabelState)> {
    view.read_with(cx, |view, _| {
        let jump = view.layout.as_ref().unwrap().jump.as_ref();
        (jump.expect("a jump is pending").labels.iter())
            .map(|label| (label.offset, label.text.to_string(), label.state))
            .collect()
    })
}

fn head(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model().primary_selection().head())
}

#[gpui::test]
fn labels_reach_the_last_row_from_partway_down_a_wrapped_row(cx: &mut TestAppContext) {
    // A first row that wraps to many display rows, then rows of three
    // `ac`s: from partway down the first, `ac` waits.
    let rows = std::iter::once("x".repeat(300)).chain(std::iter::repeat_n("ac ac ac".into(), 40));
    let (view, cx) = editor(cx, rows.collect::<Vec<_>>().join("\n"));
    let line_height = draw(&view, cx);
    // Down to the first row's last display row but one.
    let first = view.read_with(cx, |view, _| {
        view.model().display().row_for_buffer_row(1) - 2
    });
    assert!(first > 0);
    view.update(cx, |view, _| {
        view.scroll.y = px(line_height * first as f32);
        view.autoscroll = false;
    });
    draw(&view, cx);
    let bottom = view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().unwrap().frame;
        assert_eq!(frame.first_row, first);
        let display = view.model().display();
        let last = frame.first_row + frame.visible_rows() - 1;
        display.row(last).buffer_row
    });
    assert!(bottom > 2, "{bottom}");

    cx.simulate_keystrokes("g s a");
    draw(&view, cx);
    let letters: Vec<(usize, String)> = (labels(&view, cx).into_iter())
        .filter(|(.., state)| *state == JumpLabelState::Preview)
        .map(|(offset, text, _)| (offset, text))
        .collect();
    let rows: Vec<u32> = view.read_with(cx, |view, _| {
        let text = view.model().text();
        (letters.iter())
            .map(|&(offset, _)| text.offset_to_point(offset).row)
            .collect()
    });
    // Every whole row in view, to the last, has its targets labelled.
    assert_eq!(rows.last(), Some(&bottom), "{rows:?}");

    cx.simulate_keystrokes("c");
    draw(&view, cx);
    assert_eq!(head(&view, cx), 0);
    let live: Vec<(usize, String)> = (labels(&view, cx).into_iter())
        .filter(|(.., state)| *state == JumpLabelState::Live)
        .map(|(offset, text, _)| (offset, text))
        .collect();
    assert_eq!(live, letters);
    let (offset, letter) = letters.last().unwrap().clone();
    cx.simulate_keystrokes(&letter);
    assert_eq!(head(&view, cx), offset);
}
