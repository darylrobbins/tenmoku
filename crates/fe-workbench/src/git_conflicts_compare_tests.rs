//! `git.compare_conflict` and the lens item that runs it, in a headless
//! workbench.

use fe_command::{Invocation, Value};
use fe_gpui::{DiffView, EditorView};
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{dispatch, draw, open, open_file, write};
use crate::Workbench;

const RUST: &str = "fn a() {}\n<<<<<<< HEAD\nlet x = 1;\n=======\nlet x = 2;\n>>>>>>> other\n";

fn active_diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DiffView> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.active_item())
            .and_then(|item| item.downcast_ref::<DiffView>().cloned())
            .expect("a diff view is the active tab")
    })
}

fn language(side: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    side.read_with(cx, |side, _| side.syntax().map(|s| s.language().name()))
}

#[gpui::test]
fn compare_opens_the_two_sides_in_a_diff(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "f.rs", RUST);
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &dir.path().join("f.rs"), cx);
    let at = editor.read_with(cx, |editor, _| editor.model().text().row_start(2));
    editor.update(cx, |editor, _| {
        editor.model_mut().select_range(at..at, false)
    });

    dispatch("git.compare_conflict", cx);
    cx.run_until_parked();
    draw(cx);

    let diff = active_diff(&workbench, cx);
    let sides = diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()]);
    let texts = sides
        .clone()
        .map(|side| side.read_with(cx, |side, _| side.model().buffer().text().to_string()));
    assert_eq!(texts, ["let x = 1;\n", "let x = 2;\n"]);
    for side in &sides {
        assert!(side.read_with(cx, |side, _| side.is_read_only()));
        assert_eq!(language(side, cx), Some("rust"));
    }
    let title = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.active_item().unwrap().tab_title(cx).to_string()
    });
    assert_eq!(
        title,
        "f.rs (f.rs: Current Changes ↔ f.rs: Incoming Changes)"
    );
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.model().text().to_string(), RUST);
    });
}

#[gpui::test]
fn the_fourth_lens_item_compares(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "f.rs", &format!("{RUST}{RUST}"));
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &dir.path().join("f.rs"), cx);
    let lenses = editor.update(cx, |editor, _| {
        let snapshot = editor.model().snapshot().clone();
        (editor.lens_providers().iter())
            .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
            .collect::<Vec<_>>()
    });
    assert_eq!(lenses.len(), 2);
    for lens in lenses {
        assert_eq!(lens.items.len(), 4);
        let last = lens.items.last().unwrap();
        let args = Value::Object([("row".to_owned(), lens.row.into())].into_iter().collect());
        assert_eq!(last.title, "Compare Changes");
        assert_eq!(
            last.invocation,
            Invocation::of("git.compare_conflict").unwrap().args(args)
        );
    }
}
