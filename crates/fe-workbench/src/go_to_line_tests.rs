use std::fs;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Workbench, picker::PickerDelegate as _, tests::open_root};

/// A workbench showing a 50-row file, "line 1" to "line 50", whose rows
/// 3 and 4 hold `é` for a column in chars, not bytes.
fn fifty(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &mut VisualTestContext,
    Entity<EditorView>,
) {
    let dir = tempfile::tempdir().unwrap();
    let text: Vec<String> = (1..=50)
        .map(|n| match n {
            3 => "éé line 3".to_owned(),
            n => format!("line {n}"),
        })
        .collect();
    let path = dir.path().join("a.txt");
    fs::write(&path, text.join("\n")).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    // Short, so the file's 50 rows scroll.
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(300.)));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = active_editor(&workbench, cx).expect("a file is shown");
    (dir, workbench, cx, editor)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn active_editor(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<EditorView>> {
    workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
}

fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The caret's (row, byte column), and whether it's the only selection.
fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> ((u32, u32), bool) {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let selection = model.primary_selection();
        let point = model.snapshot().text().offset_to_point(selection.head());
        (
            (point.row, point.column),
            model.selections().len() == 1 && selection.range.is_empty(),
        )
    })
}

/// Whether the last frame laid out buffer `row`.
fn shown(editor: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.shows_buffer_row(row).expect("drawn"))
}

fn open(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, _| workbench.go_to_line_open())
}

/// Opens the picker, types `query`, and returns its match count and status.
fn ask(workbench: &Entity<Workbench>, query: &str, cx: &mut VisualTestContext) -> (usize, String) {
    if !open(workbench, cx) {
        dispatch("go_to_line.toggle", cx);
    }
    cx.simulate_input(query);
    draw(cx);
    let picker = workbench.read_with(cx, |workbench, _| workbench.go_to_line_picker().clone());
    picker.read_with(cx, |picker, _| {
        (picker.delegate.match_count(), picker.delegate.status_text())
    })
}

fn go(workbench: &Entity<Workbench>, query: &str, cx: &mut VisualTestContext) {
    ask(workbench, query, cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(!open(workbench, cx), "{query}");
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn moves_the_caret(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, editor) = fifty(cx);

    // From the last row, row 11 is offscreen: the jump scrolls to it.
    go(&workbench, "999", cx);
    assert_eq!(caret(&editor, cx), ((49, 0), true));
    assert!(shown(&editor, 49, cx));
    assert!(
        !shown(&editor, 11, cx),
        "the file is taller than the window"
    );
    let (rows, _) = ask(&workbench, "12", cx);
    assert_eq!(rows, 1);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(caret(&editor, cx), ((11, 0), true));
    assert!(shown(&editor, 11, cx));
    assert!(!shown(&editor, 49, cx));
    assert!(focused(&editor, cx));

    // Row 3 is "éé line 3": column 5 is after two two-byte chars and two
    // more.
    go(&workbench, "3:5", cx);
    assert_eq!(caret(&editor, cx), ((2, 6), true));
    go(&workbench, ": 2 : 3 ", cx);
    assert_eq!(caret(&editor, cx), ((1, 2), true));

    go(&workbench, "999", cx);
    assert_eq!(caret(&editor, cx), ((49, 0), true));
    assert!(shown(&editor, 49, cx));
    go(&workbench, "3:999", cx);
    assert_eq!(caret(&editor, cx), ((2, 11), true));
}

#[gpui::test]
fn status_and_invalid_queries(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, editor) = fifty(cx);
    go(&workbench, "7", cx);
    let before = caret(&editor, cx);

    dispatch("go_to_line.toggle", cx);
    let (rows, status) = ask(&workbench, "", cx);
    assert_eq!((rows, status.as_str()), (0, "Line 7 of 50"));

    for query in ["abc", "0"] {
        let (rows, status) = ask(&workbench, query, cx);
        assert_eq!(
            (rows, status.as_str()),
            (0, "Type a line number between 1 and 50"),
            "{query}"
        );
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(open(&workbench, cx), "{query}");
        assert_eq!(caret(&editor, cx), before);
        cx.simulate_keystrokes("ctrl-a backspace");
        draw(cx);
    }
}

#[gpui::test]
fn back_and_escape(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, editor) = fifty(cx);
    assert_eq!(caret(&editor, cx).0, (0, 0));
    go(&workbench, "40", cx);
    assert_eq!(caret(&editor, cx).0, (39, 0));
    cx.simulate_keystrokes("ctrl--");
    draw(cx);
    assert_eq!(caret(&editor, cx).0, (0, 0));

    dispatch("go_to_line.toggle", cx);
    assert!(open(&workbench, cx));
    cx.simulate_input("20");
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!open(&workbench, cx));
    assert_eq!(caret(&editor, cx).0, (0, 0));
    assert!(focused(&editor, cx));

    // Toggled while it shows, it dismisses.
    dispatch("go_to_line.toggle", cx);
    dispatch("go_to_line.toggle", cx);
    assert!(!open(&workbench, cx));
}

#[gpui::test]
fn nothing_without_an_editor(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    dispatch("go_to_line.toggle", cx);
    assert!(!open(&workbench, cx));
}
