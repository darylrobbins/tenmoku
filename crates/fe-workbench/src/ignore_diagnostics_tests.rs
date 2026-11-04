//! Ignore-file checks in a real workbench, on a paused clock: the walk is the
//! real one, over a temp dir, wrapped to record what ran and where.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::Ordering},
};

use fe_buffer::BufferId;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::diagnostics::{DiagnosticTag, Problem, Severity};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_project::{
    FsChange, IgnoreFinding, IgnoreFindingKind, IgnoreReport, IgnoreSeverity, check_ignore_file,
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use super::{DEBOUNCE, IgnoreSource};
use crate::{Launch, Workbench, dock::problems_panel::ShownRow};

/// A temp dir, its path as it's opened.
struct Files(TempDir);

impl Files {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.path().join(name)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path(name);
        fs::write(&path, text).unwrap();
        path
    }
}

/// Whether each check run so far ran on the main thread.
#[derive(Clone, Default)]
struct Checks(Arc<Mutex<Vec<bool>>>);

impl Checks {
    fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    fn any_on_main_thread(&self) -> bool {
        self.0.lock().unwrap().iter().any(|main| *main)
    }
}

fn workbench(cx: &mut TestAppContext) -> (Entity<Workbench>, Checks, &mut VisualTestContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let checks = Checks::default();
    let recorded = checks.clone();
    let executor = cx.executor();
    workbench.update(cx, |workbench, _| {
        workbench.set_ignore_check(Arc::new(move |path, text, cancel| {
            recorded.0.lock().unwrap().push(executor.is_main_thread());
            check_ignore_file(path, text, cancel)
        }));
    });
    (workbench, checks, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Past the debounce, with whatever it started finished and shown.
fn settle(cx: &mut VisualTestContext) {
    draw(cx);
    cx.executor().advance_clock(DEBOUNCE);
    draw(cx);
}

/// Shows `path` in the active pane, without waiting out the debounce.
fn open(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown")
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The file watcher reports `changes`, by the way it reaches the workbench.
fn tree_changes(workbench: &Entity<Workbench>, changes: &[FsChange], cx: &mut VisualTestContext) {
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    panel.update(cx, |panel, cx| panel.apply_fs_changes(changes, cx));
    draw(cx);
}

fn problems(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Problem> {
    editor.read_with(cx, |view, _| view.model().problems())
}

/// `editor`'s problems as `(row, severity)`.
fn severities(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, Severity)> {
    let problems = problems(editor, cx);
    (problems.iter())
        .map(|problem| (problem.row, problem.severity))
        .collect()
}

fn panel_rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<ShownRow> {
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).problems().clone()
    });
    panel.read_with(cx, |panel, _| panel.shown_rows())
}

fn buffer_id(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> BufferId {
    editor.read_with(cx, |view, _| view.model().buffer().id())
}

/// Adds a comment at the end of `editor`'s text, which leaves what's matched
/// as it was.
fn add_comment(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    editor.update(cx, |view, cx| {
        let end = view.model().buffer().len();
        view.model_mut().select_range(end..end, false);
        view.model_mut().insert("# a comment\n");
        cx.notify();
    });
    draw(cx);
}

#[gpui::test]
fn unused_pattern_is_dimmed_and_listed(cx: &mut TestAppContext) {
    let (workbench, _, cx) = workbench(cx);
    let files = Files::new();
    files.write("used.log", "");
    let path = files.write(".gitignore", "used.log\nmissing.log\n");
    let editor = open(&workbench, &path, cx);
    assert_eq!(problems(&editor, cx), [], "not before the debounce");
    settle(cx);

    let tags: Vec<Vec<DiagnosticTag>> = editor.read_with(cx, |view, _| {
        (view.model().diagnostics().iter())
            .map(|entry| entry.tags.clone())
            .collect()
    });
    assert_eq!(tags, [vec![DiagnosticTag::Unnecessary]]);
    let found = problems(&editor, cx);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].severity, Severity::Hint);
    assert_eq!(found[0].row, 1);
    assert_eq!(
        found[0].range,
        9..20,
        "the pattern, without its line ending"
    );
    assert_eq!(found[0].source.as_deref(), Some("ignore"));

    let rows = panel_rows(&workbench, cx);
    assert_eq!(rows.len(), 2, "{rows:?}");
    let ShownRow::Problem(mark, message, source, at) = &rows[1] else {
        panic!("not a problem: {rows:?}");
    };
    assert_eq!(*mark, "H");
    assert!(message.contains("matches nothing"), "{message}");
    assert_eq!(source.as_deref(), Some("ignore"));
    assert_eq!(at, "2:1");
}

#[gpui::test]
fn duplicate_shadowed_and_invalid_have_severities(cx: &mut TestAppContext) {
    let (workbench, _, cx) = workbench(cx);
    let files = Files::new();
    files.write("a.log", "");
    files.write("keep.txt", "");
    let text = "a.log\na.log\n*.txt\n!keep.txt\n[z\n";
    let path = files.write(".gitignore", text);
    let editor = open(&workbench, &path, cx);
    settle(cx);

    assert_eq!(
        severities(&editor, cx),
        [
            (1, Severity::Warning),
            (2, Severity::Warning),
            (4, Severity::Error),
        ]
    );
    let found = problems(&editor, cx);
    assert!(found[0].message.contains("line 1"), "{:?}", found[0]);
    assert!(found[1].message.contains("line 4"), "{:?}", found[1]);
    assert!(found[2].message.contains("unclosed"), "{:?}", found[2]);
    assert!(
        found
            .iter()
            .all(|it| it.source.as_deref() == Some("ignore")),
        "{found:?}"
    );
    // A file row and its three.
    assert_eq!(panel_rows(&workbench, cx).len(), 4);
    // Only an unused pattern is dimmed.
    let tagged = editor.read_with(cx, |view, _| {
        (view.model().diagnostics().iter()).any(|entry| !entry.tags.is_empty())
    });
    assert!(!tagged);
}

#[gpui::test]
fn save_and_tree_change_rerun_the_check(cx: &mut TestAppContext) {
    let (workbench, checks, cx) = workbench(cx);
    let files = Files::new();
    let path = files.write(".gitignore", "*.log\n");
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);
    assert_eq!(checks.count(), 1);

    // The file alone says nothing: only the watcher's word starts a check.
    let log = files.write("a.log", "");
    settle(cx);
    assert_eq!(checks.count(), 1);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);

    tree_changes(&workbench, &[FsChange::Path(log.clone())], cx);
    assert_eq!(checks.count(), 1, "not before the debounce");
    settle(cx);
    assert_eq!(checks.count(), 2);
    assert_eq!(severities(&editor, cx), []);
    assert_eq!(panel_rows(&workbench, cx), []);

    // Gone again, it's unused again, on a rescan too.
    fs::remove_file(&log).unwrap();
    tree_changes(&workbench, &[FsChange::Rescan], cx);
    settle(cx);
    assert_eq!(checks.count(), 3);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);

    // Creating the file, then saving, clears it, with no word from a watcher.
    add_comment(&editor, cx);
    settle(cx);
    assert_eq!(checks.count(), 3, "an edit starts no check");
    files.write("b.log", "");
    run("editor.save", cx);
    settle(cx);
    assert!(fs::read_to_string(&path).unwrap().contains("# a comment"));
    assert_eq!(checks.count(), 4);
    assert_eq!(severities(&editor, cx), []);

    assert!(!checks.any_on_main_thread(), "the walk stays off the UI");
}

#[gpui::test]
fn a_superseded_check_is_stopped_and_its_result_dropped(cx: &mut TestAppContext) {
    let (workbench, checks, cx) = workbench(cx);
    let files = Files::new();
    let path = files.write(".gitignore", "*.log\n");
    let log = files.write("a.log", "");
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(severities(&editor, cx), []);
    assert_eq!(checks.count(), 1);

    let id = buffer_id(&editor, cx);
    let newest = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, _| workbench.newest_ignore_check(id))
    };
    let (stale, stopped) = newest(cx).expect("it has a slot");
    assert!(!stopped.load(Ordering::Relaxed));
    tree_changes(&workbench, &[FsChange::Path(log.clone())], cx);
    assert!(
        stopped.load(Ordering::Relaxed),
        "the older walk is told to stop"
    );
    let (current, stops) = newest(cx).expect("it has a slot");
    assert_ne!(stale, current);
    assert!(!stops.load(Ordering::Relaxed));

    // The older check lands, with something that isn't so any more.
    let report = IgnoreReport {
        findings: vec![IgnoreFinding {
            line: 1,
            range: 0..5,
            kind: IgnoreFindingKind::Unused,
            severity: IgnoreSeverity::Hint,
        }],
        ..IgnoreReport::default()
    };
    let version = editor.read_with(cx, |view, _| view.model().buffer().version());
    workbench.update(cx, |workbench, cx| {
        workbench.ignore_check_landed(id, stale, version, report.clone(), cx);
    });
    draw(cx);
    assert_eq!(severities(&editor, cx), [], "a stale result is dropped");

    // A newer one, cancelled, isn't shown either.
    workbench.update(cx, |workbench, cx| {
        let report = IgnoreReport {
            cancelled: true,
            ..report
        };
        workbench.ignore_check_landed(id, current, version, report, cx);
    });
    draw(cx);
    assert_eq!(severities(&editor, cx), []);

    // Triggers close together make one check.
    tree_changes(&workbench, &[FsChange::Path(log.clone())], cx);
    cx.executor().advance_clock(DEBOUNCE / 2);
    tree_changes(&workbench, &[FsChange::Path(log)], cx);
    cx.executor().advance_clock(DEBOUNCE / 2);
    draw(cx);
    assert_eq!(checks.count(), 1, "the wait starts again");
    settle(cx);
    assert_eq!(checks.count(), 2);
}

#[gpui::test]
fn git_internals_changing_starts_no_check(cx: &mut TestAppContext) {
    let (workbench, checks, cx) = workbench(cx);
    let files = Files::new();
    let path = files.write(".gitignore", "*.log\n");
    let _editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(checks.count(), 1);
    let index = files.path(".git/index.lock");
    tree_changes(&workbench, &[FsChange::Path(index)], cx);
    settle(cx);
    assert_eq!(checks.count(), 1);
}

/// The text the check sees is the buffer's as it is, saved or not, and what it
/// finds lands on the text that has moved on since.
#[gpui::test]
fn unsaved_text_is_checked_and_edits_during_a_check_rebase(cx: &mut TestAppContext) {
    let (workbench, _, cx) = workbench(cx);
    let files = Files::new();
    let path = files.write(".gitignore", "*.log\n");
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(problems(&editor, cx)[0].range, 0..5);

    add_comment(&editor, cx);
    tree_changes(&workbench, &[FsChange::Rescan], cx);
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);

    // Found in text an edit has since moved: as a check finishing late would.
    let id = buffer_id(&editor, cx);
    let (generation, _) = workbench
        .read_with(cx, |workbench, _| workbench.newest_ignore_check(id))
        .expect("it has a slot");
    let version = editor.read_with(cx, |view, _| view.model().buffer().version());
    editor.update(cx, |view, cx| {
        view.model_mut().select_range(0..0, false);
        view.model_mut().insert("# first\n");
        cx.notify();
    });
    let report = IgnoreReport {
        findings: vec![IgnoreFinding {
            line: 1,
            range: 0..5,
            kind: IgnoreFindingKind::Unused,
            severity: IgnoreSeverity::Hint,
        }],
        ..IgnoreReport::default()
    };
    workbench.update(cx, |workbench, cx| {
        workbench.ignore_check_landed(id, generation, version, report, cx);
    });
    draw(cx);
    let found = problems(&editor, cx);
    assert_eq!(found[0].row, 1, "{found:?}");
    assert_eq!(found[0].range, 8..13, "{found:?}");
}

#[gpui::test]
fn dockerignore_and_other_languages_get_none(cx: &mut TestAppContext) {
    let (workbench, checks, cx) = workbench(cx);
    let files = Files::new();
    let docker = files.write(".dockerignore", "missing.log\nmissing.log\n");
    let rust = files.write("main.rs", "fn main() {}\n");
    for path in [&docker, &rust] {
        let editor = open(&workbench, path, cx);
        settle(cx);
        assert_eq!(problems(&editor, cx), [], "{path:?}");
    }
    assert_eq!(checks.count(), 0);
    assert_eq!(workbench.read_with(cx, |it, _| it.ignore_slot_count()), 0);
    assert_eq!(panel_rows(&workbench, cx), []);

    // Closing the file clears what it had, and what it was waiting for.
    let path = files.write(".gitignore", "missing.log\n");
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);
    assert_eq!(panel_rows(&workbench, cx).len(), 2);
    let id = buffer_id(&editor, cx);
    tree_changes(&workbench, &[FsChange::Rescan], cx);
    let (_, stopped) = workbench
        .read_with(cx, |workbench, _| workbench.newest_ignore_check(id))
        .expect("it has a slot");
    run("workbench.close_tab", cx);
    settle(cx);
    assert_eq!(panel_rows(&workbench, cx), []);
    assert_eq!(workbench.read_with(cx, |it, _| it.ignore_slot_count()), 0);
    assert!(stopped.load(Ordering::Relaxed));
    assert_eq!(checks.count(), 1, "the check it was waiting for never ran");
}

#[gpui::test]
fn renaming_away_or_picking_another_language_clears(cx: &mut TestAppContext) {
    let (workbench, _, cx) = workbench(cx);
    let files = Files::new();
    let path = files.write(".gitignore", "missing.log\n");
    let editor = open(&workbench, &path, cx);
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);

    // Another language by hand, and back.
    editor.update(cx, |view, cx| view.set_language(None, cx));
    settle(cx);
    assert_eq!(severities(&editor, cx), []);
    assert_eq!(workbench.read_with(cx, |it, _| it.ignore_slot_count()), 0);
    editor.update(cx, |view, cx| view.set_language(Some("gitignore"), cx));
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);
    let chosen = editor.read_with(cx, |view, _| view.is_language_chosen());
    assert!(chosen, "picked by hand");

    // Renamed away, it's a file like any other.
    let notes = files.path("notes.txt");
    fs::rename(&path, &notes).unwrap();
    workbench.update(cx, |workbench, cx| {
        workbench.follow_rename(&path, &notes, cx)
    });
    settle(cx);
    assert_eq!(severities(&editor, cx), []);
    assert_eq!(panel_rows(&workbench, cx), []);
    assert_eq!(workbench.read_with(cx, |it, _| it.ignore_slot_count()), 0);

    // Renamed to one, it's checked.
    let back = files.path(".npmignore");
    fs::rename(&notes, &back).unwrap();
    workbench.update(cx, |workbench, cx| {
        workbench.follow_rename(&notes, &back, cx)
    });
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);
}

/// A hidden tab has no frames of its own, but its file is listed all the same.
#[gpui::test]
fn a_hidden_tab_and_a_split_get_them_too(cx: &mut TestAppContext) {
    let (workbench, _, cx) = workbench(cx);
    let files = Files::new();
    let path = files.write(".gitignore", "missing.log\n");
    let other = files.write("notes.txt", "notes\n");
    let editor = open(&workbench, &path, cx);
    let shown = open(&workbench, &other, cx);
    assert_ne!(editor, shown);
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);
    assert_eq!(panel_rows(&workbench, cx).len(), 2);

    let editor = open(&workbench, &path, cx);
    run("pane.split_right", cx);
    let split = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the split shows it");
    assert_ne!(editor, split);
    assert_eq!(severities(&split, cx), [(0, Severity::Hint)], "copied");

    // Both follow the next check, as one row.
    files.write("missing.log", "");
    tree_changes(&workbench, &[FsChange::Rescan], cx);
    settle(cx);
    assert_eq!(severities(&editor, cx), []);
    assert_eq!(severities(&split, cx), []);
    assert_eq!(panel_rows(&workbench, cx), []);
}

/// What a language server published stays: a set is keyed by one pair of
/// types, so showing a check's would lose it.
#[gpui::test]
fn another_owners_diagnostics_are_left_alone(cx: &mut TestAppContext) {
    let (workbench, _, cx) = workbench(cx);
    let files = Files::new();
    let path = files.write(".gitignore", "missing.log\n");
    let editor = open(&workbench, &path, cx);
    let publish = |items: Vec<_>, cx: &mut VisualTestContext| {
        editor.update(cx, |view, cx| {
            let version = view.model().buffer().version();
            let model = view.model_mut();
            assert!(model.apply_diagnostics(1u32, 1u32, version, items, Some(1u32)));
            cx.notify();
        });
        draw(cx);
    };
    let server = fe_editor::diagnostics::Diagnostic::new(Severity::Error, "from a server");
    publish(vec![(0..7, server)], cx);
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Error)]);
    tree_changes(&workbench, &[FsChange::Rescan], cx);
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Error)]);

    // The server has nothing more to say: the check's come back.
    publish(Vec::new(), cx);
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);
    let own = editor.read_with(cx, |view, _| {
        (view.model())
            .diagnostic_set::<IgnoreSource, u64>()
            .is_some()
    });
    assert!(own);

    // Reset, as for a new root's servers: they come back too.
    editor.update(cx, |view, cx| {
        view.model_mut().reset_diagnostics();
        cx.notify();
    });
    settle(cx);
    assert_eq!(severities(&editor, cx), [(0, Severity::Hint)]);
}

/// 5,000 files: opening the file returns at once, and the walk happens off the
/// main thread, its result arriving later.
#[gpui::test]
fn a_big_tree_leaves_the_ui_thread_free(cx: &mut TestAppContext) {
    let (workbench, checks, cx) = workbench(cx);
    let files = Files::new();
    for at in 0..5_000 {
        files.write(&format!("file{at}.txt"), "");
    }
    let path = files.write(".gitignore", "file4999.txt\nmissing.log\n");
    let editor = open(&workbench, &path, cx);
    assert_eq!(checks.count(), 0, "not on the way to the first frame");
    assert_eq!(problems(&editor, cx), []);
    settle(cx);
    assert_eq!(checks.count(), 1);
    assert!(!checks.any_on_main_thread());
    assert_eq!(severities(&editor, cx), [(1, Severity::Hint)]);
}

/// Splits can be in different languages: the one still an ignore file keeps its
/// row, however the panes are ordered, and the other one stays clear.
#[gpui::test]
fn a_split_in_another_language_keeps_the_row(cx: &mut TestAppContext) {
    let (workbench, _, cx) = workbench(cx);
    let files = Files::new();
    let path = files.write(".gitignore", "missing.log\n");
    let first = open(&workbench, &path, cx);
    settle(cx);
    run("pane.split_right", cx);
    let second = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the split shows it");
    assert_ne!(first, second);
    assert_eq!(panel_rows(&workbench, cx).len(), 2);

    // The first pane's, which the list comes to first.
    first.update(cx, |view, cx| view.set_language(None, cx));
    settle(cx);
    assert_eq!(severities(&first, cx), []);
    assert_eq!(severities(&second, cx), [(0, Severity::Hint)]);
    let rows = panel_rows(&workbench, cx);
    assert_eq!(rows.len(), 2, "one file, one problem: {rows:?}");

    // Back: both hold it, listed once.
    first.update(cx, |view, cx| view.set_language(Some("gitignore"), cx));
    settle(cx);
    assert_eq!(severities(&first, cx), [(0, Severity::Hint)]);
    assert_eq!(severities(&second, cx), [(0, Severity::Hint)]);
    assert_eq!(panel_rows(&workbench, cx).len(), 2);

    // The second pane's.
    second.update(cx, |view, cx| view.set_language(None, cx));
    settle(cx);
    assert_eq!(severities(&first, cx), [(0, Severity::Hint)]);
    assert_eq!(severities(&second, cx), []);
    assert_eq!(panel_rows(&workbench, cx).len(), 2);

    // Neither: nothing's left, and no check is kept for it.
    first.update(cx, |view, cx| view.set_language(None, cx));
    settle(cx);
    assert_eq!(severities(&first, cx), []);
    assert_eq!(panel_rows(&workbench, cx), []);
    assert_eq!(workbench.read_with(cx, |it, _| it.ignore_slot_count()), 0);
}
