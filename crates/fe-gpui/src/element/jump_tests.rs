//! A pending vim jump in the editor: its labels painted over the targets on
//! a dimmed backdrop, and its query in the status bar.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{init, pane_item::status, vim::set_vim_enabled};

/// 40 rows of six `ab`s in an editor 10 rows tall, drawn, with vim on.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let text = vec!["ab ab ab ab ab ab"; 40].join("\n");
    cx.update(init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    let line_height = draw(&view, cx);
    cx.simulate_resize(size(px(600.), px(line_height * 10.)));
    draw(&view, cx);
    (view, cx)
}

/// Draws a frame; its line height.
fn draw(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    })
}

/// The vim mode item's text.
fn vim_status(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| {
        let items = crate::pane_item::status_items(view);
        let item = items.iter().find(|item| item.id == status::VIM_MODE);
        item.unwrap().text.to_string()
    })
}

#[gpui::test]
fn labels_at_targets(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    // 59 matches in view after the caret, more than the safe labels take.
    cx.simulate_keystrokes("g s a b");
    draw(&view, cx);
    view.update(cx, |view, _| {
        assert_eq!(view.model().primary_selection().head(), 0);
        let layout = view.layout.as_mut().unwrap();
        let jump = layout.jump.as_ref().expect("a jump is pending");
        let window = layout.bounds(layout.frame.text_bounds());
        assert_eq!(layout.bounds(jump.backdrop), window);
        let offsets: Vec<usize> = jump.labels.iter().map(|label| label.offset).collect();
        let row = "ab ab ab ab ab ab\n".len();
        let targets: Vec<usize> = (0..10)
            .flat_map(|r| (0..6).map(move |m| r * row + m * 3))
            .skip(1)
            .collect();
        assert_eq!(offsets, targets);
        let states = |state| jump.labels.iter().filter(|l| l.state == state).count();
        let (live, concealed) = (
            states(JumpLabelState::Live),
            states(JumpLabelState::Concealed),
        );
        assert!(
            live > 0 && concealed > 0,
            "{live} live, {concealed} concealed"
        );
        let labels: Vec<(usize, Bounds<Pixels>)> = (jump.labels.iter())
            .map(|label| (label.offset, layout.bounds(label.rect)))
            .collect();
        for (offset, label) in labels {
            let caret = layout.caret_bounds(offset).unwrap();
            assert_eq!(
                (label.origin, label.size.height),
                (caret.origin, caret.size.height)
            );
            assert!(label.size.width > px(0.));
            let inside = window.contains(&label.origin)
                && label.bottom_right().y <= window.bottom_right().y
                && label.bottom_right().x <= window.bottom_right().x;
            assert!(inside, "{label:?} outside {window:?}");
        }
    });

    cx.simulate_keystrokes("escape");
    draw(&view, cx);
    view.read_with(cx, |view, _| {
        assert!(view.layout.as_ref().unwrap().jump.is_none());
    });
}

#[gpui::test]
fn status_shows_query(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    assert_eq!(vim_status(&view, cx), "NORMAL");
    cx.simulate_keystrokes("g s");
    assert_eq!(vim_status(&view, cx), "JUMP");
    cx.simulate_keystrokes("a");
    assert_eq!(vim_status(&view, cx), "JUMP a");
    cx.simulate_keystrokes("b");
    assert_eq!(vim_status(&view, cx), "JUMP ab");
    // Picking a label ends it.
    draw(&view, cx);
    let label = view.read_with(cx, |view, _| {
        let jump = view.layout.as_ref().unwrap().jump.as_ref().unwrap();
        let live = jump.labels.iter().find(|l| l.state == JumpLabelState::Live);
        live.unwrap().text.to_string()
    });
    cx.simulate_keystrokes(&label);
    assert_eq!(vim_status(&view, cx), "NORMAL");
}
