//! The vim line starts with a `COMMAND` segment in the editor theme's
//! `vim_mode_command`, left of the prefix, for `:`, `/` and `?` alike.

use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, px};

use crate::{Launch, Workbench, status_bar::StatusBar};

/// A workbench showing a file, focused, with vim on.
fn opened(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<StatusBar>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "one\nfoo\nbar\n").unwrap();
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
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    (dir, bar, editor, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn caret_row(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let text = model.text();
        text.offset_to_point(model.primary_selection().range.start)
            .row
    })
}

#[gpui::test]
fn segment_before_each_prefix(cx: &mut TestAppContext) {
    let (_dir, bar, editor, cx) = opened(cx);
    let theme = editor.read_with(cx, |editor, _| editor.theme().clone());
    let colors = Some((theme.vim_mode_command, theme.vim_mode_foreground));
    assert_eq!(bar.read_with(cx, |bar, _| bar.line_mode_colors()), None);

    for key in [":", "/", "?"] {
        cx.simulate_keystrokes(key);
        draw(cx);
        let prefix = key.chars().next().unwrap();
        assert_eq!(
            bar.read_with(cx, |bar, cx| bar.line(cx)),
            Some((prefix, String::new()))
        );
        assert_eq!(bar.read_with(cx, |bar, _| bar.line_mode_colors()), colors);
        let line = cx.debug_bounds("vim-line").expect("the line");
        let mode = cx.debug_bounds("vim-line-mode").expect("the segment");
        let shown = cx.debug_bounds("vim-line-prefix").expect("the prefix");
        assert_eq!(mode.left(), line.left(), "{key}: at the bar's left edge");
        assert_eq!(shown.left() - mode.right(), px(8.), "{key}: 8 px gap");

        cx.simulate_keystrokes("escape");
        draw(cx);
        assert_eq!(bar.read_with(cx, |bar, cx| bar.line(cx)), None);
        assert_eq!(bar.read_with(cx, |bar, _| bar.line_mode_colors()), None);
        assert_eq!(cx.debug_bounds("vim-line-mode"), None);
    }

    // Typing and Enter still run the line.
    cx.simulate_keystrokes("/");
    cx.simulate_input("bar");
    draw(cx);
    assert_eq!(
        bar.read_with(cx, |bar, cx| bar.line(cx)),
        Some(('/', "bar".into()))
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(bar.read_with(cx, |bar, cx| bar.line(cx)), None);
    assert_eq!(caret_row(&editor, cx), 2);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.vim_mode()),
        Some("normal")
    );
}
