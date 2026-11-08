//! Cmd-click go to definition with the default keymap: the click places the
//! caret on the word, then `nav.goto_definition` runs there.

use std::{fs, ops::Range, path::PathBuf};

use fe_gpui::EditorView;
use gpui::{
    Bounds, Entity, EntityInputHandler as _, Focusable as _, Modifiers, Pixels, Point,
    TestAppContext, VisualTestContext, point, px,
};

use super::{open_root, panel};
use crate::Workbench;

const MAIN: &str = "mod util;\n\nfn main() {\n    util::helper();\n}\n";
const UTIL: &str = "pub fn helper() {}\n";

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

/// The active editor's file and its selection.
fn at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, Range<usize>) {
    let editor = editor(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (
            model.path().unwrap().to_owned(),
            model.selections()[0].range.clone(),
        )
    })
}

/// A workbench on a project of `src/main.rs` and `src/util.rs`, showing
/// `main.rs`, and its root.
fn opened(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    PathBuf,
    Entity<Workbench>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), MAIN).unwrap();
    fs::write(root.join("src/util.rs"), UTIL).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    let main = root.join("src/main.rs");
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(main, window, cx));
    });
    settle(cx);
    (dir, root, workbench, cx)
}

/// Just right of the caret position at `offset` in the active editor.
fn at_offset(
    workbench: &Entity<Workbench>,
    offset: usize,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    editor(workbench, cx).update_in(cx, |editor, window, cx| {
        let caret = editor
            .bounds_for_range(offset..offset, Bounds::default(), window, cx)
            .expect("on screen");
        point(caret.left() + px(1.), caret.center().y)
    })
}

#[gpui::test]
fn cmd_click_goes_to_definition(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = opened(cx);
    let main = root.join("src/main.rs");
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    settle(cx);

    // Inside `helper` in `helper()`.
    let clicked = MAIN.find("helper").unwrap() + 2;
    let position = at_offset(&workbench, clicked, cx);
    cx.simulate_click(position, Modifiers::command());
    settle(cx);

    let start = UTIL.find("helper").unwrap();
    let name = start..start + "helper".len();
    assert_eq!(at(&workbench, cx), (root.join("src/util.rs"), name));
    let editor = editor(&workbench, cx);
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));

    // Back returns to the clicked word, where the click put the caret.
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(at(&workbench, cx), (main, clicked..clicked));
}

#[gpui::test]
fn cmd_pressed_while_the_panel_has_focus_hovers_the_editor(cx: &mut TestAppContext) {
    let (_dir, _root, workbench, cx) = opened(cx);
    let editor = editor(&workbench, cx);
    // Focused once, so it knows its context; then the panel takes focus.
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&editor.focus_handle(cx), cx);
    });
    settle(cx);
    let panel = panel(&workbench, cx);
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    settle(cx);

    let start = MAIN.find("helper").unwrap();
    let position = at_offset(&workbench, start + 2, cx);
    cx.simulate_mouse_move(position, None, Modifiers::none());
    settle(cx);
    let hover = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.hover_range());
    assert_eq!(hover(cx), None);
    // The workbench root hears the press and tells the editor.
    cx.simulate_modifiers_change(Modifiers::command());
    settle(cx);
    assert_eq!(hover(cx), Some(start..start + "helper".len()));
    assert!(cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)));
    cx.simulate_modifiers_change(Modifiers::none());
    settle(cx);
    assert_eq!(hover(cx), None);
}
