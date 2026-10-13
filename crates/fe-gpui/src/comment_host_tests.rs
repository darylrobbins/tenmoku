use super::*;
use crate::tests::{editor, state};
use gpui::TestAppContext;

/// Runs `editor.toggle_comment` in a view of `language` showing `marked`,
/// and returns what it shows.
fn toggled(cx: &mut TestAppContext, language: Option<&str>, marked: &str) -> String {
    let (view, cx) = editor(cx, marked);
    view.update(cx, |view, cx| view.set_language(language, cx));
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        let invocation = Invocation::of("editor.toggle_comment").unwrap();
        view.run_command(invocation, None, cx);
    });
    state(&view, cx)
}

#[gpui::test]
fn tokens_follow_the_language(cx: &mut TestAppContext) {
    assert_eq!(toggled(cx, Some("python"), "|x = 1"), "# |x = 1");
    assert_eq!(toggled(cx, Some("rust"), "|let x = 1;"), "// |let x = 1;");
    assert_eq!(toggled(cx, None, "|x = 1"), "|x = 1");
}

#[gpui::test]
fn toggles_back_before_the_next_frame(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "|let x = 1;");
    view.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    cx.run_until_parked();
    // The second toggle runs before a frame brings the syntax up to the
    // first's edit.
    view.update(cx, |view, cx| {
        for _ in 0..2 {
            let invocation = Invocation::of("editor.toggle_comment").unwrap();
            view.run_command(invocation, None, cx);
        }
    });
    assert_eq!(state(&view, cx), "|let x = 1;");
}
