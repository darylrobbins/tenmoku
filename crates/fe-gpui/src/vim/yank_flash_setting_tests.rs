//! `vim.highlight_yank` and `vim.highlight_yank_ms` shape the yank flash.

use std::time::Duration;

use fe_buffer::Buffer;
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, Tint},
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, init};

fn editor<'a>(
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
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

fn highlights(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Decoration> {
    view.read_with(cx, |view, _| {
        let len = view.model().text().len();
        view.model()
            .decorations(0..len)
            .into_iter()
            .filter(|decoration| decoration.kind == DecorationKind::Highlight)
            .collect()
    })
}

#[gpui::test]
fn the_settings_shape_the_flash(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one two");

    cx.update(|_, cx| set_vim_highlight_yank(cx, false, Duration::from_millis(150)));
    cx.simulate_keystrokes("w y i w");
    assert_eq!(highlights(&view, cx), vec![]);

    // Turning it on doesn't flash the yank made while it was off.
    cx.update(|_, cx| set_vim_highlight_yank(cx, true, Duration::from_millis(400)));
    cx.run_until_parked();
    assert_eq!(highlights(&view, cx), vec![]);

    cx.simulate_keystrokes("y i w");
    let found = highlights(&view, cx);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].range.clone(), found[0].tint), (4..7, Tint::Yank));
    cx.executor().advance_clock(Duration::from_millis(399));
    cx.run_until_parked();
    assert_eq!(highlights(&view, cx).len(), 1);
    cx.executor().advance_clock(Duration::from_millis(2));
    cx.run_until_parked();
    assert_eq!(highlights(&view, cx), vec![]);
}
