//! The system clipboard from vim: `"+` and `"*` always, and unnamed
//! registers with `vim.clipboard = "unnamedplus"`.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
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

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

#[gpui::test]
fn system_clipboard_in_the_editor(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one\ntwo");

    cx.simulate_keystrokes("\" + y y");
    assert_eq!(clipboard(cx).as_deref(), Some("one\n"));

    cx.write_to_clipboard(gpui::ClipboardItem::new_string("from outside\n".into()));
    cx.simulate_keystrokes("\" * p");
    assert_eq!(text(&view, cx), "one\nfrom outside\ntwo");

    // unnamedplus: yy fills the clipboard, cmd-v in insert mode pastes it.
    cx.update(|_, cx| apply_vim_reload(cx, (true, true), (true, false)));
    cx.simulate_keystrokes("g g j j y y");
    assert_eq!(clipboard(cx).as_deref(), Some("two\n"));
    cx.simulate_keystrokes("g g i cmd-v");
    assert_eq!(text(&view, cx), "two\none\nfrom outside\ntwo");
    cx.simulate_keystrokes("escape");

    // Back to "": yy leaves the clipboard alone.
    cx.update(|_, cx| apply_vim_reload(cx, (true, true), (false, true)));
    cx.simulate_keystrokes("g g j y y");
    assert_eq!(clipboard(cx).as_deref(), Some("two\n"));
}
