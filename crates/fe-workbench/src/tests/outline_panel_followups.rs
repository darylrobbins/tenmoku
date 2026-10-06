//! Follow-ups from the review of the outline panel (T78).

use std::fs;

use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::open_root;
use crate::{OutlinePanel, Workbench};

const RUST: &str = "\
struct Point;

impl Point {
    fn new() -> Self {
        Point
    }
}

fn main() {}
";

fn rows(outline: &Entity<OutlinePanel>, cx: &mut VisualTestContext) -> Vec<(u32, String)> {
    outline.read_with(cx, |outline, _| outline.rows())
}

fn jobs_started(outline: &Entity<OutlinePanel>, cx: &mut VisualTestContext) -> usize {
    outline.read_with(cx, |outline, _| outline.jobs_started())
}

/// The editor's language, if it has a syntax.
fn language(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    editor.read_with(cx, |editor, _| {
        editor.syntax().map(|syntax| syntax.language().name())
    })
}

/// Sets the editor's language, and lets the update's effects flush, which
/// draws the window: the editor parses within its render, and the panel
/// refreshes once the frame is drawn. The executor doesn't run, so a job
/// that starts doesn't land.
fn set_language(editor: &Entity<EditorView>, name: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| editor.set_language(Some(name), cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// F3: a job for Python, started while a Rust file was set to Python, lands
/// after it's set back to Rust, reparsed at the version the Rust outline
/// shown is of. The Rust outline stays.
#[gpui::test]
fn a_job_for_a_language_switched_away_from_lands_unshown(cx: &mut TestAppContext) {
    // Loaded up front, so setting either language parses without waiting.
    for name in ["rust", "python"] {
        assert!(fe_syntax::language_by_name(name).is_some());
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, RUST).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-o");
    let outline = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().clone()
    });
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    cx.run_until_parked();
    let editor = active_editor(&workbench, cx);

    let shown = rows(&outline, cx);
    assert_eq!(
        shown,
        [
            (0, "Point".to_string()),
            (0, "impl Point".to_string()),
            (1, "new".to_string()),
            (0, "main".to_string()),
        ]
    );

    let started = jobs_started(&outline, cx);
    set_language(&editor, "python", cx);
    assert_eq!(language(&editor, cx), Some("python"));
    assert_eq!(jobs_started(&outline, cx), started + 1, "Python's job runs");

    set_language(&editor, "rust", cx);
    assert_eq!(language(&editor, cx), Some("rust"));
    assert_eq!(rows(&outline, cx), shown);

    cx.run_until_parked();
    assert_eq!(rows(&outline, cx), shown, "Rust's outline stays");
    // Python's outline was dropped as it landed, rather than shown until a
    // Rust job replaced it.
    assert_eq!(jobs_started(&outline, cx), started + 1);
    let version = editor.read_with(cx, |editor, _| editor.model().snapshot().version());
    let outlined = outline.read_with(cx, |outline, _| outline.outline().unwrap().version);
    assert_eq!(outlined, version);
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}
