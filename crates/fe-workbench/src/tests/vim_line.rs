//! Vim's command line in the status bar's place: `/`, `?` and `:` typed in
//! an editor with vim on, through the default keymap.

use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use gpui::{
    AppContext as _, Entity, Focusable as _, Modifiers, SharedString, TestAppContext,
    VisualTestContext, px,
};

use crate::{Launch, StatusBar, StatusItem, StatusSide, Workbench};

const TEXT: &str = "one\nfoo\nbar foo";

/// A workbench showing `files`, each holding [`TEXT`], opened in order,
/// the last focused, with vim on.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    files: &[&str],
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = files.iter().map(|name| dir.path().join(name)).collect();
    for path in &paths {
        std::fs::write(path, TEXT).unwrap();
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
    // So focus leaving the line is a blur.
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
    assert!(focused(&editor, cx));
    (dir, workbench, editor, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn bar(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<StatusBar> {
    workbench.read_with(cx, |workbench, _| workbench.status_bar().clone())
}

/// The open line's prefix and text.
fn line(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(char, String)> {
    let bar = bar(workbench, cx);
    bar.read_with(cx, |bar, cx| bar.line(cx))
}

fn line_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let bar = bar(workbench, cx);
    let input = bar.read_with(cx, |bar, _| bar.line_input().cloned());
    input.is_some_and(|input| cx.update(|window, cx| input.focus_handle(cx).is_focused(window)))
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
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

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn mode(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    editor.read_with(cx, |editor, _| editor.vim_mode())
}

/// The message on the bar's left, once a frame has drawn it.
fn message(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<SharedString> {
    draw(cx);
    let bar = bar(workbench, cx);
    bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Left))
        .into_iter()
        .find(|(id, _)| id == StatusItem::VIM_MESSAGE)
        .map(|(_, text)| text)
}

#[gpui::test]
fn search_through_the_line(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, &["a.txt"]);

    cx.simulate_keystrokes("/");
    assert_eq!(line(&workbench, cx), Some(('/', String::new())));
    assert!(line_focused(&workbench, cx));
    assert!(cx.debug_bounds("vim-line").is_some());
    assert_eq!(cx.debug_bounds("status-bar"), None);
    cx.simulate_input("foo");
    assert_eq!(line(&workbench, cx), Some(('/', "foo".into())));
    assert_eq!(text(&editor, cx), TEXT);
    cx.simulate_keystrokes("enter");
    assert_eq!(caret(&editor, cx), (1, 0));
    assert_eq!(line(&workbench, cx), None);
    assert!(focused(&editor, cx));
    assert_eq!(mode(&editor, cx), Some("normal"));
    draw(cx);
    assert_eq!(cx.debug_bounds("vim-line"), None);
    assert!(cx.debug_bounds("status-bar").is_some());

    cx.simulate_keystrokes("n");
    assert_eq!(caret(&editor, cx), (2, 4));

    // A search that finds nothing leaves the caret, and its error until
    // the next key.
    cx.simulate_keystrokes("/");
    cx.simulate_input("zzz");
    cx.simulate_keystrokes("enter");
    assert_eq!(caret(&editor, cx), (2, 4));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E486: Pattern not found: zzz")
    );
    cx.simulate_keystrokes("h");
    assert_eq!(caret(&editor, cx), (2, 3));
    assert_eq!(message(&workbench, cx), None);
    // Keys bound past vim's reducer end it too, as does one it forwards;
    // a key that leaves the message again shows it.
    let e486 = Some("E486: Pattern not found: zzz");
    for key in ["left", "cmd-c", "shift-q"] {
        cx.simulate_keystrokes("n");
        assert_eq!(message(&workbench, cx).as_deref(), e486);
        cx.simulate_keystrokes("n");
        assert_eq!(message(&workbench, cx).as_deref(), e486);
        cx.simulate_keystrokes(key);
        assert_eq!(message(&workbench, cx), None, "{key}");
    }
    cx.simulate_keystrokes("/");
    cx.simulate_input("foo");
    cx.simulate_keystrokes("enter");

    // An operator's target, as one undo step.
    cx.simulate_keystrokes("g g 0 d /");
    cx.simulate_input("foo");
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, cx), "foo\nbar foo");
    cx.simulate_keystrokes("u");
    assert_eq!(text(&editor, cx), TEXT);

    // Escape, or a backspace on an empty line, cancels.
    let at = caret(&editor, cx);
    cx.simulate_keystrokes("/");
    cx.simulate_input("o");
    cx.simulate_keystrokes("escape");
    assert_eq!(line(&workbench, cx), None);
    assert!(focused(&editor, cx));
    assert_eq!(mode(&editor, cx), Some("normal"));
    assert_eq!((caret(&editor, cx), text(&editor, cx)), (at, TEXT.into()));
    cx.simulate_keystrokes("? backspace");
    assert_eq!(line(&workbench, cx), None);
    assert!(focused(&editor, cx));
    assert_eq!(mode(&editor, cx), Some("normal"));
    cx.simulate_keystrokes("/");
    cx.simulate_input("o");
    cx.simulate_keystrokes("backspace");
    assert_eq!(line(&workbench, cx), Some(('/', String::new())));
    cx.simulate_keystrokes("backspace");
    assert_eq!(line(&workbench, cx), None);
    assert_eq!((caret(&editor, cx), text(&editor, cx)), (at, TEXT.into()));

    // With the status bar hidden, the line shows while it's open.
    workbench.update(cx, |workbench, cx| {
        workbench.set_status_bar_visible(false, cx)
    });
    draw(cx);
    assert_eq!(cx.debug_bounds("status-bar"), None);
    cx.simulate_keystrokes("?");
    draw(cx);
    assert_eq!(line(&workbench, cx), Some(('?', String::new())));
    assert!(line_focused(&workbench, cx));
    assert!(cx.debug_bounds("vim-line").is_some());
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(cx.debug_bounds("vim-line"), None);
    assert_eq!(cx.debug_bounds("status-bar"), None);
}

#[gpui::test]
fn ex_through_the_line(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, &["a.txt"]);

    cx.simulate_keystrokes(":");
    assert_eq!(line(&workbench, cx), Some((':', String::new())));
    cx.simulate_input("2");
    cx.simulate_keystrokes("enter");
    assert_eq!(caret(&editor, cx).0, 1);
    assert!(focused(&editor, cx));

    // From visual mode the line holds the selection's range, and :s
    // changes only its rows.
    cx.simulate_keystrokes("v j :");
    assert_eq!(line(&workbench, cx), Some((':', "'<,'>".into())));
    cx.simulate_input("s/o/0/g");
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, cx), "one\nf00\nbar f00");
    assert_eq!(mode(&editor, cx), Some("normal"));

    cx.simulate_keystrokes(":");
    cx.simulate_input("frobnicate");
    cx.simulate_keystrokes("enter");
    let error = message(&workbench, cx).expect("an error shows");
    assert!(error.starts_with("E492"), "{error}");

    // :w is the host's to run: nothing here saves, and nothing's wrong.
    cx.simulate_keystrokes(":");
    cx.simulate_input("w");
    cx.simulate_keystrokes("enter");
    assert!(editor.read_with(cx, |editor, _| editor.model().is_dirty()));
    assert_eq!(message(&workbench, cx), None);

    // In the line, j and d only type: its context carries no vim keys.
    let at = caret(&editor, cx);
    cx.simulate_keystrokes(":");
    cx.simulate_input("jd");
    assert_eq!(line(&workbench, cx), Some((':', "jd".into())));
    assert_eq!(
        (caret(&editor, cx), text(&editor, cx)),
        (at, "one\nf00\nbar f00".into())
    );
    let vim_keys = cx.update(|window, _| {
        window
            .context_stack()
            .iter()
            .any(|context| context.contains("vim_mode") || context.contains("editor"))
    });
    assert!(!vim_keys);

    // A click in the editor cancels the line.
    let toolbar = cx.debug_bounds("pane-toolbar").expect("drawn");
    let below = gpui::point(toolbar.center().x, toolbar.bottom() + px(30.));
    cx.simulate_click(below, Modifiers::none());
    assert_eq!(line(&workbench, cx), None);
    assert!(focused(&editor, cx));
    assert_eq!(mode(&editor, cx), Some("normal"));
    assert_eq!(text(&editor, cx), "one\nf00\nbar f00");
    draw(cx);
    assert_eq!(cx.debug_bounds("vim-line"), None);
}

#[gpui::test]
fn leaving_the_line_cancels_it(cx: &mut TestAppContext) {
    let (_dir, workbench, b, cx) = opened(cx, &["a.txt", "b.txt"]);

    // Another tab: the line's editor is out of the frame by the time focus
    // has moved, and its operator search is cancelled all the same.
    cx.simulate_keystrokes("d /");
    cx.simulate_input("foo");
    cx.simulate_keystrokes("cmd-shift-]");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(mode(&b, cx), Some("normal"));
    let a = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .unwrap();
    assert_ne!(a, b);
    assert!(focused(&a, cx));
    cx.simulate_keystrokes("cmd-shift-]");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert!(focused(&b, cx));
    assert_eq!((mode(&b, cx), text(&b, cx)), (Some("normal"), TEXT.into()));
    cx.simulate_keystrokes("j");
    assert_eq!(caret(&b, cx), (1, 0));

    // Closing the line's tab closes it.
    cx.simulate_keystrokes("/ cmd-w");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert!(focused(&a, cx));
    assert_eq!(cx.debug_bounds("vim-line"), None);

    // A click lands in the mode the line came from: as it does with none.
    let toolbar = cx.debug_bounds("pane-toolbar").expect("drawn");
    let at = gpui::point(toolbar.left() + px(60.), toolbar.bottom() + px(30.));
    for (count, modifiers) in [
        (1, Modifiers::none()),
        (1, Modifiers::shift()),
        (2, Modifiers::none()),
        (3, Modifiers::none()),
    ] {
        let mut landed = [true, false].map(|line_open| {
            cx.simulate_keystrokes("escape escape g g 0 v j");
            if line_open {
                cx.simulate_keystrokes("/");
                assert!(line(&workbench, cx).is_some());
            }
            click(at, count, modifiers, cx);
            assert_eq!(line(&workbench, cx), None);
            let selections = a.read_with(cx, |editor, _| editor.model().selections().to_vec());
            (mode(&a, cx), selections)
        });
        let [with_line, without] = &mut landed;
        assert_eq!(with_line, without, "{count} clicks with {modifiers:?}");
    }
}

/// Presses and releases the left button `count` times at `position`.
fn click(
    position: gpui::Point<gpui::Pixels>,
    count: usize,
    modifiers: Modifiers,
    cx: &mut VisualTestContext,
) {
    for click_count in 1..=count {
        cx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position,
            modifiers,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(gpui::MouseUpEvent {
            button: gpui::MouseButton::Left,
            position,
            modifiers,
            click_count,
        });
    }
}

#[gpui::test]
fn messages_last_until_the_next_key_in_their_editor(cx: &mut TestAppContext) {
    let (_dir, workbench, b, cx) = opened(cx, &["a.txt", "b.txt"]);
    // A user's binding that searches, outside vim's reducer.
    let source = format!(
        "{}\n[[bindings]]\nkeys = \"f5\"\ncommand = \"vim.move\"\n\
         args = {{ motion = \"search_next\" }}\ncontext = \"editor && vim.mode == normal\"\n",
        fe_keymap::DEFAULT_MACOS
    );
    let store = fe_keymap::KeymapStore::load(&source, None, None);
    assert_eq!(store.diagnostics(), []);
    let keymap = store.keymap().clone();
    cx.update(|_, cx| fe_gpui::set_user_keymap(cx, keymap));
    let e486 = Some("E486: Pattern not found: zzz");
    let fail = |cx: &mut VisualTestContext| {
        cx.simulate_keystrokes("/");
        cx.simulate_input("zzz");
        cx.simulate_keystrokes("enter");
        assert_eq!(message(&workbench, cx).as_deref(), e486);
    };

    // Delete and an unbound function key end it too; a bound key that
    // fails again shows it again.
    for key in ["delete", "f6"] {
        fail(cx);
        cx.simulate_keystrokes(key);
        assert_eq!(message(&workbench, cx), None, "{key}");
    }
    fail(cx);
    cx.simulate_keystrokes("f5");
    assert_eq!(message(&workbench, cx).as_deref(), e486);
    cx.simulate_keystrokes("f5");
    assert_eq!(message(&workbench, cx).as_deref(), e486);

    // Ended, it stays ended across a tab switch.
    cx.simulate_keystrokes("left cmd-shift-] cmd-shift-]");
    draw(cx);
    assert!(focused(&b, cx));
    assert_eq!(message(&workbench, cx), None);

    // A key in another window's editor leaves this one's be.
    fail(cx);
    let other = cx.update(|_, cx| {
        cx.open_window(Default::default(), |window, cx| {
            let view = cx.new(|cx| {
                let model = fe_editor::EditorModel::new(fe_buffer::Buffer::new("x"), None);
                EditorView::new(model, EditorTheme::default_dark(cx), cx)
            });
            window.focus(&view.focus_handle(cx), cx);
            view
        })
        .unwrap()
    });
    cx.cx.simulate_keystrokes(other.into(), "left");
    assert_eq!(message(&workbench, cx).as_deref(), e486);
    cx.update(|window, _| window.activate_window());
    cx.simulate_keystrokes("left");
    assert_eq!(message(&workbench, cx), None);
}
