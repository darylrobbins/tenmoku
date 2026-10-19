//! Vim's `<C-n>` and `<C-p>` keyword candidates in the completion menu:
//! label-only rows with the current one selected, no question for the
//! provider, and `escape` and `enter` still vim's.

use super::*;

/// A vim view on `TEXT` after a line of words, in insert mode with the
/// caret after `va`, and the test provider offering its own items.
fn vim_editor(
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, Rc<TestProvider>, &mut VisualTestContext) {
    let (view, provider, cx) = editor(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    view.update(cx, |view, cx| {
        let model = view.model_mut();
        model.set_cursor(0);
        model.insert("value values vary\n");
        let end = model.text().to_string().find("va\n").unwrap() + 2;
        view.select_and_reveal(end..end, cx);
    });
    keys("a", cx);
    provider.calls.borrow_mut().clear();
    (view, provider, cx)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

#[gpui::test]
fn ctrl_n_shows_the_candidates(cx: &mut TestAppContext) {
    let (view, provider, cx) = vim_editor(cx);
    keys("ctrl-n", cx);
    let shown = view.read_with(cx, |view, _| view.painted_completion.clone());
    let shown = shown.expect("the menu shows");
    // Label only, the first candidate selected and in the text.
    assert_eq!(shown.rows, [("value".into(), String::new(), String::new())]);
    assert_eq!(shown.selected, Some(0));
    assert!(text(&view, cx).ends_with("    value\n}\n"));
    // Not the key context: the language-server menu's keys aren't taken.
    assert!(!view.read_with(cx, |view, _| view.completion_shown()));

    // The list grows as the search goes on, and the selection moves.
    keys("ctrl-n", cx);
    assert_eq!(
        rows(&view, cx),
        (vec!["value".into(), "values".into()], Some("values".into()))
    );
    assert!(text(&view, cx).ends_with("    values\n}\n"));

    keys("space", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.painted_completion.clone()),
        None
    );
    assert!(provider.calls.borrow().is_empty());
}

#[gpui::test]
fn menu_keys_stay_vims(cx: &mut TestAppContext) {
    let (view, provider, cx) = vim_editor(cx);
    let mode = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.vim_mode());
    let shown = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.painted_completion.is_some())
    };

    // `escape` leaves insert mode, keeping the match.
    keys("ctrl-n", cx);
    assert!(shown(cx));
    let completed = text(&view, cx);
    keys("escape", cx);
    assert_eq!(mode(cx), Some("normal"));
    assert_eq!(text(&view, cx), completed);
    assert!(!shown(cx));

    // `enter` breaks the line, as without the menu.
    keys("a ctrl-n", cx);
    assert!(shown(cx));
    keys("enter", cx);
    assert!(text(&view, cx).ends_with('\n'));
    assert_eq!(text(&view, cx).lines().count(), 5);
    assert!(!shown(cx));

    // A language-server menu closes as keyword completion starts.
    keys("ctrl-space", cx);
    assert_eq!(provider.calls.borrow().len(), 1);
    assert!(view.read_with(cx, |view, _| view.completion_shown()));
    keys("v a", cx);
    keys("ctrl-n", cx);
    assert!(!view.read_with(cx, |view, _| view.completion_shown()));
    assert!(shown(cx));
    assert_eq!(provider.calls.borrow().len(), 1);
}
