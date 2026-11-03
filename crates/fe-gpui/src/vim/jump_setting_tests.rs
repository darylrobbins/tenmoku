//! `vim.jump_on_s` reaching the editors' vim state.

use fe_buffer::Buffer;
use fe_command::ContextFrame;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

#[path = "search_setting_tests.rs"]
mod search_setting_tests;

use super::*;
use crate::{EditorTheme, EditorView, init};

pub(super) fn open<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx)
}

/// Presses `s` in normal mode at the buffer's start, reads the pending kind,
/// the mode and the text, then leaves: `escape` and `u` undo a `cl`.
fn press_s(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Option<String>, Option<&'static str>, String) {
    cx.simulate_keystrokes("s");
    let seen = view.read_with(cx, |view, _| {
        let frame = view.vim_state().unwrap().stamp(ContextFrame::new(""));
        (
            frame.values.get("vim.pending_kind").cloned(),
            view.vim_mode(),
            view.model().text().to_string(),
        )
    });
    cx.simulate_keystrokes("escape u");
    seen
}

/// `s` started a jump: text untouched, still in normal mode.
fn assert_jumps(view: &Entity<EditorView>, cx: &mut VisualTestContext, text: &str) {
    let (kind, mode, now) = press_s(view, cx);
    assert_eq!(kind.as_deref(), Some("jump"));
    assert_eq!(mode, Some("normal"));
    assert_eq!(now, text);
}

/// `s` was `cl`: the first char gone, in insert mode, no jump.
fn assert_changes(view: &Entity<EditorView>, cx: &mut VisualTestContext, text: &str) {
    let (kind, mode, now) = press_s(view, cx);
    assert_eq!(kind, None);
    assert_eq!(mode, Some("insert"));
    assert_eq!(now, &text[1..]);
    assert_eq!(text_of(view, cx), text, "undone");
}

pub(super) fn text_of(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

#[gpui::test]
fn applies_to_editors(cx: &mut TestAppContext) {
    cx.update(init);
    let (first, cx) = open(cx, "one two\nthree");
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    assert_changes(&first, cx, "one two\nthree");

    cx.update(|_, cx| set_vim_jump_on_s(cx, true));
    assert_jumps(&first, cx, "one two\nthree");
    cx.update(|_, cx| set_vim_jump_on_s(cx, false));
    assert_changes(&first, cx, "one two\nthree");

    // An editor opened afterwards gets the value too (keys go to the newest
    // window, so each editor is checked before the next opens).
    cx.update(|_, cx| set_vim_jump_on_s(cx, true));
    let (second, cx) = open(cx, "four five");
    assert_jumps(&second, cx, "four five");
    cx.update(|_, cx| set_vim_jump_on_s(cx, false));
    assert_changes(&second, cx, "four five");
}

#[path = "replace_register_setting_tests.rs"]
mod replace_register_setting_tests;
