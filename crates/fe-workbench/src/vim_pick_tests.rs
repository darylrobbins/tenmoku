//! `:marks` on the status bar's line shows the marks in a picker, and
//! choosing one jumps to it as `` `x `` does.

use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::picker::PickerDelegate as _;

use super::{active, ex, focused, window};
use crate::Workbench;

fn cursor(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let point = model
            .text()
            .offset_to_point(model.primary_selection().head());
        (point.row, point.column)
    })
}

/// The picker's rows' cells, its status, and whether it has focus; `None`
/// if the vim picker isn't the modal showing.
fn shown(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<(Vec<Vec<String>>, Option<String>, bool)> {
    let picker = workbench.read_with(cx, |workbench, cx| workbench.vim_pick(cx))?;
    let focus = picker.read_with(cx, |picker, cx| picker.focus_handle(cx));
    let focused = cx.update(|window, cx| focus.contains_focused(window, cx));
    Some(picker.read_with(cx, |picker, _| {
        let status = picker.delegate.status().map(|status| status.to_string());
        (picker.delegate.rows(), status, focused)
    }))
}

fn row(cells: [&str; 4]) -> Vec<String> {
    cells.map(String::from).to_vec()
}

#[gpui::test]
fn marks_opens_a_picker_and_jumps(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree").unwrap();
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let editor = active(&workbench, cx);

    // `ma` on row 2, then the cursor elsewhere.
    cx.simulate_keystrokes("j l l m a G");
    assert_eq!(cursor(&editor, cx), (2, 2));
    ex("marks", cx);
    let (rows, status, focus) = shown(&workbench, cx).expect("the picker shows");
    assert_eq!(status.as_deref(), Some("mark line  col file/text"));
    assert!(focus);
    // The jump mark `G` set first, as nvim lists it.
    assert_eq!(
        rows[..2],
        [row(["'", "2", "2", "two"]), row(["a", "2", "2", "two"])]
    );

    // The query narrows the rows.
    cx.simulate_input("a");
    let (rows, ..) = shown(&workbench, cx).unwrap();
    assert_eq!(rows, [row(["a", "2", "2", "two"])]);

    // Enter jumps to the mark, from the editor, focused.
    cx.simulate_keystrokes("enter");
    assert!(shown(&workbench, cx).is_none());
    assert!(focused(&editor, cx));
    assert_eq!(cursor(&editor, cx), (1, 2));

    // Escape, twice for the picker's vim normal mode, leaves the cursor.
    cx.simulate_keystrokes("G");
    assert_eq!(cursor(&editor, cx), (2, 2));
    ex("marks", cx);
    assert!(shown(&workbench, cx).is_some());
    cx.simulate_keystrokes("escape escape");
    assert!(shown(&workbench, cx).is_none());
    assert!(focused(&editor, cx));
    assert_eq!(cursor(&editor, cx), (2, 2));
}

fn row3(cells: [&str; 3]) -> Vec<String> {
    cells.map(String::from).to_vec()
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

#[gpui::test]
fn registers_opens_a_picker_and_puts(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree").unwrap();
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let editor = active(&workbench, cx);

    cx.simulate_keystrokes("\" a y y");
    ex("reg", cx);
    let (rows, status, focus) = shown(&workbench, cx).expect("the picker shows");
    assert_eq!(status.as_deref(), Some("Type Name Content"));
    assert!(focus);
    assert!(rows.contains(&row3(["l", "\"a", "one^J"])), "{rows:?}");

    // Enter puts the line below the cursor's row, as `"ap` does, from the
    // editor, focused.
    cx.simulate_input("\"a");
    cx.simulate_keystrokes("enter");
    assert!(shown(&workbench, cx).is_none());
    assert!(focused(&editor, cx));
    assert_eq!(text(&editor, cx), "one\none\ntwo\nthree");
    assert_eq!(cursor(&editor, cx).0, 1);

    // Escape, twice for the picker's vim normal mode, changes nothing.
    ex("di", cx);
    assert!(shown(&workbench, cx).is_some());
    cx.simulate_keystrokes("escape escape");
    assert!(shown(&workbench, cx).is_none());
    assert!(focused(&editor, cx));
    assert_eq!(text(&editor, cx), "one\none\ntwo\nthree");
}
