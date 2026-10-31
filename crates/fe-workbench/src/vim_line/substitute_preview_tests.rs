//! 'inccommand' through the line: each edit of a `:` line that is a `:s`
//! reaches its editor, which previews it; other `:` lines don't, but for
//! the edit that stops one being a `:s`, and with `vim.inccommand` off none
//! do.

use fe_editor::decorations::DecorationKind;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled, set_vim_inccommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

const TEXT: &str = "one a\ntwo a\nthree\n";

/// A workbench showing a file holding [`TEXT`], focused, with vim on.
fn opened(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, TEXT).unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown");
    (dir, workbench, editor, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The `:` lines the bar told the editor since last asked.
fn forwarded(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.update(cx, |bar, _| bar.take_forwarded())
}

/// The text the editor shows after its rows.
fn after_rows(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    draw(cx);
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let decorations = model.decorations(0..model.text().len());
        (decorations.into_iter())
            .filter_map(|decoration| match decoration.kind {
                DecorationKind::AfterLine(after) => Some(after.to_string()),
                _ => None,
            })
            .collect()
    })
}

/// Types `text` on the open line a character at a time, each an edit.
fn type_each(text: &str, cx: &mut VisualTestContext) {
    for ch in text.chars() {
        cx.simulate_input(&ch.to_string());
        draw(cx);
    }
}

#[gpui::test]
fn colon_lines_reach_the_editor(cx: &mut TestAppContext) {
    // A loaded machine's clock mustn't make a preview give up.
    fe_vim::set_search_preview_limit(Some(std::time::Duration::from_secs(3600)));
    let (_dir, workbench, editor, cx) = opened(cx);
    cx.simulate_keystrokes(":");
    draw(cx);
    assert_eq!(forwarded(&workbench, cx), Vec::<String>::new());
    // `%` alone is no `:s`; from `%s` on, each edit reaches the editor.
    type_each("%s/a/b/", cx);
    assert_eq!(
        forwarded(&workbench, cx),
        ["%s", "%s/", "%s/a", "%s/a/", "%s/a/b", "%s/a/b/"]
    );
    assert_eq!(after_rows(&editor, cx), ["→ one b", "→ two b"]);
    // Backing out to what is no `:s` takes the preview down, told once.
    for _ in 0..6 {
        cx.simulate_keystrokes("backspace");
        draw(cx);
    }
    assert_eq!(
        forwarded(&workbench, cx),
        ["%s/a/b", "%s/a/", "%s/a", "%s/", "%s", "%"]
    );
    assert_eq!(after_rows(&editor, cx), Vec::<String>::new());
    type_each("d", cx);
    assert_eq!(forwarded(&workbench, cx), Vec::<String>::new());
    cx.simulate_keystrokes("escape");
    draw(cx);

    // Other commands never reach it.
    cx.simulate_keystrokes(":");
    type_each("w", cx);
    type_each("q", cx);
    assert_eq!(forwarded(&workbench, cx), Vec::<String>::new());
    cx.simulate_keystrokes("escape");
    draw(cx);

    // Escape after a preview leaves the text as it was, nothing painted.
    cx.simulate_keystrokes(":");
    type_each("%s/a/b/", cx);
    assert_eq!(after_rows(&editor, cx).len(), 2);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(after_rows(&editor, cx), Vec::<String>::new());
    let text = editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, TEXT);
    forwarded(&workbench, cx);

    // `vim.inccommand = false`: nothing is sent, nothing shows.
    cx.update(|_, cx| set_vim_inccommand(cx, false));
    cx.simulate_keystrokes(":");
    type_each("%s/a/b/", cx);
    assert_eq!(forwarded(&workbench, cx), Vec::<String>::new());
    assert_eq!(after_rows(&editor, cx), Vec::<String>::new());
    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.update(|_, cx| set_vim_inccommand(cx, true));
}
