//! Next and Previous TODO Comment: the caret walks the keywords of the
//! whole buffer's comments, past the rows in view, wrapping nowhere, and a
//! toast says when there are no more.

use std::fs;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::NO_MORE;
use crate::{Workbench, tests::open_root};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A buffer whose keywords are in comments on rows 1, 3 and 160, with one
/// in a string on row 2 and filler past the view between.
fn text() -> String {
    let filler = "    let x = 1;\n".repeat(156);
    format!(
        "fn main() {{\n    // TODO: first\n    let s = \"FIXME: a string\";\n    // FIXME(me): second\n{filler}    // NOTE: third\n}}\n"
    )
}

/// A workbench showing `main.rs` holding [`text`], focused and parsed.
fn opened(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    fs::write(&path, text()).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(300.)));
    cx.update(|window, _| window.activate_window());
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("a file is shown");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    for _ in 0..50 {
        let parsed = editor.read_with(cx, |view, _| {
            view.syntax()
                .is_some_and(|syntax| syntax.tree().is_some() && !syntax.needs_parse())
        });
        if parsed {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        draw(cx);
    }
    draw(cx);
    (dir, workbench, editor, cx)
}

/// Runs command `id` where focus is, in the workbench's context.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The caret's row and column, and whether it selects nothing.
fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32, bool) {
    editor.read_with(cx, |view, _| {
        let model = view.model();
        let selection = model.primary_selection();
        let point = model.text().offset_to_point(selection.head());
        (point.row, point.column, selection.range.is_empty())
    })
}

/// What the window's shown toasts say, top first.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.toasts().read(cx);
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

#[gpui::test]
fn next_and_previous_walk_the_buffers_hits_without_wrapping(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = opened(cx);
    assert_eq!(caret(&editor, cx), (0, 0, true));
    let shows = |row: u32, cx: &mut VisualTestContext| {
        editor.read_with(cx, |view, _| view.shows_buffer_row(row).expect("drawn"))
    };
    assert!(!shows(160, cx), "the last keyword is out of view");

    run("todo.next", cx);
    assert_eq!(caret(&editor, cx), (1, 7, true));
    // The string's keyword is passed over.
    run("todo.next", cx);
    assert_eq!(caret(&editor, cx), (3, 7, true));
    // Out of view, and so not yet scanned for badges: found all the same.
    run("todo.next", cx);
    assert_eq!(caret(&editor, cx), (160, 7, true));
    assert!(shows(160, cx), "revealed");

    // No wrapping: the caret stays at the last.
    run("todo.next", cx);
    assert_eq!(caret(&editor, cx), (160, 7, true));

    run("todo.previous", cx);
    assert_eq!(caret(&editor, cx), (3, 7, true));
    run("todo.previous", cx);
    assert_eq!(caret(&editor, cx), (1, 7, true));
    run("todo.previous", cx);
    assert_eq!(caret(&editor, cx), (1, 7, true), "nor at the first");

    // From inside a keyword, back goes to its start.
    editor.update(cx, |view, _| {
        let at = view.model().text().row_start(3) + 9;
        view.model_mut().select_range(at..at, false);
    });
    run("todo.previous", cx);
    assert_eq!(caret(&editor, cx), (3, 7, true));
}

#[gpui::test]
fn at_the_last_hit_next_toasts(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx);
    for _ in 0..3 {
        run("todo.next", cx);
    }
    assert_eq!(caret(&editor, cx), (160, 7, true));
    assert_eq!(toasts(&workbench, cx), Vec::<String>::new());

    run("todo.next", cx);
    assert_eq!(caret(&editor, cx), (160, 7, true));
    assert_eq!(toasts(&workbench, cx), [NO_MORE]);
}
