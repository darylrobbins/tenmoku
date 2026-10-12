//! Vim's replace mode and macro recording on the status bar, and the
//! command line's keys recorded into a macro, through the default keymap.

use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, SharedString, TestAppContext, VisualTestContext};

use crate::{Launch, StatusItem, StatusSide, Workbench};

/// A workbench showing a file holding `text`, focused, with vim on.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    std::fs::write(&path, text).unwrap();
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

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The vim items the last frame drew on the bar's left: the mode and the
/// recording, by id.
fn vim_items(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(SharedString, SharedString)> {
    draw(cx);
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Left))
        .into_iter()
        .filter(|(id, _)| [StatusItem::VIM_MODE, StatusItem::VIM_RECORDING].contains(&&**id))
        .collect()
}

fn item(id: &str, text: &str) -> (SharedString, SharedString) {
    (id.to_owned().into(), text.to_owned().into())
}

#[gpui::test]
fn shows_replace_and_recording(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, "abcd\nefgh");
    assert_eq!(
        vim_items(&workbench, cx),
        [item(StatusItem::VIM_MODE, "NORMAL")]
    );

    cx.simulate_keystrokes("shift-r");
    assert_eq!(
        vim_items(&workbench, cx),
        [item(StatusItem::VIM_MODE, "REPLACE")]
    );
    cx.simulate_input("xy");
    cx.simulate_keystrokes("escape");
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.model().text().to_string()),
        "xycd\nefgh"
    );

    cx.simulate_keystrokes("q a");
    assert_eq!(
        vim_items(&workbench, cx),
        [
            item(StatusItem::VIM_MODE, "NORMAL"),
            item(StatusItem::VIM_RECORDING, "recording @a"),
        ]
    );
    // Beside the mode in every mode.
    cx.simulate_keystrokes("i");
    assert_eq!(
        vim_items(&workbench, cx),
        [
            item(StatusItem::VIM_MODE, "INSERT"),
            item(StatusItem::VIM_RECORDING, "recording @a"),
        ]
    );
    cx.simulate_keystrokes("escape q");
    assert_eq!(
        vim_items(&workbench, cx),
        [item(StatusItem::VIM_MODE, "NORMAL")]
    );
}

#[gpui::test]
fn the_line_records_into_a_macro(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, "x\nfoo y\nbar\nfoo y");
    let caret = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            let model = editor.model();
            let point = model
                .text()
                .offset_to_point(model.primary_selection().head());
            (point.row, point.column)
        })
    };
    cx.simulate_keystrokes("q a /");
    cx.simulate_input("foo");
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.simulate_keystrokes("x q");
    assert_eq!(caret(cx), (1, 0));
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    assert!(!bar.read_with(cx, |bar, _| bar.line_open()));

    // The replay searches with no line shown, and deletes there.
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert!(!bar.read_with(cx, |bar, _| bar.line_open()));
    assert_eq!(caret(cx), (3, 0));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.model().text().to_string()),
        "x\noo y\nbar\noo y"
    );
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
}
