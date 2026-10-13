//! Up and Down in the vim line recall its history: earlier `/` and `?`
//! lines, or earlier `:` lines, by the prefix typed.

use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

const TEXT: &str = "one\nfoo\nbar foo\nfar";

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

/// The open line's prefix and text.
fn line(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(char, String)> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, cx| bar.line(cx))
}

/// Submits `text` on a line opened by `key`.
fn submit(key: &str, text: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(key);
    cx.simulate_input(text);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

/// The caret's row and column.
fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let point = model
            .text()
            .offset_to_point(model.primary_selection().range.start);
        (point.row, point.column)
    })
}

/// Each step's key, then the line it leaves.
fn steps(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
    prefix: char,
    steps: &[(&str, &str)],
) {
    for (key, text) in steps {
        cx.simulate_keystrokes(key);
        assert_eq!(
            line(workbench, cx),
            Some((prefix, text.to_string())),
            "after {key}"
        );
    }
}

#[gpui::test]
fn up_and_down_recall_search_lines(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx);
    submit("/", "foo", cx);
    submit("/", "bar", cx);
    cx.simulate_keystrokes("g g");

    cx.simulate_keystrokes("/");
    steps(
        &workbench,
        cx,
        '/',
        &[
            ("up", "bar"),
            ("up", "foo"),
            ("up", "foo"),
            ("down", "bar"),
            // Past the newest: what was typed, nothing.
            ("down", ""),
            ("down", ""),
            ("up", "bar"),
        ],
    );
    // The caret is at the text's end: typing goes after the entry.
    cx.simulate_input(" foo");
    assert_eq!(line(&workbench, cx), Some(('/', "bar foo".into())));
    cx.simulate_keystrokes("enter");
    assert_eq!(caret(&editor, cx), (2, 0));

    // A new line starts from the newest again.
    cx.simulate_keystrokes("g g /");
    steps(&workbench, cx, '/', &[("up", "bar foo"), ("up", "bar")]);
    cx.simulate_keystrokes("enter");
    assert_eq!(caret(&editor, cx), (2, 0));
}

#[gpui::test]
fn recall_goes_by_the_prefix_typed(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx);
    for text in ["foo", "bar", "far"] {
        submit("/", text, cx);
    }

    cx.simulate_keystrokes("/");
    cx.simulate_input("f");
    steps(
        &workbench,
        cx,
        '/',
        &[
            ("up", "far"),
            ("up", "foo"),
            ("up", "foo"),
            ("down", "far"),
            ("down", "f"),
        ],
    );
    // An edit makes the line the prefix, from the entry shown.
    cx.simulate_keystrokes("up up backspace");
    assert_eq!(line(&workbench, cx), Some(('/', "fo".into())));
    steps(&workbench, cx, '/', &[("down", "fo"), ("up", "foo")]);
    cx.simulate_keystrokes("escape");
    assert_eq!(line(&workbench, cx), None);
}

#[gpui::test]
fn question_mark_and_colon_lines_recall_their_histories(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx);
    submit("/", "foo/e", cx);
    submit(":", "4", cx);

    // `?` shares `/`'s history, the entry's delimiter swapped.
    cx.simulate_keystrokes("?");
    steps(&workbench, cx, '?', &[("up", "foo?e"), ("up", "foo?e")]);
    cx.simulate_keystrokes("escape");

    // `:` has its own.
    cx.simulate_keystrokes("g g 0 :");
    steps(
        &workbench,
        cx,
        ':',
        &[("up", "4"), ("up", "4"), ("down", "")],
    );
    cx.simulate_keystrokes("up enter");
    assert_eq!(caret(&editor, cx), (3, 0));
}

#[gpui::test]
fn an_escaped_line_stays_in_its_history(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx);
    cx.simulate_keystrokes("/");
    cx.simulate_input("bar");
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes(":");
    cx.simulate_input("3");
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);

    cx.simulate_keystrokes("/");
    steps(&workbench, cx, '/', &[("up", "bar")]);
    cx.simulate_keystrokes("escape :");
    steps(&workbench, cx, ':', &[("up", "3")]);
    cx.simulate_keystrokes("enter");
    assert_eq!(caret(&editor, cx), (2, 0));
    // A backspace on an empty line adds nothing.
    cx.simulate_keystrokes("/ backspace / up");
    assert_eq!(line(&workbench, cx), Some(('/', "bar".into())));
}
