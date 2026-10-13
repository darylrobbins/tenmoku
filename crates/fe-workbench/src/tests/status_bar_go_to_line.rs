//! Clicking the status bar's line and column opens Go to Line/Column.

use std::fs;

use fe_buffer::ResolvedSelection;
use fe_gpui::{EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_shell::PaneItem as _;

use super::open_root;
use crate::{StatusItem, Workbench};

/// A workbench showing a three-line file, focused, with the caret on line 2.
fn opened(
    cx: &mut TestAppContext,
    vim: bool,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\ntwo\nthree").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    if vim {
        cx.update(|_, cx| set_vim_enabled(cx, true));
        draw(cx);
    }
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_selections([ResolvedSelection {
            range: 4..4,
            reversed: false,
        }]);
        cx.notify();
    });
    draw(cx);
    (dir, workbench, editor, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The caret's row, from zero.
fn row(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let head = model.primary_selection().head();
        model.snapshot().text().offset_to_point(head).row
    })
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn open(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, _| workbench.go_to_line_open())
}

/// Clicks the cursor item.
fn click_cursor(cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(format!("status-item:{}", StatusItem::CURSOR).leak())
        .expect("drawn");
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

fn assert_cursor_item_toggles_go_to_line(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let items = cx.update(|_, cx| editor.read(cx).status_items(cx));
    let cursor = items.iter().find(|item| item.id == StatusItem::CURSOR);
    let command = cursor.and_then(|item| item.command.as_ref());
    assert_eq!(
        command.map(|command| command.command.as_str()),
        Some("go_to_line.toggle")
    );
}

#[gpui::test]
fn click_opens_go_to_line(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, false);
    assert_cursor_item_toggles_go_to_line(&editor, cx);
    assert_eq!(row(&editor, cx), 1);

    click_cursor(cx);
    assert!(open(&workbench, cx));
    let picker = workbench.read_with(cx, |workbench, _| workbench.go_to_line_picker().clone());
    let status = picker.read_with(cx, |picker, _| picker.delegate.status_text());
    assert_eq!(status, "Line 2 of 3");

    cx.simulate_input("3");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(!open(&workbench, cx));
    assert_eq!(row(&editor, cx), 2);
    assert!(focused(&editor, cx));
}

#[gpui::test]
fn click_with_vim_on(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, true);
    assert_cursor_item_toggles_go_to_line(&editor, cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.vim_mode()),
        Some("normal")
    );
    let caret = editor.read_with(cx, |editor, _| editor.model().primary_selection().head());

    click_cursor(cx);
    assert!(open(&workbench, cx));
    // With vim on, the picker's first escape enters its own normal mode.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(open(&workbench, cx));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!open(&workbench, cx));
    assert!(focused(&editor, cx));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.vim_mode()),
        Some("normal")
    );
    let after = editor.read_with(cx, |editor, _| editor.model().primary_selection().head());
    assert_eq!(after, caret);

    cx.simulate_keystrokes("j");
    draw(cx);
    assert_eq!(row(&editor, cx), 2);
}
