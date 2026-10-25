//! The References, Problems and Search results lists' vim keys, pressed
//! through the default keymap with vim on: `j` and `k` move the selection,
//! `gg` and `G` go to the first and last row there is to select, and `o`
//! opens the selected row, as Enter does.

use std::{fs, path::PathBuf};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand, set_vim_enabled};
use fe_project::SearchOptions;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, Position, PublishDiagnosticsParams,
    notification::PublishDiagnostics,
};

use crate::{
    SearchPanel, Workbench,
    dock::problems_panel,
    references_panel, search_panel,
    tests::{lsp::Lsp, open_root},
};

/// `files` (relative path, contents) in a temporary directory, and the
/// directory resolved, as the watcher reports it.
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    for (rel, text) in files {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    (dir, root)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Presses `keys`, then lets what they started land.
fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("a file is shown")
}

/// The active editor's file, where its primary selection starts (vim's
/// normal mode leaves the caret there) and whether it's focused.
fn at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, usize, bool) {
    let editor = active_editor(workbench, cx);
    let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let path = model.path().unwrap().to_owned();
        (path, model.primary_selection().range.start, focused)
    })
}

fn focus<V: gpui::Focusable>(view: &Entity<V>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    draw(cx);
}

/// The indices of the rows `selectable` picks out of `rows`.
fn indices<T>(rows: &[T], selectable: impl Fn(&T) -> bool) -> Vec<usize> {
    (rows.iter().enumerate())
        .filter(|(_, row)| selectable(row))
        .map(|(ix, _)| ix)
        .collect()
}

#[gpui::test]
fn references(cx: &mut TestAppContext) {
    use references_panel::ShownRow;

    let main = "mod a;\nmod util;\n\nfn main() {\n    util::helper();\n}\n";
    let a = "pub fn run() {\n    crate::util::helper();\n}\n";
    let util = "pub fn helper() {}\n";
    let (_dir, root) = project(&[
        ("src/a.rs", a),
        ("src/main.rs", main),
        ("src/util.rs", util),
    ]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = root.join("src/main.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = active_editor(&workbench, cx);
    focus(&editor, cx);
    editor.update(cx, |editor, cx| {
        let offset = main.find("helper").unwrap() + 1;
        editor.model_mut().set_cursor(offset);
        cx.notify();
    });

    // Three references, in three files, the panel focused on the first;
    // then vim on.
    keys("shift-f12", cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).references().clone()
    });
    let rows = panel.read_with(cx, |panel, _| panel.shown_rows());
    let refs = indices(&rows, |row| matches!(row, ShownRow::Ref { .. }));
    assert_eq!(refs.len(), 3, "{rows:?}");
    let locations = panel.read_with(cx, |panel, _| panel.locations());
    assert!(cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)));
    let selected = |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.selected());
    assert_eq!(selected(cx), Some(refs[0]));

    // j j k gg G, past the file headers.
    for (key, at) in [("j", 1), ("j", 2), ("k", 1), ("g g", 0), ("shift-g", 2)] {
        keys(key, cx);
        assert_eq!(selected(cx), Some(refs[at]), "after {key}");
    }

    // `o` opens the last reference in the editor, focused, as Enter does.
    let opens = |n: usize, cx: &mut VisualTestContext| {
        let location = &locations[n];
        let (path, selection, focused) = at(&workbench, cx);
        assert_eq!(path, root.join(&location.rel));
        assert_eq!(selection, location.range.start);
        assert!(focused, "the editor has focus");
    };
    keys("o", cx);
    opens(2, cx);
    focus(&panel, cx);
    keys("g g", cx);
    keys("enter", cx);
    opens(0, cx);
    focus(&panel, cx);
    keys("j", cx);
    keys("o", cx);
    opens(1, cx);
}

/// A diagnostic of `len` columns at `line`:`column`.
fn diagnostic(line: u32, column: u32, len: u32, severity: DiagnosticSeverity) -> Diagnostic {
    Diagnostic {
        range: lsp_types::Range::new(
            Position::new(line, column),
            Position::new(line, column + len),
        ),
        severity: Some(severity),
        source: Some("rustc".into()),
        message: format!("problem at {line}:{column}"),
        ..Default::default()
    }
}

/// Has the fake publish `diagnostics` for `name`, at the version it has.
fn publish(s: &Lsp, name: &str, diagnostics: Vec<Diagnostic>) {
    let uri = s.uri(name);
    let version = s.fake.document(&uri).expect("it's open").version;
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: Some(version),
        });
}

#[gpui::test]
fn problems(cx: &mut TestAppContext) {
    use problems_panel::ShownRow;

    // `x` at offset 20 and `y` at 31; `g` at 21.
    let main = "fn main() {\n    let x = 1;\n    y();\n}\n";
    let lib = "pub fn f() {}\npub fn g() {}\n";
    let mut s = Lsp::new(cx);
    fs::write(s.path("src/main.rs"), main).unwrap();
    fs::write(s.path("src/lib.rs"), lib).unwrap();
    for name in ["src/lib.rs", "src/main.rs"] {
        s.open(name);
        let uri = s.uri(name);
        s.fake
            .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    }
    publish(
        &s,
        "src/main.rs",
        vec![
            diagnostic(1, 8, 1, DiagnosticSeverity::WARNING),
            diagnostic(2, 4, 1, DiagnosticSeverity::ERROR),
        ],
    );
    publish(
        &s,
        "src/lib.rs",
        vec![diagnostic(1, 7, 1, DiagnosticSeverity::INFORMATION)],
    );
    let panel = s
        .workbench
        .read_with(&s.cx, |w, cx| w.dock().read(cx).problems().clone());
    let rows = |s: &mut Lsp| panel.read_with(&s.cx, |panel, _| panel.shown_rows());
    s.until("both files listed", |s| rows(s).len() == 5);

    // lib.rs's file row and its problem, then main.rs's and its two.
    let shown = rows(&mut s);
    let problems = indices(&shown, |row| matches!(row, ShownRow::Problem(..)));
    assert_eq!(problems, [1, 3, 4], "{shown:?}");
    s.cx.update(|_, cx| set_vim_enabled(cx, true));
    s.run("problems.show");
    assert!(
        s.cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)),
        "the panel has focus"
    );
    let selected = |s: &mut Lsp| panel.read_with(&s.cx, |panel, _| panel.selected());
    assert_eq!(selected(&mut s), None);

    // j j k gg G, the file rows skipped: gg on lib.rs's problem, not its
    // file's row; G on main.rs's last.
    for (key, at) in [("j", 0), ("j", 1), ("k", 0), ("g g", 0), ("shift-g", 2)] {
        s.cx.simulate_keystrokes(key);
        s.draw();
        assert_eq!(selected(&mut s), Some(problems[at]), "after {key}");
    }

    // `o` opens `y`'s, focused, as Enter does `g`'s.
    let at = |s: &mut Lsp| {
        let editor = (s.workbench)
            .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
            .expect("a tab is shown");
        let focused =
            s.cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
        editor.read_with(&s.cx, |view, _| {
            let model = view.model();
            let path = model.path().unwrap().to_owned();
            (path, model.primary_selection().range.start, focused)
        })
    };
    s.cx.simulate_keystrokes("o");
    s.draw();
    assert_eq!(at(&mut s), (s.path("src/main.rs"), 31, true));
    s.run("problems.show");
    s.cx.simulate_keystrokes("g g enter");
    s.draw();
    assert_eq!(at(&mut s), (s.path("src/lib.rs"), 21, true));
    s.run("problems.show");
    s.cx.simulate_keystrokes("shift-g k o");
    s.draw();
    assert_eq!(at(&mut s), (s.path("src/main.rs"), 20, true));
}

/// Runs `command` where focus is, as a key would.
fn run(command: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let context: ContextStack = window
            .context_stack()
            .iter()
            .map(|context| ContextFrame::new(context.primary().map_or("", |entry| &entry.key)))
            .collect();
        let run = RunCommand(Invocation::of(command).unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
    draw(cx);
}

#[gpui::test]
fn search(cx: &mut TestAppContext) {
    use search_panel::ShownRow;

    let (_dir, root) = project(&[
        ("a.txt", "foo a\nx\nfoo b\n"),
        ("b.txt", "foo c\n"),
        ("sub/c.txt", "foo d\n"),
    ]);
    let (workbench, cx) = open_root(cx, &root);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let search: Entity<SearchPanel> = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    search.update(cx, |search, _| {
        search.set_options(SearchOptions {
            threads: 1,
            ..SearchOptions::default()
        })
    });
    keys("cmd-shift-f", cx);
    let input = search.read_with(cx, |search, _| search.input().clone());
    input.update(cx, |input, cx| input.insert("foo", cx));
    search.update(cx, |search, cx| search.run(cx));
    draw(cx);
    let rows = |cx: &mut VisualTestContext| search.read_with(cx, |search, _| search.shown_rows());
    let file = |rel: &str, matches| ShownRow::File(rel.to_owned(), matches);
    let line = |row: &ShownRow| matches!(row, ShownRow::Line(..));
    let shown = rows(cx);
    assert_eq!(shown.len(), 7, "{shown:?}");
    assert_eq!(
        [&shown[0], &shown[3], &shown[5]],
        [&file("a.txt", 2), &file("b.txt", 1), &file("sub/c.txt", 1)]
    );
    assert_eq!(indices(&shown, line), [1, 2, 4, 6]);

    // Down from the query focuses the results on the first line; j and k
    // go over files' rows and lines' alike.
    keys("down", cx);
    assert!(cx.update(|window, cx| search.read(cx).results_focused(window)));
    let selected = |cx: &mut VisualTestContext| search.read_with(cx, |search, _| search.selected());
    assert_eq!(selected(cx), Some(1));
    for (key, at) in [("j", 2), ("j", 3), ("k", 2), ("g g", 0), ("shift-g", 6)] {
        keys(key, cx);
        assert_eq!(selected(cx), Some(at), "after {key}");
    }

    // sub/c.txt collapsed, G lands on its row, the last shown, its line
    // hidden.
    keys("left", cx);
    assert_eq!(selected(cx), Some(5));
    keys("left", cx);
    assert_eq!(rows(cx).len(), 6);
    keys("g g", cx);
    keys("shift-g", cx);
    assert_eq!(selected(cx), Some(5));

    // `o` on a line opens it, focused, as Enter does; on a file's row, its
    // first line.
    let opens = |rel: &str, offset: usize, cx: &mut VisualTestContext| {
        let (path, selection, focused) = at(&workbench, cx);
        assert_eq!(path, root.join(rel));
        assert_eq!(selection, offset);
        assert!(focused, "the editor has focus");
    };
    // Back to the results: showing search focuses the query, and down
    // the results, the selection kept.
    let results = |cx: &mut VisualTestContext| {
        keys("cmd-shift-f", cx);
        keys("down", cx);
    };
    keys("g g j j o", cx);
    opens("a.txt", 8, cx);
    results(cx);
    assert_eq!(selected(cx), Some(2));
    keys("enter", cx);
    opens("a.txt", 8, cx);
    results(cx);
    keys("shift-g o", cx);
    opens("sub/c.txt", 0, cx);

    // In the tree, `o` on a folder's row collapses it, and expands it
    // again, as Enter does.
    results(cx);
    run("search.view_as_tree", cx);
    let shown = rows(cx);
    assert_eq!(shown[0], ShownRow::Folder("sub".to_owned(), 1), "{shown:?}");
    let len = shown.len();
    keys("g g", cx);
    assert_eq!(selected(cx), Some(0));
    keys("o", cx);
    assert_eq!(rows(cx).len(), len - 1, "{:?}", rows(cx));
    assert_eq!(selected(cx), Some(0));
    keys("o", cx);
    assert_eq!(rows(cx).len(), len);
    keys("enter", cx);
    assert_eq!(rows(cx).len(), len - 1);
    keys("o", cx);
    assert_eq!(rows(cx).len(), len);

    // G on the last line, which `o` opens.
    keys("shift-g", cx);
    assert_eq!(selected(cx), Some(len - 1));
    assert!(line(&rows(cx)[len - 1]));
    keys("o", cx);
    opens("b.txt", 0, cx);
}
