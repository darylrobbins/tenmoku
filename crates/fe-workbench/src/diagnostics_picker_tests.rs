//! Search Diagnostics in File and in Workspace: the Problems panel's
//! problems (an editor's diagnostics and a task's alike), errors first, the
//! document variant the active file's alone; Enter opening at the range, and
//! a move previewing in the editor until escape puts the scroll back.

use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::diagnostics::{Diagnostic, Problem, Severity};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Workbench,
    dock::problems_panel::{ColumnUnit, SourceProblem},
    tests::open_root,
    workbench::Palette,
};

/// `a.txt`: an error on `bad` (row 3), a warning on `odd` (row 150), filler
/// between them, far enough apart to scroll.
fn a_text() -> String {
    let filler = "filler\n".repeat(146);
    format!("one\ntwo\nbad\n{filler}odd\nend\n")
}

/// `b.txt`: an error on `wrong`, a hint on `meh`.
const B: &str = "wrong\nmeh\n";

/// `c.txt`, open in no editor: a task's error on row 2.
const C: &str = "first\nbroken\n";

/// A workbench rooted at a temp dir holding `a.txt`, `b.txt` and `c.txt`,
/// `b.txt` then `a.txt` open (`a.txt` shown), with `a.txt`'s and `b.txt`'s
/// diagnostics applied and a task's problem in `c.txt`.
fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = a_text();
    for (name, text) in [("a.txt", a.as_str()), ("b.txt", B), ("c.txt", C)] {
        fs::write(dir.path().join(name), text).unwrap();
    }
    let (workbench, cx) = open_root(cx, dir.path());
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(300.)));
    cx.update(|window, _| window.activate_window());
    let b = open(&workbench, &dir.path().join("b.txt"), cx);
    let a_editor = open(&workbench, &dir.path().join("a.txt"), cx);
    publish(
        &b,
        vec![
            (
                at(B, "wrong"),
                Diagnostic::new(Severity::Error, "b is wrong"),
            ),
            (
                at(B, "meh"),
                Diagnostic::new(Severity::Hint, "b could be better"),
            ),
        ],
        cx,
    );
    publish(
        &a_editor,
        vec![
            (at(&a, "bad"), Diagnostic::new(Severity::Error, "a is bad")),
            (
                at(&a, "odd"),
                Diagnostic::new(Severity::Warning, "a is odd"),
            ),
        ],
        cx,
    );
    let task = SourceProblem {
        path: dir.path().join("c.txt"),
        problem: Problem {
            range: 0..0,
            row: 1,
            column: 2,
            severity: Severity::Error,
            message: "c is broken".into(),
            source: Some("make".into()),
            related: Vec::new(),
            code_href: None,
        },
        unit: ColumnUnit::Bytes,
        file_column: 2,
    };
    workbench.update(cx, |workbench, cx| {
        let panel = workbench.dock().read(cx).problems().clone();
        panel.update(cx, |panel, _| panel.set_source(7, vec![task]));
        workbench.sync_problems(cx);
    });
    draw(cx);
    (dir, workbench, cx)
}

/// Where `name` is in `text`.
fn at(text: &str, name: &str) -> Range<usize> {
    let start = text.find(name).expect("in the text");
    start..start + name.len()
}

/// Applies `items` as `editor`'s diagnostics, as a server's would be.
fn publish(
    editor: &Entity<EditorView>,
    items: Vec<(Range<usize>, Diagnostic)>,
    cx: &mut VisualTestContext,
) {
    editor.update(cx, |view, cx| {
        let version = view.model().buffer().version();
        let model = view.model_mut();
        assert!(model.apply_diagnostics(1u32, 1u32, version, items, Some(1u32)));
        cx.notify();
    });
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Opens `path` in the active pane, focused: its editor.
fn open(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|window, cx| {
        let focus = pane.read(cx).focus_target(cx);
        window.focus(&focus, cx);
    });
    draw(cx);
    active_editor(workbench, cx)
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("a file is shown")
}

/// Runs command `id` where focus is, in the workbench's context.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The picker's rows: severity, message and `file:row`.
fn rows(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(Severity, String, String)> {
    workbench.read_with(cx, |workbench, cx| {
        let picker = (workbench.diagnostics_picker(cx)).expect("the picker shows");
        picker.read(cx).delegate.labels()
    })
}

fn row(severity: Severity, message: &str, at: &str) -> (Severity, String, String) {
    (severity, message.to_owned(), at.to_owned())
}

fn palette(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Palette> {
    workbench.read_with(cx, |workbench, _| workbench.palette)
}

/// Confirms row `ix`.
fn confirm(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let picker = (workbench.read_with(cx, |workbench, cx| workbench.diagnostics_picker(cx)))
        .expect("the picker shows");
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(ix, window, cx)));
    draw(cx);
}

/// The active editor's file and primary selection, and whether it's focused.
fn landed(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (PathBuf, Range<usize>, bool) {
    let editor = active_editor(workbench, cx);
    let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let path = model.path().unwrap().to_owned();
        (path, model.primary_selection().range, focused)
    })
}

#[gpui::test]
fn document_variant_lists_the_active_file_only(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx);
    run("diagnostics.search_document", cx);
    assert_eq!(palette(&workbench, cx), Some(Palette::Diagnostics));
    assert_eq!(
        rows(&workbench, cx),
        [
            row(Severity::Error, "a is bad", "a.txt:3"),
            row(Severity::Warning, "a is odd", "a.txt:150"),
        ]
    );

    // With `b.txt` shown, its own.
    cx.simulate_keystrokes("escape");
    draw(cx);
    open(&workbench, &dir.path().join("b.txt"), cx);
    run("diagnostics.search_document", cx);
    assert_eq!(
        rows(&workbench, cx),
        [
            row(Severity::Error, "b is wrong", "b.txt:1"),
            row(Severity::Hint, "b could be better", "b.txt:2"),
        ]
    );
}

#[gpui::test]
fn workspace_variant_lists_every_source_errors_first(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    run("diagnostics.search_workspace", cx);
    assert_eq!(palette(&workbench, cx), Some(Palette::Diagnostics));
    // The editors' diagnostics and the task's, errors first, then by file.
    assert_eq!(
        rows(&workbench, cx),
        [
            row(Severity::Error, "a is bad", "a.txt:3"),
            row(Severity::Error, "b is wrong", "b.txt:1"),
            row(Severity::Error, "c is broken", "c.txt:2"),
            row(Severity::Warning, "a is odd", "a.txt:150"),
            row(Severity::Hint, "b could be better", "b.txt:2"),
        ]
    );

    // Typing matches the message and the file.
    let picker = (workbench.read_with(cx, |workbench, cx| workbench.diagnostics_picker(cx)))
        .expect("the picker shows");
    let input = picker.read_with(cx, |picker, _| picker.input().clone());
    input.update(cx, |input, cx| input.set_text("a.t", cx));
    picker.update(cx, |picker, cx| {
        crate::PickerDelegate::update_matches(&mut picker.delegate, "a.t".into(), cx)
    });
    draw(cx);
    assert_eq!(
        rows(&workbench, cx),
        [
            row(Severity::Error, "a is bad", "a.txt:3"),
            row(Severity::Warning, "a is odd", "a.txt:150"),
        ]
    );
}

#[gpui::test]
fn enter_opens_at_the_range(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx);

    // An open file's problem: its editor shown, the range selected.
    run("diagnostics.search_workspace", cx);
    confirm(&workbench, 1, cx);
    assert_eq!(palette(&workbench, cx), None);
    assert_eq!(
        landed(&workbench, cx),
        (dir.path().join("b.txt"), at(B, "wrong"), true)
    );

    // A task's problem in a file no editor shows: opened at its position.
    run("diagnostics.search_workspace", cx);
    confirm(&workbench, 2, cx);
    let c = C.find("broken").unwrap() + 2;
    assert_eq!(
        landed(&workbench, cx),
        (dir.path().join("c.txt"), c..c, true)
    );
}

#[gpui::test]
fn moving_previews_and_dismiss_restores_the_scroll(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    let editor = active_editor(&workbench, cx);
    let state = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            (
                editor.scroll_top(),
                editor.model().primary_selection().range,
            )
        })
    };
    let shows = |row: u32, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.shows_buffer_row(row).expect("drawn"))
    };
    let before = state(cx);
    assert_eq!(before.0, 0.);

    // Opening scrolls nothing: the first row isn't one moved to.
    run("diagnostics.search_document", cx);
    assert_eq!(state(cx), before);

    // Moving to the warning on row 150 shows it; the caret stays.
    let picker = (workbench.read_with(cx, |workbench, cx| workbench.diagnostics_picker(cx)))
        .expect("the picker shows");
    picker.update(cx, |picker, cx| picker.select(1, cx));
    draw(cx);
    assert!(shows(149, cx), "the warning's row is previewed");
    assert!(!shows(0, cx));
    let (top, caret) = state(cx);
    assert!(top > 0., "scrolled");
    assert_eq!(caret, before.1, "a preview leaves the caret");

    // Escape puts the scroll back, focus in the editor.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(palette(&workbench, cx), None);
    assert_eq!(state(cx), before);
    assert!(shows(0, cx));
    let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    assert!(focused);
}
