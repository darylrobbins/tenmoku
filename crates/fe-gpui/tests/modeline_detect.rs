//! A modeline in a file's first or last lines picks the language it opens
//! in, over its path, and a language chosen by hand beats it.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorTheme, EditorView};
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_shell::PaneItem as _;

/// An editor on `text` at `path`, once its language has loaded.
fn editor<'a>(
    text: &str,
    path: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let (text, path) = (text.to_owned(), path.into());
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some(path));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    (view, cx)
}

fn language(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    cx.run_until_parked();
    view.read_with(cx, |view, _| view.syntax().map(|s| s.language().name()))
}

/// `count` lines of 63 `x`s.
fn filler(count: usize) -> String {
    format!("{}\n", "x".repeat(63)).repeat(count)
}

#[gpui::test]
fn open_uses_a_modeline_in_the_last_lines(cx: &mut TestAppContext) {
    cx.update(fe_gpui::init);
    // Far past the first bytes read, in the last five lines.
    let text = format!("{}# vim: set ft=python:\n{}", filler(40), filler(4));
    let (view, window) = editor(&text, "build.rs", cx);
    assert_eq!(language(&view, window), Some("python"));
    view.read_with(window, |view, _| assert!(!view.is_language_chosen()));

    // Without it, the path decides.
    let text = format!("{}# no modeline\n{}", filler(40), filler(4));
    let (view, window) = editor(&text, "build.rs", cx);
    assert_eq!(language(&view, window), Some("rust"));

    // The end is read from a line start: the cut in the last kilobyte that
    // splits `nvim:` after its `n` doesn't make it a modeline.
    let rest = format!("vim: ft=python\n{}\n", "y".repeat(1008));
    assert_eq!(rest.len(), 1024);
    let text = format!("{}# n{rest}", filler(40));
    let (view, window) = editor(&text, "build.rs", cx);
    assert_eq!(language(&view, window), Some("rust"));
}

#[gpui::test]
fn hand_choice_beats_modeline(cx: &mut TestAppContext) {
    cx.update(fe_gpui::init);
    let text = format!("fn main() {{}}\n{}// vim: ft=python\n", filler(20));
    let (view, cx) = editor(&text, "main.rs", cx);
    assert_eq!(language(&view, cx), Some("python"));

    view.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    assert_eq!(language(&view, cx), Some("rust"));
    view.read_with(cx, |view, _| assert!(view.is_language_chosen()));

    // A split keeps the choice, though its modeline says otherwise.
    let split = view
        .update_in(cx, |view, window, cx| view.split(window, cx))
        .expect("an editor splits");
    assert_eq!(language(&split, cx), Some("rust"));
    split.read_with(cx, |view, _| assert!(view.is_language_chosen()));
}
