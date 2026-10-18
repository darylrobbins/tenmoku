//! Conflict bands and `git.next_conflict` / `git.previous_conflict`, in a
//! headless workbench.

use std::collections::BTreeSet;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::decorations::{DecorationKind, Tint};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{draw, open, open_file, write};
use crate::Workbench;

/// Two conflicts: rows 0-4 a two-way one, rows 6-12 one with a base.
const TWO: &str = "<<<<<<< HEAD\na\n=======\nb\n>>>>>>> other\nmid\n\
<<<<<<< HEAD\nc\n||||||| base\nd\n=======\ne\n>>>>>>> other\n";

/// The rows banded with each tint, over the whole text.
fn bands(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> [BTreeSet<u32>; 3] {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let text = model.text();
        let mut rows = [BTreeSet::new(), BTreeSet::new(), BTreeSet::new()];
        for decoration in model.decorations(0..text.len()) {
            if decoration.kind != DecorationKind::RowBackground {
                continue;
            }
            let into = match decoration.tint {
                Tint::Added => 0,
                Tint::Muted => 1,
                Tint::Modified => 2,
                other => panic!("unexpected tint {other:?}"),
            };
            rows[into].extend(decoration.covered_rows(text));
        }
        rows
    })
}

fn set(rows: &[u32]) -> BTreeSet<u32> {
    rows.iter().copied().collect()
}

/// Runs workbench command `id` with the editor focused.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

/// The row the primary caret is on.
fn caret_row(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        model
            .text()
            .offset_to_point(model.primary_selection().head())
            .row
    })
}

fn open_text<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "f.txt", text);
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &dir.path().join("f.txt"), cx);
    (dir, workbench, editor, cx)
}

#[gpui::test]
fn conflict_rows_are_banded_by_side(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = open_text(cx, TWO);
    let banded = [set(&[0, 1, 6, 7]), set(&[8, 9]), set(&[3, 4, 11, 12])];
    assert_eq!(bands(&editor, cx), banded);

    // Typing inside the current side.
    let at = editor.read_with(cx, |editor, _| editor.model().text().row_start(1) + 1);
    editor.update(cx, |editor, _| {
        editor.model_mut().select_range(at..at, false);
        editor.model_mut().insert("x");
    });
    draw(cx);
    assert_eq!(bands(&editor, cx), banded);

    // Deleting the second conflict's end marker row.
    let range = editor.read_with(cx, |editor, _| {
        let text = editor.model().text();
        text.row_start(12)..text.len()
    });
    editor.update(cx, |editor, _| {
        editor.model_mut().select_range(range, false);
        editor.model_mut().insert("");
    });
    draw(cx);
    assert_eq!(bands(&editor, cx), [set(&[0, 1]), set(&[]), set(&[3, 4])]);
}

#[gpui::test]
fn next_and_previous_conflict_wrap_round(cx: &mut TestAppContext) {
    let conflict = "<<<<<<< a\nx\n=======\ny\n>>>>>>> b\n";
    let text = format!("top\n{conflict}mid\n{conflict}{conflict}");
    let (_dir, workbench, editor, cx) = open_text(cx, &text);
    assert_eq!(caret_row(&editor, cx), 0);

    for row in [1, 7, 12] {
        dispatch("git.next_conflict", cx);
        assert_eq!(caret_row(&editor, cx), row);
        assert!(toasts(&workbench, cx).is_empty());
    }
    dispatch("git.next_conflict", cx);
    assert_eq!(caret_row(&editor, cx), 1);
    assert_eq!(toasts(&workbench, cx), ["Wrapped to the first conflict"]);

    dispatch("git.previous_conflict", cx);
    assert_eq!(caret_row(&editor, cx), 12);
    assert!(toasts(&workbench, cx).contains(&"Wrapped to the last conflict".to_owned()));
    dispatch("git.previous_conflict", cx);
    assert_eq!(caret_row(&editor, cx), 7);
}

#[gpui::test]
fn no_conflicts_says_so(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = open_text(cx, "one\ntwo\nthree\n");
    let at = editor.read_with(cx, |editor, _| editor.model().text().row_start(1));
    editor.update(cx, |editor, _| {
        editor.model_mut().select_range(at..at, false)
    });
    for id in ["git.next_conflict", "git.previous_conflict"] {
        dispatch(id, cx);
        assert_eq!(caret_row(&editor, cx), 1);
    }
    assert_eq!(
        toasts(&workbench, cx).first().map(String::as_str),
        Some("No merge conflicts found in this file")
    );
}
