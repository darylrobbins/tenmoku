use crate::tests::{editor, state};
use gpui::TestAppContext;

/// Types `text` a char at a time, as keys do, into a view of `language`
/// showing `marked`, and returns what it shows.
fn typed(cx: &mut TestAppContext, language: Option<&str>, marked: &str, text: &str) -> String {
    let (view, cx) = editor(cx, marked);
    view.update(cx, |view, cx| view.set_language(language, cx));
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        for ch in text.chars() {
            view.type_text(&ch.to_string(), cx);
        }
    });
    state(&view, cx)
}

#[gpui::test]
fn per_language(cx: &mut TestAppContext) {
    // Rust has lifetimes and chars, so `'` isn't paired; brackets and `"` are.
    assert_eq!(typed(cx, Some("rust"), "fn f|", "<'"), "fn f<'|");
    assert_eq!(typed(cx, Some("rust"), "f|", "("), "f(|)");
    assert_eq!(
        typed(cx, Some("rust"), "let s = |;", "\"a\""),
        "let s = \"a\"|;"
    );
    // Python pairs `'` too.
    assert_eq!(typed(cx, Some("python"), "x = |", "'"), "x = '|'");
    assert_eq!(typed(cx, Some("python"), "x = |", "'a'"), "x = 'a'|");
    // Without a language loaded, nothing is paired.
    assert_eq!(typed(cx, None, "|", "(\""), "(\"|");

    // Typed twice before a frame, both see the language.
    let (view, cx) = editor(cx, "f|");
    view.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.type_text("(", cx);
        view.type_text("[", cx);
    });
    assert_eq!(state(&view, cx), "f([|])");

    // Turned off in the model (as the workbench's `editor.auto_close_pairs`
    // does), typing is as typed.
    view.update(cx, |view, cx| {
        view.model_mut().set_auto_close(false);
        view.type_text("{", cx);
    });
    assert_eq!(state(&view, cx), "f([{|])");
}
