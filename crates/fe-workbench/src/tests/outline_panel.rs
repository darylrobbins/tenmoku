use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::open_root;
use crate::{DockPanel, OutlinePanel, Workbench};

const RUST: &str = "\
struct Point;

impl Point {
    fn new() -> Self {
        Point
    }
}

fn main() {}
";

const MARKDOWN: &str = "# Title\n\nText.\n\n## Section\n";

/// `files` (name, contents) in a temporary directory, and their paths.
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let paths = files
        .iter()
        .map(|(name, text)| {
            let path = dir.path().join(name);
            fs::write(&path, text).unwrap();
            path
        })
        .collect();
    (dir, paths)
}

/// A workbench rooted at `dir`, focused as the app focuses it, with the
/// outline shown.
fn outlining<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
) -> (
    Entity<Workbench>,
    Entity<OutlinePanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-o");
    let outline = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().clone()
    });
    (workbench, outline, cx)
}

/// Opens `path` in a tab, focused, and lets its outline land.
fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    cx.run_until_parked();
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

fn rows(outline: &Entity<OutlinePanel>, cx: &mut VisualTestContext) -> Vec<(u32, String)> {
    outline.read_with(cx, |outline, _| outline.rows())
}

fn message(outline: &Entity<OutlinePanel>, cx: &mut VisualTestContext) -> Option<String> {
    outline.read_with(cx, |outline, _| outline.message_text())
}

/// `fe_syntax::outline` of the editor's text, as `(depth, name)` rows.
fn expected(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, String)> {
    editor.read_with(cx, |editor, _| {
        let outline = fe_syntax::outline(editor.syntax().expect("parsed")).expect("an outline");
        outline
            .items
            .into_iter()
            .map(|item| (item.depth, item.name))
            .collect()
    })
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// Inserts `text` at `offset`, as typing does, without running the executor.
fn edit(editor: &Entity<EditorView>, offset: usize, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(offset);
        editor.model_mut().insert(text);
        cx.notify();
    });
}

fn jobs(outline: &Entity<OutlinePanel>, cx: &mut VisualTestContext) -> (usize, (usize, usize)) {
    outline.read_with(cx, |outline, _| {
        (outline.jobs_started(), outline.jobs_ran())
    })
}

#[gpui::test]
fn shows_the_active_tabs_outline(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.rs", RUST), ("b.md", MARKDOWN), ("c.txt", "text\n")]);
    let (workbench, outline, cx) = outlining(cx, &dir);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    assert_eq!(
        dock.read_with(cx, |dock, _| (dock.is_visible(), dock.active())),
        (true, DockPanel::Explorer)
    );

    open(&workbench, &paths[0], cx);
    let rust = active_editor(&workbench, cx);
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
    assert_eq!(shown, expected(&rust, cx));
    assert_eq!(message(&outline, cx), None);

    // cmd-shift-o from the editor shows the outline, in the Explorer,
    // focused for its keys.
    cx.simulate_keystrokes("cmd-shift-f");
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.active()),
        DockPanel::Search
    );
    cx.simulate_keystrokes("cmd-shift-o");
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.active()),
        DockPanel::Explorer
    );
    assert!(cx.update(|window, cx| outline.focus_handle(cx).is_focused(window)));

    open(&workbench, &paths[1], cx);
    let markdown = active_editor(&workbench, cx);
    let headings = [(0, "Title".to_string()), (1, "Section".to_string())];
    assert_eq!(rows(&outline, cx), headings);
    assert_eq!(rows(&outline, cx), expected(&markdown, cx));

    // Back to the Rust tab, edited: its job lands once a plain-text tab is
    // active, and is dropped.
    open(&workbench, &paths[2], cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let activate = |ix: usize, cx: &mut VisualTestContext| {
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
    };
    activate(0, cx);
    cx.run_until_parked();
    assert_eq!(rows(&outline, cx), expected(&rust, cx));
    edit(&rust, RUST.len(), "fn more() {}\n", cx);
    let started = jobs(&outline, cx).0;
    activate(2, cx);
    cx.run_until_parked();
    assert_eq!(jobs(&outline, cx).0, started, "nothing more to outline");
    assert!(outline.read_with(cx, |outline, _| outline.outline().is_none()));
    assert_eq!(
        message(&outline, cx).as_deref(),
        Some("No outline for plain text")
    );

    activate(1, cx);
    cx.run_until_parked();
    assert_eq!(rows(&outline, cx), headings);
    assert!(focused(&markdown, cx));
}

#[gpui::test]
fn refreshes_after_edits_off_the_main_thread(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.rs", RUST)]);
    let (workbench, outline, cx) = outlining(cx, &dir);
    open(&workbench, &paths[0], cx);
    let editor = active_editor(&workbench, cx);
    ten_edits_refresh(&editor, &outline, "f", cx);

    // Hidden, the panel doesn't render, so only the follow-up job catches
    // the edits made while the first ran.
    cx.simulate_keystrokes("cmd-shift-e");
    ten_edits_refresh(&editor, &outline, "g", cx);
}

/// Makes 10 edits, each adding `fn {prefix}N`, then runs the executor: at
/// most 2 jobs start, they run on the background executor, and the rows are
/// then the final text's outline.
fn ten_edits_refresh(
    editor: &Entity<EditorView>,
    outline: &Entity<OutlinePanel>,
    prefix: &str,
    cx: &mut VisualTestContext,
) {
    let before = jobs(outline, cx);
    for ix in 0..10 {
        let end = editor.read_with(cx, |editor, _| editor.model().text().len());
        edit(editor, end, &format!("fn {prefix}{ix}() {{}}\n"), cx);
    }
    assert_eq!(jobs(outline, cx).1, before.1, "no job ran yet");
    cx.run_until_parked();

    let (started, (ran, off_main)) = jobs(outline, cx);
    let started = started - before.0;
    assert!((1..=2).contains(&started), "{started} jobs");
    assert_eq!(ran - before.1.0, started);
    assert_eq!(off_main - before.1.1, started, "on the background executor");
    let shown = rows(outline, cx);
    assert_eq!(shown, expected(editor, cx));
    assert_eq!(shown.last(), Some(&(0, format!("{prefix}9"))));
    let version = editor.read_with(cx, |editor, _| editor.model().snapshot().version());
    let outlined = outline.read_with(cx, |outline, _| outline.outline().unwrap().version);
    assert_eq!(outlined, version);
}

#[gpui::test]
fn refreshes_while_hidden(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.rs", RUST), ("b.md", MARKDOWN)]);
    let (workbench, outline, cx) = outlining(cx, &dir);
    open(&workbench, &paths[0], cx);
    let editor = active_editor(&workbench, cx);
    cx.simulate_keystrokes("cmd-shift-e");
    let caught_up = |last: &str, cx: &mut VisualTestContext| {
        let shown = rows(&outline, cx);
        assert_eq!(shown, expected(&editor, cx));
        assert_eq!(shown.last(), Some(&(0, last.to_string())));
        let version = editor.read_with(cx, |editor, _| editor.model().snapshot().version());
        let outlined = outline.read_with(cx, |outline, _| outline.outline().unwrap().version);
        assert_eq!(outlined, version);
    };

    // The editor reparses this edit as it renders, notifying no one.
    edit(&editor, RUST.len(), "fn one() {}\n", cx);
    cx.run_until_parked();
    caught_up("one", cx);

    // Two edits in one update.
    editor.update(cx, |editor, cx| {
        for name in ["two", "three"] {
            let end = editor.model().text().len();
            editor.model_mut().set_cursor(end);
            editor.model_mut().insert(&format!("fn {name}() {{}}\n"));
        }
        cx.notify();
    });
    cx.run_until_parked();
    caught_up("three", cx);

    // A tab opened meanwhile, of a language loaded already.
    open(&workbench, &paths[1], cx);
    assert_eq!(
        rows(&outline, cx),
        [(0, "Title".to_string()), (1, "Section".to_string())]
    );
}

#[gpui::test]
fn keeps_the_last_outline_while_reparsing(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.rs", RUST)]);
    let (workbench, outline, cx) = outlining(cx, &dir);
    open(&workbench, &paths[0], cx);
    let editor = active_editor(&workbench, cx);
    let previous = rows(&outline, cx);
    assert!(!previous.is_empty());

    // Renaming `main` to `xmain`.
    let main = RUST.find("main").unwrap();
    edit(&editor, main, "x", cx);
    assert_eq!(rows(&outline, cx), previous);
    assert_eq!(message(&outline, cx), None);

    cx.run_until_parked();
    assert_eq!(rows(&outline, cx).last(), Some(&(0, "xmain".to_string())));
}

#[gpui::test]
fn clicking_an_item_jumps(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.rs", RUST)]);
    let (workbench, outline, cx) = outlining(cx, &dir);
    open(&workbench, &paths[0], cx);
    let editor = active_editor(&workbench, cx);
    cx.update(|window, cx| window.blur(cx));
    assert!(!focused(&editor, cx));

    // `new`, the third row. Rows are only as wide as their text: click near
    // the row's left edge.
    let name = outline.read_with(cx, |outline, _| {
        outline.outline().unwrap().items[2].name_range.clone()
    });
    assert_eq!(&RUST[name.clone()], "new");
    let bounds = outline.read_with(cx, |outline, _| outline.row_bounds(2).expect("drawn"));
    cx.simulate_click(
        bounds.origin + gpui::point(gpui::px(30.), bounds.size.height / 2.),
        Modifiers::none(),
    );

    assert!(focused(&editor, cx));
    let selections: Vec<Range<usize>> = editor.read_with(cx, |editor, _| {
        editor
            .model()
            .selections()
            .into_iter()
            .map(|selection| selection.range)
            .collect()
    });
    assert_eq!(selections, vec![name.start..name.start]);
}

#[gpui::test]
fn empty_and_too_large_states(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[
        ("notes.txt", "fn not_code() {}\n"),
        ("page.html", "<p>a</p>\n"),
        ("empty.rs", ""),
    ]);
    let (workbench, outline, cx) = outlining(cx, &dir);
    assert_eq!(message(&outline, cx).as_deref(), Some("No file open"));

    let shown = |path: &PathBuf, cx: &mut VisualTestContext| {
        open(&workbench, path, cx);
        assert!(rows(&outline, cx).is_empty());
        message(&outline, cx)
    };
    assert_eq!(
        shown(&paths[0], cx).as_deref(),
        Some("No outline for plain text")
    );
    assert_eq!(shown(&paths[1], cx).as_deref(), Some("No outline for html"));
    assert_eq!(shown(&paths[2], cx).as_deref(), Some("No symbols"));

    // Over Rust's parse limit; built in memory rather than written out.
    let limit = fe_syntax::language_by_name("rust").unwrap().parse_limit();
    let big = "fn f() {}\n".repeat(limit / 10 + 1);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = dir.path().join("big.rs");
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.opening(path.clone(), cx);
            pane.file_read(path, Ok(Buffer::new(&big).into()), window, cx)
        })
    });
    cx.run_until_parked();
    assert!(rows(&outline, cx).is_empty());
    assert_eq!(
        message(&outline, cx).as_deref(),
        Some("File too large for an outline")
    );
}

#[gpui::test]
fn parsing_shows_only_while_the_editors_language_loads(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[
        ("notes.txt", "text\n"),
        ("a.rs", RUST),
        ("script.txt", "def f():\n    pass\n"),
    ]);
    let (workbench, outline, cx) = outlining(cx, &dir);
    let plain = Some("No outline for plain text");

    // A shebang typed into an open plain-text file doesn't change its
    // language: the view detects one only as it opens.
    open(&workbench, &paths[0], cx);
    let notes = active_editor(&workbench, cx);
    edit(&notes, 0, "#!/usr/bin/env python3\n", cx);
    assert_eq!(message(&outline, cx).as_deref(), plain);
    cx.run_until_parked();
    assert_eq!(message(&outline, cx).as_deref(), plain);
    assert!(!notes.read_with(cx, |notes, _| notes.is_loading_language()));

    // A Rust file set to plain text.
    open(&workbench, &paths[1], cx);
    let rust = active_editor(&workbench, cx);
    assert!(!rows(&outline, cx).is_empty());
    rust.update(cx, |rust, cx| rust.set_language(None, cx));
    cx.run_until_parked();
    assert!(rows(&outline, cx).is_empty());
    assert_eq!(message(&outline, cx).as_deref(), plain);

    // A plain-text file set to Python: parsing until its outline lands.
    open(&workbench, &paths[2], cx);
    let script = active_editor(&workbench, cx);
    assert_eq!(message(&outline, cx).as_deref(), plain);
    script.update(cx, |script, cx| script.set_language(Some("python"), cx));
    assert_eq!(message(&outline, cx).as_deref(), Some("Parsing…"));
    cx.run_until_parked();
    assert_eq!(rows(&outline, cx), [(0, "f".to_string())]);
    assert_eq!(message(&outline, cx), None);
}

#[gpui::test]
fn a_capped_outline_builds_only_visible_rows(cx: &mut TestAppContext) {
    let text: String = (0..20_000).map(|ix| format!("fn f{ix}() {{}}\n")).collect();
    let (dir, paths) = project(&[("many.rs", &text)]);
    let (workbench, outline, cx) = outlining(cx, &dir);
    open(&workbench, &paths[0], cx);

    assert_eq!(rows(&outline, cx).len(), fe_syntax::MAX_OUTLINE_ITEMS);
    assert_eq!(
        outline
            .read_with(cx, |outline, _| outline.footer_text())
            .as_deref(),
        Some("Showing the first 10,000 symbols")
    );
    // The rows that fit, one more for a partly scrolled one, and the first
    // row measured twice (the list's request_layout and prepaint).
    let (fit, built) = outline.read_with(cx, |outline, _| {
        let row = outline.row_bounds(0).unwrap();
        let list = outline.list_bounds();
        let fit = (list.size.height / row.size.height).ceil() as usize;
        (fit, outline.built())
    });
    assert!(fit > 10 && fit < 1000, "{fit} rows fit");
    let total: usize = built.iter().map(Range::len).sum();
    assert!(
        total > 0 && total <= fit + 1 + 2,
        "{built:?} for {fit} rows"
    );
}

#[test]
fn the_default_keymap_binds_outline_show() {
    let store = fe_keymap::KeymapStore::load(
        fe_keymap::DEFAULT_MACOS,
        None,
        Some(crate::command_catalog()),
    );
    assert_eq!(store.diagnostics(), []);
    let id = fe_command::CommandId::parse("outline.show").unwrap();
    assert_eq!(store.keymap().bindings_for(&id).len(), 2);
}
