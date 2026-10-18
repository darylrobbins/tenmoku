//! `git.accept_*` and `git.accept_all_*`, in a headless workbench.

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, TestAppContext, VisualTestContext};
use serde_json::Value;

use super::{TWO, caret_row, dispatch, draw, open_text, toasts};

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn put_caret(editor: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) {
    let at = editor.read_with(cx, |editor, _| editor.model().text().row_start(row));
    editor.update(cx, |editor, _| {
        editor.model_mut().select_range(at..at, false)
    });
}

fn dispatch_with(id: &str, args: Value, cx: &mut VisualTestContext) {
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    let invocation = Invocation::of(id).unwrap().args(args).context(context);
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    draw(cx);
}

const FIRST_CURRENT: &str = "a\nmid\n<<<<<<< HEAD\nc\n||||||| base\nd\n=======\ne\n>>>>>>> other\n";
const FIRST_INCOMING: &str =
    "b\nmid\n<<<<<<< HEAD\nc\n||||||| base\nd\n=======\ne\n>>>>>>> other\n";
const FIRST_BOTH: &str = "a\nb\nmid\n<<<<<<< HEAD\nc\n||||||| base\nd\n=======\ne\n>>>>>>> other\n";

#[gpui::test]
fn accept_current_incoming_and_both_at_the_caret(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = open_text(cx, TWO);
    for (id, expected) in [
        ("git.accept_current", FIRST_CURRENT),
        ("git.accept_incoming", FIRST_INCOMING),
        ("git.accept_both", FIRST_BOTH),
    ] {
        put_caret(&editor, 1, cx);
        dispatch(id, cx);
        assert_eq!(text(&editor, cx), expected, "{id}");
        assert_eq!(caret_row(&editor, cx), 0, "{id}");
        dispatch("editor.undo", cx);
        assert_eq!(text(&editor, cx), TWO, "{id}");
    }
}

#[gpui::test]
fn a_row_argument_picks_the_conflict(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = open_text(cx, TWO);
    put_caret(&editor, 5, cx);
    dispatch_with("git.accept_incoming", serde_json::json!({ "row": 6 }), cx);
    assert_eq!(
        text(&editor, cx),
        "<<<<<<< HEAD\na\n=======\nb\n>>>>>>> other\nmid\ne\n"
    );
}

#[gpui::test]
fn accept_all_resolves_every_conflict_in_one_undo_step(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = open_text(cx, TWO);
    for (id, expected) in [
        ("git.accept_all_current", "a\nmid\nc\n"),
        ("git.accept_all_incoming", "b\nmid\ne\n"),
        ("git.accept_all_both", "a\nb\nmid\nc\ne\n"),
    ] {
        put_caret(&editor, 5, cx);
        dispatch(id, cx);
        assert_eq!(text(&editor, cx), expected, "{id}");
        assert_eq!(caret_row(&editor, cx), 0, "{id}");
        dispatch("editor.undo", cx);
        assert_eq!(text(&editor, cx), TWO, "{id}");
    }
}

#[gpui::test]
fn outside_a_conflict_says_so(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = open_text(cx, TWO);
    put_caret(&editor, 5, cx);
    for id in [
        "git.accept_current",
        "git.accept_incoming",
        "git.accept_both",
    ] {
        dispatch(id, cx);
        assert_eq!(text(&editor, cx), TWO, "{id}");
    }
    assert_eq!(
        toasts(&workbench, cx).first().map(String::as_str),
        Some("Editor cursor is not within a merge conflict")
    );
}

#[gpui::test]
fn navigation_and_accept_all_see_conflicts_in_a_large_file(cx: &mut TestAppContext) {
    let filler = "x".repeat(crate::git::git_conflicts::MAX_SCAN_BYTES);
    let big = format!("{filler}\n{TWO}");
    let (_dir, _workbench, editor, cx) = open_text(cx, &big);
    dispatch("git.next_conflict", cx);
    assert_eq!(caret_row(&editor, cx), 1);
    dispatch("git.accept_all_current", cx);
    assert_eq!(text(&editor, cx), format!("{filler}\na\nmid\nc\n"));
}
