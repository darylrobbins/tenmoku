//! Up and Down in the vim line recall its history: earlier `/` and `?`
//! lines, or earlier `:` lines, by the prefix typed. A line cancelled
//! without a key goes into a macro recording as its text and `<C-c>`.

use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled, vim_registers};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext, px};

use crate::{Launch, Workbench};

#[path = "complete_tests.rs"]
mod complete_tests;

#[path = "incsearch_tests.rs"]
mod incsearch_tests;

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
    opened_files(cx, &[("a.txt", TEXT)])
}

/// A workbench showing `files`, each name with its text, opened in order,
/// the last focused, with vim on.
fn opened_files<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        paths.push(path);
    }
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
    for path in paths {
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
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

/// What register `name` holds.
fn register(name: char, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| Some(vim_registers(cx)?.get(name)?.text.clone()))
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown")
}

#[gpui::test]
fn a_line_cancelled_without_a_key_records_ctrl_c(cx: &mut TestAppContext) {
    let (_dir, workbench, b, cx) = opened_files(cx, &[("a.txt", TEXT), ("b.txt", TEXT)]);

    // Focus leaving the line.
    cx.simulate_keystrokes("q a /");
    cx.simulate_input("foo");
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    cx.update(|window, cx| window.focus(&b.focus_handle(cx), cx));
    cx.simulate_keystrokes("x q");
    assert_eq!(register('a', cx).as_deref(), Some("/foo\x03x"));

    // A click outside it.
    cx.simulate_keystrokes("q b /");
    cx.simulate_input("bar");
    let toolbar = cx.debug_bounds("pane-toolbar").expect("drawn");
    let below = gpui::point(toolbar.left() + px(60.), toolbar.bottom() + px(30.));
    cx.simulate_click(below, Modifiers::none());
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    cx.simulate_keystrokes("q");
    assert_eq!(register('b', cx).as_deref(), Some("/bar\x03"));

    // Switching tabs, and back to stop.
    cx.simulate_keystrokes("q c /");
    cx.simulate_input("far");
    cx.simulate_keystrokes("cmd-shift-]");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_ne!(active(&workbench, cx), b);
    cx.simulate_keystrokes("cmd-shift-]");
    draw(cx);
    assert_eq!(active(&workbench, cx), b);
    cx.simulate_keystrokes("q");
    assert_eq!(register('c', cx).as_deref(), Some("/far\x03"));

    // A replay cancels the line as it was, leaving no line open.
    cx.simulate_keystrokes("g g 0 @ a");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(
        b.read_with(cx, |editor, _| editor.vim_mode()),
        Some("normal")
    );
    assert_eq!(caret(&b, cx), (0, 0));
    assert_eq!(
        b.read_with(cx, |editor, _| editor.model().text().to_string()),
        TEXT[2..]
    );
}
