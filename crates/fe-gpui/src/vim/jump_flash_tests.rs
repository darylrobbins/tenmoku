//! `vim.jump_style` reaching the editors' vim state: in the flash style
//! the overlay shows after `{c1}`, and a reload switches the style live.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, init};

/// The caret on `C`, the `a`s around it.
const TEXT: &str = "a b a\nb a C a b\na b a";

fn open(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(TEXT), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx)
}

/// Types `g s a` from the `C`, draws a frame, and reads the labels the
/// frame laid out (their text, in text order), the query and whether a
/// backdrop shows; then `escape` cancels the jump.
fn after_one_char(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Vec<String>, Option<String>, bool) {
    cx.simulate_keystrokes("g g j w w");
    cx.simulate_keystrokes("g s a");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let seen = view.read_with(cx, |view, _| {
        let jump = view.layout.as_ref().unwrap().jump.as_ref();
        let labels = jump.map(|jump| jump.labels.iter().map(|label| label.text.to_string()));
        (
            labels.into_iter().flatten().collect(),
            view.vim_state().unwrap().jump_query().map(str::to_string),
            jump.is_some_and(|jump| jump.backdrop.is_some()),
        )
    });
    cx.simulate_keystrokes("escape");
    seen
}

fn flashed() -> (Vec<String>, Option<String>, bool) {
    let labels = ["l", "j", "f", "s", "k", "n"];
    (labels.map(String::from).to_vec(), Some("a".into()), true)
}

fn leapt() -> (Vec<String>, Option<String>, bool) {
    (Vec::new(), Some("a".into()), true)
}

#[gpui::test]
fn overlay_after_one_char(cx: &mut TestAppContext) {
    cx.update(init);
    let (first, cx) = open(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    // Leap by default: `{c1}` alone labels nothing.
    assert_eq!(after_one_char(&first, cx), leapt());

    // A reload to flash reaches the open editor: labels on every `a` in
    // view, both ways, nearest first.
    cx.update(|_, cx| set_vim_jump_style(cx, VimJumpStyle::Flash));
    assert_eq!(after_one_char(&first, cx), flashed());
    // A label jumps.
    cx.simulate_keystrokes("g s a j");
    first.read_with(cx, |view, _| {
        let head = view.model().primary_selection().head();
        let point = view.model().text().offset_to_point(head);
        assert_eq!((point.row, point.column), (0, 4));
        assert_eq!(view.vim_state().unwrap().jump_query(), None);
    });

    // And back to leap, live.
    cx.update(|_, cx| set_vim_jump_style(cx, VimJumpStyle::Leap));
    assert_eq!(after_one_char(&first, cx), leapt());

    // An editor opened afterwards gets the value too (keys go to the newest
    // window).
    cx.update(|_, cx| set_vim_jump_style(cx, VimJumpStyle::Flash));
    let (second, cx) = open(cx);
    assert_eq!(after_one_char(&second, cx), flashed());
}
