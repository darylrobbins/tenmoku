//! Filling the quickfix and location lists (`quickfix_fill.rs`): `:grep`
//! and `:vimgrep` search the project, `:make` runs its default build task,
//! and their lists go on the stack and open their first item.
//!
//! The project is a temporary folder of text files, which start no language
//! server; `:make`'s session is fake, as `task_run_tests.rs`'s are, and
//! leaves the output a test gives it.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use fe_terminal_view::{TerminalView, TerminalViewEvent};
use gpui::{Entity, EntityId, Focusable as _, SharedString, TestAppContext, VisualTestContext};

use crate::{
    Launch, StatusItem, StatusSide, Workbench,
    dock::problems_panel::ShownRow,
    quickfix::{QuickfixItem, QuickfixKind, QuickfixList},
    task_commands::TestFiles,
    task_defs::{ProjectFiles, TASKS_FILE},
    task_run::FakeTaskOutput,
};

/// The disk, as the app reads a project's tasks.
struct Disk;

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// A window on `root` holding `files`, with the first open and focused, vim
/// on, and a terminal panel whose sessions are fake.
fn window(
    cx: &mut TestAppContext,
    root: &Path,
    files: &[(&str, &[u8])],
) -> (Entity<Workbench>, VisualTestContext) {
    for (name, text) in files {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        cx.set_global(TestFiles(Arc::new(Disk)));
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = root.join(files[0].0);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Types `:{line}` and Enter in the focused editor.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

/// The vim message the status bar draws.
fn message(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<SharedString> {
    draw(cx);
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Left))
        .into_iter()
        .find(|(id, _)| id == StatusItem::VIM_MESSAGE)
        .map(|(_, text)| text)
}

/// The list in use: the quickfix list, or with `location` the active
/// pane's location list.
fn list(
    workbench: &Entity<Workbench>,
    location: bool,
    cx: &mut VisualTestContext,
) -> Option<QuickfixList> {
    workbench.update(cx, |workbench, cx| workbench.quickfix_list(location, cx))
}

/// Each item of the list in use as (file under `root`, row, column, text).
fn items(
    workbench: &Entity<Workbench>,
    location: bool,
    root: &Path,
    cx: &mut VisualTestContext,
) -> Vec<(String, u32, u32, String)> {
    let list = list(workbench, location, cx).expect("a list");
    (list.items.iter())
        .map(|item| {
            let rel = item.path.strip_prefix(root).expect("under the root");
            let rel = crate::tests::slashed(rel.to_string_lossy());
            (rel, item.row, item.col, item.text.clone())
        })
        .collect()
}

fn row(path: &str, row: u32, col: u32, text: &str) -> (String, u32, u32, String) {
    (path.to_owned(), row, col, text.to_owned())
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown")
}

/// The active editor's file and its caret's (row, byte column).
fn landed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, (u32, u32)) {
    let editor = active(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let head = model.primary_selection().head();
        let point = model.snapshot().text().offset_to_point(head);
        let path = model.path().expect("a file").to_owned();
        (path, (point.row, point.column))
    })
}

fn pane_id(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> EntityId {
    workbench.read_with(cx, |workbench, _| workbench.pane().entity_id())
}

fn has_location(workbench: &Entity<Workbench>, pane: EntityId, cx: &mut VisualTestContext) -> bool {
    workbench.update(cx, |workbench, cx| workbench.has_location_list(pane, cx))
}

fn terminal_views(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<Entity<TerminalView>> {
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.read_with(cx, |terminal, _| terminal.views().cloned().collect())
}

#[gpui::test]
fn grep_fills_one_item_per_matching_line(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files: &[(&str, &[u8])] = &[
        ("a.txt", b"foo one\nnone here\nbar foo foo\n"),
        ("sub/b.txt", "\u{e9} foo\n".as_bytes()),
        ("c.txt", b"FOO loud\n"),
        ("d.txt", b"nothing\n"),
    ];
    let (workbench, mut vcx) = window(cx, root, files);
    let cx = &mut vcx;

    ex("grep foo", cx);
    let list = list(&workbench, false, cx).expect("a quickfix list");
    assert_eq!(list.title, ":grep foo");
    assert!(
        (list.items.iter()).all(|item| item.kind == QuickfixKind::Untyped),
        "grep's items have no type"
    );
    // One per line at its first match, by path; smart case matches FOO.
    assert_eq!(
        items(&workbench, false, root, cx),
        [
            row("a.txt", 0, 0, "foo one"),
            row("a.txt", 2, 4, "bar foo foo"),
            row("c.txt", 0, 0, "FOO loud"),
            row("sub/b.txt", 0, 2, "\u{e9} foo"),
        ]
    );
    assert_eq!(landed(&workbench, cx), (root.join("a.txt"), (0, 0)));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("(1 of 4): foo one")
    );
    assert!(
        (list.items.iter()).all(|item| item.path.is_absolute()),
        "paths are absolute"
    );

    // The column is in chars: `é` is two bytes.
    ex("clast", cx);
    assert_eq!(landed(&workbench, cx), (root.join("sub/b.txt"), (0, 3)));

    // An uppercase letter matches case by case; the new list goes on top.
    ex("grep FOO", cx);
    assert_eq!(
        items(&workbench, false, root, cx),
        [row("c.txt", 0, 0, "FOO loud")]
    );
    assert_eq!(landed(&workbench, cx), (root.join("c.txt"), (0, 0)));
    ex("colder", cx);
    assert_eq!(
        self::list(&workbench, false, cx).map(|list| list.title),
        Some(":grep foo".to_owned())
    );

    // The rest of the line is the pattern, a regex.
    ex("grep o{2} [ol]", cx);
    assert_eq!(
        items(&workbench, false, root, cx),
        [
            row("a.txt", 0, 1, "foo one"),
            row("c.txt", 0, 1, "FOO loud")
        ]
    );
}

#[gpui::test]
fn grep_respects_gitignore_and_skips_binaries(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files: &[(&str, &[u8])] = &[
        ("kept.txt", b"foo kept\n"),
        (".gitignore", b"ignored/\n*.log\n"),
        ("ignored/x.txt", b"foo ignored\n"),
        ("run.log", b"foo logged\n"),
        ("bin.dat", b"foo\0binary\n"),
        ("deep/also.txt", b"a foo\n"),
    ];
    let (workbench, mut vcx) = window(cx, root, files);
    let cx = &mut vcx;

    ex("grep foo", cx);
    assert_eq!(
        items(&workbench, false, root, cx),
        [
            row("deep/also.txt", 0, 2, "a foo"),
            row("kept.txt", 0, 0, "foo kept")
        ]
    );

    // Nor does :vimgrep reach them by glob.
    ex("vimgrep /foo/ **/*.txt *.log *.dat", cx);
    assert_eq!(
        items(&workbench, false, root, cx),
        [
            row("deep/also.txt", 0, 2, "a foo"),
            row("kept.txt", 0, 0, "foo kept")
        ]
    );
}

#[gpui::test]
fn vimgrep_flag_g_lists_every_match_on_a_line(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files: &[(&str, &[u8])] = &[
        ("a.txt", b"foo and foo\nfoo\nnone\n"),
        ("b.txt", b"x foo foo\n"),
    ];
    let (workbench, mut vcx) = window(cx, root, files);
    let cx = &mut vcx;

    // One per line, in the file typed in.
    ex("vimgrep /foo/ %", cx);
    let list = list(&workbench, false, cx).expect("a list");
    assert_eq!(list.title, ":vimgrep /foo/ %");
    assert_eq!(
        items(&workbench, false, root, cx),
        [row("a.txt", 0, 0, "foo and foo"), row("a.txt", 1, 0, "foo")]
    );

    // Every match.
    ex("vimgrep /foo/g %", cx);
    assert_eq!(
        items(&workbench, false, root, cx),
        [
            row("a.txt", 0, 0, "foo and foo"),
            row("a.txt", 0, 8, "foo and foo"),
            row("a.txt", 1, 0, "foo"),
        ]
    );
    assert_eq!(landed(&workbench, cx), (root.join("a.txt"), (0, 0)));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("(1 of 3): foo and foo")
    );

    // `j` lists without jumping, and says what it made.
    ex("cnext", cx);
    assert_eq!(landed(&workbench, cx), (root.join("a.txt"), (0, 8)));
    ex("vimgrep #foo#gj", cx);
    assert_eq!(
        items(&workbench, false, root, cx),
        [
            row("a.txt", 0, 0, "foo and foo"),
            row("a.txt", 0, 8, "foo and foo"),
            row("a.txt", 1, 0, "foo"),
            row("b.txt", 0, 2, "x foo foo"),
            row("b.txt", 0, 6, "x foo foo"),
        ]
    );
    assert_eq!(landed(&workbench, cx), (root.join("a.txt"), (0, 8)));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("error list 3 of 3; 5 errors       :vimgrep #foo#gj")
    );

    // A word starts a pattern of one word, without flags.
    ex("vimgrep foo b.txt", cx);
    assert_eq!(
        items(&workbench, false, root, cx),
        [row("b.txt", 0, 2, "x foo foo")]
    );

    // A pattern with no closing delimiter.
    ex("vimgrep /foo", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E682: Invalid search pattern or delimiter")
    );
}

#[gpui::test]
fn vimgrep_globs_limit_the_files(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files: &[(&str, &[u8])] = &[
        ("top.md", b"foo top\n"),
        ("docs/deep.md", b"foo deep\n"),
        ("docs/x.txt", b"foo x\n"),
        ("notes.txt", b"foo notes\n"),
    ];
    let (workbench, mut vcx) = window(cx, root, files);
    let cx = &mut vcx;
    let paths = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| -> Vec<String> {
        (items(workbench, false, root, cx).into_iter())
            .map(|(path, ..)| path)
            .collect()
    };

    // As vim's, relative to the root: `*.md` is the root's own.
    ex("vimgrep /foo/ *.md", cx);
    assert_eq!(paths(&workbench, cx), ["top.md"]);
    ex("vimgrep /foo/ **/*.md", cx);
    assert_eq!(paths(&workbench, cx), ["docs/deep.md", "top.md"]);
    ex("vimgrep /foo/ docs", cx);
    assert_eq!(paths(&workbench, cx), ["docs/deep.md", "docs/x.txt"]);
    ex("vimgrep /foo/ *.md notes.txt", cx);
    assert_eq!(paths(&workbench, cx), ["notes.txt", "top.md"]);
    let absolute = root.join("docs").join("*.txt");
    ex(&format!("vimgrep /foo/ {}", absolute.display()), cx);
    assert_eq!(paths(&workbench, cx), ["docs/x.txt"]);

    // None is the whole project.
    ex("vimgrep /foo/", cx);
    assert_eq!(
        paths(&workbench, cx),
        ["docs/deep.md", "docs/x.txt", "notes.txt", "top.md"]
    );

    // A glob matching nothing matches nothing.
    ex("vimgrep /foo/ *.rs", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E480: No match: foo")
    );
    assert_eq!(paths(&workbench, cx).len(), 4, "the list stays");
}

#[gpui::test]
fn vimgrep_without_a_pattern_uses_the_last_search(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files: &[(&str, &[u8])] = &[("a.txt", b"one\ntwo\none two\n")];
    let (workbench, mut vcx) = window(cx, root, files);
    let cx = &mut vcx;

    ex("vimgrep // %", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E35: No previous regular expression")
    );
    assert!(list(&workbench, false, cx).is_none());

    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter");
    draw(cx);
    ex("vimgrep // %", cx);
    assert_eq!(
        items(&workbench, false, root, cx),
        [row("a.txt", 1, 0, "two"), row("a.txt", 2, 4, "one two")]
    );
}

#[gpui::test]
fn no_match_reports_e480_and_keeps_the_old_list(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files: &[(&str, &[u8])] = &[("a.txt", b"foo\nbar\n"), ("b.txt", b"foo\n")];
    let (workbench, mut vcx) = window(cx, root, files);
    let cx = &mut vcx;

    ex("grep foo", cx);
    ex("cnext", cx);
    let before = list(&workbench, false, cx).expect("a list");
    assert_eq!(before.current, 1);

    ex("grep zzz", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E480: No match: zzz")
    );
    assert_eq!(list(&workbench, false, cx), Some(before.clone()));
    ex("vimgrep /zz+/g", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E480: No match: zz+")
    );
    assert_eq!(list(&workbench, false, cx), Some(before));
    // Still one list: nothing went on the stack.
    ex("colder", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E380: At bottom of quickfix stack")
    );

    // A pattern that isn't a regex says why, and keeps the list too.
    ex("grep a(b", cx);
    let text = message(&workbench, cx).expect("a message");
    assert!(
        text.starts_with("E383: Invalid search string: a(b ("),
        "{text}"
    );
}

#[gpui::test]
fn lgrep_fills_only_the_panes_location_list(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files: &[(&str, &[u8])] = &[("a.txt", b"foo\nbar foo\n"), ("b.txt", b"foo b\n")];
    let (workbench, mut vcx) = window(cx, root, files);
    let cx = &mut vcx;
    let first = pane_id(&workbench, cx);

    ex("lgrep foo", cx);
    assert!(list(&workbench, false, cx).is_none(), "no quickfix list");
    let location = list(&workbench, true, cx).expect("a location list");
    assert_eq!(location.title, ":lgrep foo");
    assert_eq!(
        items(&workbench, true, root, cx),
        [
            row("a.txt", 0, 0, "foo"),
            row("a.txt", 1, 4, "bar foo"),
            row("b.txt", 0, 0, "foo b"),
        ]
    );
    assert_eq!(landed(&workbench, cx), (root.join("a.txt"), (0, 0)));
    ex("lnext", cx);
    assert_eq!(landed(&workbench, cx), (root.join("a.txt"), (1, 4)));

    // Another pane has none of its own.
    ex("vsplit", cx);
    let second = pane_id(&workbench, cx);
    assert_ne!(first, second);
    assert!(has_location(&workbench, first, cx));
    assert!(!has_location(&workbench, second, cx));
    ex("lnext", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E776: No location list")
    );

    // :lvimgrep fills the pane it's typed in.
    ex("lvimgrep /b/ b.txt", cx);
    assert_eq!(
        items(&workbench, true, root, cx),
        [row("b.txt", 0, 4, "foo b")]
    );
    assert!(
        list(&workbench, false, cx).is_none(),
        "still no quickfix list"
    );
    assert!(has_location(&workbench, first, cx));
}

#[gpui::test]
fn make_fills_the_list_from_the_tasks_matched_problems(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let tasks = "[[task]]\nlabel = \"build\"\ncommand = \"true\"\ngroup = \"build\"\n\
                 default = true\nproblem_matcher = \"gcc\"\n";
    let files: &[(&str, &[u8])] = &[
        ("a.txt", b"one\ntwo\n"),
        ("src/b.txt", b"zero\n\tx = 1;\n"),
        (TASKS_FILE, tasks.as_bytes()),
    ];
    let (workbench, mut vcx) = window(cx, root, files);
    let cx = &mut vcx;
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    // gcc's columns are display cells: past a tab, column 9 is the second char.
    let output = "src/b.txt:1:2: warning: unused [-Wunused]\n\
                  src/b.txt:2:9: error: bad x\n\
                  a.txt:2:1: error: second\n";
    cx.update(|_, cx| cx.set_global(FakeTaskOutput(output.to_owned())));

    ex("make", cx);
    let views = terminal_views(&workbench, cx);
    assert_eq!(views.len(), 1, "the build task runs");
    assert!(
        list(&workbench, false, cx).is_none(),
        "nothing until it exits"
    );

    views[0].update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(1))));
    draw(cx);
    let list = list(&workbench, false, cx).expect("a quickfix list");
    assert_eq!(list.title, ":make");
    let item = |path: &str, row: u32, col: u32, text: &str, kind| QuickfixItem {
        path: root.join(path),
        row,
        col,
        text: text.to_owned(),
        kind,
    };
    assert_eq!(
        list.items,
        [
            item(
                "src/b.txt",
                0,
                1,
                "unused [-Wunused]",
                QuickfixKind::Warning
            ),
            item("src/b.txt", 1, 1, "bad x", QuickfixKind::Error),
            item("a.txt", 1, 0, "second", QuickfixKind::Error),
        ]
    );
    // The first error, past the warning, is open.
    assert_eq!(list.current, 1);
    assert_eq!(landed(&workbench, cx), (root.join("src/b.txt"), (1, 1)));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("(2 of 3) error: bad x")
    );

    // The Problems panel lists them as the task's, as before.
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).problems().clone()
    });
    let rows = panel.read_with(cx, |panel, _| panel.shown_rows());
    let problems: Vec<String> = (rows.into_iter())
        .filter_map(|row| match row {
            ShownRow::Problem(_, message, source, _) => {
                Some(format!("{message} ({})", source.unwrap_or_default()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(
        problems.contains(&"bad x (build)".to_owned()),
        "{problems:?}"
    );

    // :lmake fills the pane's location list, and arguments are ignored.
    ex("lmake -j4", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("Ignoring -j4: :make runs the default build task")
    );
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    let view = terminal.read_with(cx, |terminal, _| terminal.view().cloned());
    let view = view.expect("the build task runs again");
    view.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(1))));
    draw(cx);
    let location = (self::list(&workbench, true, cx)).expect("a location list");
    assert_eq!(location.title, ":lmake");
    assert_eq!(location.items.len(), 3);
    let quickfix = self::list(&workbench, false, cx).expect("the quickfix list");
    assert_eq!(quickfix.title, ":make", "untouched");
}

#[gpui::test]
fn make_without_a_build_task_says_so(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files: &[(&str, &[u8])] = &[("a.txt", b"one\n")];
    let (workbench, mut vcx) = window(cx, root, files);
    let cx = &mut vcx;
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);

    ex("make", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("No build tasks: add .tenmoku/tasks.toml")
    );
    assert!(terminal_views(&workbench, cx).is_empty(), "nothing runs");
    assert!(list(&workbench, false, cx).is_none());
}
