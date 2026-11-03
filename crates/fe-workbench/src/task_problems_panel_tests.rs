//! A finished task's problems in the Problems panel. The sessions are real:
//! each task prints a file of output the test wrote, so what's matched is what
//! the terminal kept. The fake language server publishes for open files.

use std::{collections::BTreeMap, fs};

use fe_gpui::EditorView;
use fe_lsp::fake::{InitializeReply, Script, initialize_result};
use fe_text::Text;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, Position, PositionEncodingKind, PublishDiagnosticsParams,
    Range, notification::PublishDiagnostics,
};

use crate::{
    StatusSide, TerminalPanel,
    dock::{
        ProblemsPanel,
        problems_panel::{ColumnUnit, ITEM, ShownRow, WARNINGS_ITEM, column_offset},
    },
    task_defs::{Source, Task, TaskCommand},
    tests::lsp::Lsp,
};

const BUILD: &str = "cargo build";
const CLIPPY: &str = "cargo clippy";

/// A trusted project with `src/main.rs` open on the fake.
fn setup(cx: &mut TestAppContext) -> Lsp {
    let mut s = Lsp::new(cx);
    s.set_trust(true);
    s.open("src/main.rs");
    let uri = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    s
}

/// rustc's report of an error `message` at `location`.
fn rustc_error(message: &str, location: &str) -> String {
    format!("error: {message}\n --> {location}\n  |\n\n")
}

/// rustc's report of a warning `message` at `location`.
fn rustc_warning(message: &str, location: &str) -> String {
    format!("warning: {message}\n --> {location}\n  |\n\n")
}

/// `label`, a Cargo task running `line`.
fn cargo_task(s: &Lsp, label: &str, line: String) -> Task {
    let cwd = s
        .workbench
        .read_with(&s.cx, |w, _| w.root().unwrap().to_owned());
    Task {
        label: label.to_owned(),
        command: TaskCommand::Shell(line),
        cwd,
        env: BTreeMap::new(),
        group: None,
        default: false,
        source: Source::Cargo,
        problem_matcher: None,
    }
}

/// `label`, printing `output` and exiting with `code`.
fn printing_task(s: &Lsp, label: &str, output: &str, code: i32) -> Task {
    let file = format!("{}.out", label.replace(' ', "-"));
    fs::write(s.path(&file), output).unwrap();
    cargo_task(s, label, format!("cat '{file}'; exit {code}"))
}

fn run_task(s: &mut Lsp, task: Task) {
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.run_task(task, window, cx));
    });
    s.draw();
}

/// Runs `label`, printing `output` and passing.
fn run_printing(s: &mut Lsp, label: &str, output: &str) {
    let task = printing_task(s, label, output, 0);
    run_task(s, task);
}

fn terminal(s: &Lsp) -> Entity<TerminalPanel> {
    s.workbench.read_with(&s.cx, |w, _| w.terminal().clone())
}

/// Closes the active terminal tab.
fn close_tab(s: &mut Lsp) {
    let terminal = terminal(s);
    s.cx.update(|window, cx| {
        terminal.update(cx, |terminal, cx| terminal.close_active(window, cx));
    });
    s.draw();
}

fn panel(s: &Lsp) -> Entity<ProblemsPanel> {
    s.workbench
        .read_with(&s.cx, |w, cx| w.dock().read(cx).problems().clone())
}

fn rows(s: &mut Lsp) -> Vec<ShownRow> {
    let panel = panel(s);
    panel.read_with(&s.cx, |panel, _| panel.shown_rows())
}

/// A status item's text, as the last frame drew it.
fn item(s: &mut Lsp, id: &str) -> Option<String> {
    let bar = s.workbench.read_with(&s.cx, |w, _| w.status_bar().clone());
    let items = bar.read_with(&s.cx, |bar, _| bar.drawn(StatusSide::Left));
    (items.into_iter())
        .find(|(item, _)| item == id)
        .map(|(_, text)| text.to_string())
}

fn file(path: &str, problems: usize) -> ShownRow {
    ShownRow::File(path.to_owned(), problems)
}

fn error(message: &str, source: &str, position: &str) -> ShownRow {
    let source = Some(source.to_owned());
    ShownRow::Problem("E", message.to_owned(), source, position.to_owned())
}

fn warning(message: &str, source: &str, position: &str) -> ShownRow {
    let source = Some(source.to_owned());
    ShownRow::Problem("W", message.to_owned(), source, position.to_owned())
}

/// Has the fake publish an error `message` on `main.rs`'s first row, at
/// column `column`, `len` long.
fn publish_on_main(s: &Lsp, column: u32, len: u32, message: &str) {
    let uri = s.uri("src/main.rs");
    let version = s.fake.document(&uri).expect("it's open").version;
    let diagnostic = Diagnostic {
        range: Range::new(Position::new(0, column), Position::new(0, column + len)),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("rustc".into()),
        message: message.into(),
        ..Default::default()
    };
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri,
            diagnostics: vec![diagnostic],
            version: Some(version),
        });
}

fn active_editor(s: &Lsp) -> Entity<EditorView> {
    s.workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown")
}

fn active_path(s: &Lsp) -> Option<std::path::PathBuf> {
    let editor = active_editor(s);
    editor.read_with(&s.cx, |view, _| view.model().path().map(ToOwned::to_owned))
}

fn selection(s: &Lsp) -> std::ops::Range<usize> {
    let editor = active_editor(s);
    editor.read_with(&s.cx, |view, _| view.model().primary_selection().range)
}

/// Clicks panel row `ix`.
fn click_row(s: &mut Lsp, ix: usize) {
    let panel = panel(s);
    let bounds = panel.read_with(&s.cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    s.cx.simulate_click(bounds.center(), Modifiers::none());
    s.draw();
}

#[gpui::test]
fn finished_task_feeds_the_panel(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // A path too long for the grid's row, whose report and an earlier one
    // scroll off the screen before the task exits.
    let deep = ["d".repeat(100), "e".repeat(100), "f".repeat(100)].join("/");
    let deep = format!("src/{deep}");
    fs::create_dir_all(s.path(&deep)).unwrap();
    fs::write(s.path(&format!("{deep}/g.rs")), "fn g() {}\n").unwrap();
    let mut output = "error[E0308]: mismatched types\n --> src/other.rs:1:8\n".to_owned();
    output += &rustc_warning("unused import", "src/lib.rs:1:5");
    output += &rustc_error("cannot find `g`", &format!("{deep}/g.rs:1:4"));
    output += &"Compiling\n".repeat(300);
    let task = printing_task(&s, BUILD, &output, 101);
    run_task(&mut s, task);

    let expected = vec![
        file(&format!("{deep}/g.rs"), 1),
        error("cannot find `g`", BUILD, "1:4"),
        file("src/lib.rs", 1),
        warning("unused import", BUILD, "1:5"),
        file("src/other.rs", 1),
        error("mismatched types (E0308)", BUILD, "1:8"),
    ];
    s.until("the task's problems", |s| rows(s) == expected);
    assert_eq!(item(&mut s, ITEM).as_deref(), Some("2"));
    assert_eq!(item(&mut s, WARNINGS_ITEM).as_deref(), Some("1"));

    // The reports weren't on the screen, and the path didn't fit a row.
    let view = terminal(&s).read_with(&s.cx, |t, _| t.view().cloned().unwrap());
    let snapshot = view.read_with(&s.cx, |view, _| view.terminal().unwrap().snapshot());
    assert!(!snapshot.text().contains("mismatched types"));
    assert!(snapshot.cols < deep.len());
}

#[gpui::test]
fn rerun_replaces_the_previous_set(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let first =
        rustc_error("first a", "src/other.rs:1:1") + &rustc_error("first b", "src/lib.rs:1:1");
    run_printing(&mut s, BUILD, &first);
    let listed = vec![
        file("src/lib.rs", 1),
        error("first b", BUILD, "1:1"),
        file("src/other.rs", 1),
        error("first a", BUILD, "1:1"),
    ];
    s.until("the first run's problems", |s| rows(s) == listed);

    // Its next run replaces them, in the same tab.
    let second = rustc_error("second", "src/other.rs:1:2");
    run_printing(&mut s, BUILD, &second);
    let replaced = vec![file("src/other.rs", 1), error("second", BUILD, "1:2")];
    s.until("the second run's problems", |s| rows(s) == replaced);
    let tabs = terminal(&s).read_with(&s.cx, |t, _| t.views().count());
    assert_eq!(tabs, 1);

    // Another task's are its own, and merge with them in the same file.
    let other = rustc_error("clippy's", "src/other.rs:1:1");
    run_printing(&mut s, CLIPPY, &other);
    let both = vec![
        file("src/other.rs", 2),
        error("clippy's", CLIPPY, "1:1"),
        error("second", BUILD, "1:2"),
    ];
    s.until("both tasks' problems", |s| rows(s) == both);

    // A clean run clears its own alone.
    run_printing(&mut s, BUILD, "Finished\n");
    let clippy = vec![file("src/other.rs", 1), error("clippy's", CLIPPY, "1:1")];
    s.until("only the other task's", |s| rows(s) == clippy);
    run_printing(&mut s, CLIPPY, "Finished\n");
    s.until("no problems", |s| rows(s).is_empty());
    assert_eq!(item(&mut s, ITEM), None);
}

#[gpui::test]
fn closing_the_tab_clears_them(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    publish_on_main(&s, 3, 4, "from the server");
    let lsp_only = vec![
        file("src/main.rs", 1),
        error("from the server", "rustc", "1:4"),
    ];
    s.until("the server's problem", |s| rows(s) == lsp_only);

    let output =
        rustc_error("in main", "src/main.rs:1:1") + &rustc_error("in other", "src/other.rs:1:8");
    run_printing(&mut s, BUILD, &output);
    let merged = vec![
        file("src/main.rs", 2),
        error("in main", BUILD, "1:1"),
        error("from the server", "rustc", "1:4"),
        file("src/other.rs", 1),
        error("in other", BUILD, "1:8"),
    ];
    s.until("merged with the server's", |s| rows(s) == merged);
    assert_eq!(item(&mut s, ITEM).as_deref(), Some("3"));

    close_tab(&mut s);
    s.until("the tab's problems gone", |s| rows(s) == lsp_only);
    assert_eq!(item(&mut s, ITEM).as_deref(), Some("1"));
}

#[gpui::test]
fn clicking_a_task_problem_opens_the_location(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // Columns count chars: `bad` is the 14th, after a two-byte one.
    let wide_text = "fn f() {}\nlet s = \"é\"; bad();\n";
    let bad = wide_text.find("bad").unwrap();
    fs::write(s.path("src/wide.rs"), wide_text).unwrap();
    let output =
        rustc_error("in other", "src/other.rs:1:8") + &rustc_error("in wide", "src/wide.rs:2:14");
    run_printing(&mut s, BUILD, &output);
    let listed = vec![
        file("src/other.rs", 1),
        error("in other", BUILD, "1:8"),
        file("src/wide.rs", 1),
        error("in wide", BUILD, "2:14"),
    ];
    s.until("the task's problems", |s| rows(s) == listed);
    s.run("problems.show");

    // A file that isn't open opens in the active pane, focused.
    click_row(&mut s, 1);
    let other = s.path("src/other.rs");
    s.until("other.rs open", |s| active_path(s).as_ref() == Some(&other));
    assert_eq!(selection(&s), 7..7);
    let editor = active_editor(&s);
    let focused =
        s.cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    assert!(focused);
    assert_eq!(rows(&mut s), listed, "open, its problem is still listed");

    click_row(&mut s, 3);
    let wide = s.path("src/wide.rs");
    s.until("wide.rs open", |s| active_path(s).as_ref() == Some(&wide));
    assert_eq!(selection(&s), bad..bad);

    // An open file, shown again.
    click_row(&mut s, 1);
    s.until("other.rs shown", |s| {
        active_path(s).as_ref() == Some(&other)
    });
    assert_eq!(selection(&s), 7..7);
}

#[gpui::test]
fn merged_rows_open_their_own_way(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    publish_on_main(&s, 3, 4, "from the server");
    let output = rustc_error("from the task", "src/main.rs:1:10");
    run_printing(&mut s, BUILD, &output);
    let merged = vec![
        file("src/main.rs", 2),
        error("from the server", "rustc", "1:4"),
        error("from the task", BUILD, "1:10"),
    ];
    s.until("merged", |s| rows(s) == merged);
    s.run("problems.show");

    // The buffer's problem selects its range, the task's its position.
    click_row(&mut s, 1);
    assert_eq!(selection(&s), 3..7);
    click_row(&mut s, 2);
    assert_eq!(selection(&s), 9..9);
}

#[gpui::test]
fn task_problems_do_not_mark_editors(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.open("src/other.rs");
    let output = rustc_error("in other", "src/other.rs:1:8")
        + &rustc_warning("in other too", "src/other.rs:1:1");
    run_printing(&mut s, BUILD, &output);
    let listed = vec![
        file("src/other.rs", 2),
        warning("in other too", BUILD, "1:1"),
        error("in other", BUILD, "1:8"),
    ];
    s.until("the task's problems", |s| rows(s) == listed);

    // The counts are of the panel's rows, these among them.
    assert_eq!(item(&mut s, ITEM).as_deref(), Some("1"));
    assert_eq!(item(&mut s, WARNINGS_ITEM).as_deref(), Some("1"));

    // The editor has nothing to mark, or for the diagnostic keys to go to.
    let editor = active_editor(&s);
    editor.read_with(&s.cx, |view, _| {
        let model = view.model();
        assert_eq!(model.path(), Some(s.path("src/other.rs").as_path()));
        assert_eq!(model.problems(), []);
        assert_eq!(model.next_diagnostic(0, true), None);
    });
}

#[gpui::test]
fn two_tabs_of_a_label_are_two_sources(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // Still running, so the next run of its label gets a tab of its own.
    let running = cargo_task(&s, BUILD, "read -r x".to_owned());
    run_task(&mut s, running);
    let output = rustc_error("from the second", "src/other.rs:1:1");
    run_printing(&mut s, BUILD, &output);
    let listed = vec![
        file("src/other.rs", 1),
        error("from the second", BUILD, "1:1"),
    ];
    s.until("the second tab's problems", |s| rows(s) == listed);
    let tabs = terminal(&s).read_with(&s.cx, |t, _| t.views().count());
    assert_eq!(tabs, 2);

    // The first tab closing leaves the second's.
    let terminal = terminal(&s);
    s.cx.update(|window, cx| terminal.update(cx, |t, cx| t.activate(0, window, cx)));
    close_tab(&mut s);
    assert_eq!(terminal.read_with(&s.cx, |t, _| t.views().count()), 1);
    assert_eq!(rows(&mut s), listed);

    close_tab(&mut s);
    s.until("the second tab's gone", |s| rows(s).is_empty());
}

#[gpui::test]
fn a_task_reads_its_output_with_its_matcher(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let output = "src/other.rs:1:8: error: from gcc\n";
    let file_task = |s: &Lsp, label: &str, matcher: Option<&str>| Task {
        source: Source::File,
        problem_matcher: matcher.map(ToOwned::to_owned),
        ..printing_task(s, label, output, 0)
    };

    // Named by the task.
    let named = file_task(&s, "named", Some("gcc"));
    run_task(&mut s, named);
    let listed = vec![file("src/other.rs", 1), error("from gcc", "named", "1:8")];
    s.until("the named matcher's problem", |s| rows(s) == listed);

    // None, or none that exists, reads nothing: each passes with no rows.
    for (label, matcher) in [("plain", None), ("misnamed", Some("nope"))] {
        let task = file_task(&s, label, matcher);
        run_task(&mut s, task);
        let passed = format!("{label} \u{2713}");
        let terminal = terminal(&s);
        s.until("the task to pass", |s| {
            terminal.read_with(&s.cx, |terminal, _| {
                (terminal.drawn_tabs().iter()).any(|(tab, _)| tab.as_ref() == passed)
            })
        });
        assert_eq!(rows(&mut s), listed);
    }
}

/// A project trusted on a UTF-16 fake, with `text` as `src/main.rs`, open.
fn setup_with_main(cx: &mut TestAppContext, text: &str) -> Lsp {
    let rust = Script {
        initialize: InitializeReply::Result(Box::new(initialize_result(Some(
            PositionEncodingKind::UTF16,
        )))),
        ..Script::default()
    };
    let mut s = Lsp::with_scripts(cx, rust, Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), text).unwrap();
    s.open("src/main.rs");
    let uri = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    s
}

/// A gcc-format task of `label`, printing `output`.
fn gcc_task(s: &Lsp, label: &str, output: &str) -> Task {
    Task {
        source: Source::File,
        problem_matcher: Some("gcc".to_owned()),
        ..printing_task(s, label, output, 0)
    }
}

/// Each case is what the tool printed for `bad` on a row of real source, but
/// tsc's, counted by hand in the UTF-16 units its positions are in.
#[test]
fn columns_decode_in_each_tools_unit() {
    use ColumnUnit::{Bytes, Chars, Display, Utf16};
    for (what, line, column, unit) in [
        ("rustc", "\tlet _s = \"é🦀\"; bad();", 17, Chars),
        ("go", "\t_ = \"é🦀\"; bad()", 16, Bytes),
        ("clang", "\tconst char *s = \"é🦀\"; return bad;", 35, Bytes),
        ("clang, tab", "\tint x = 0; return bad;", 20, Bytes),
        ("gcc", "\tconst char *s = \"é🦀\"; return bad;", 39, Display),
        ("gcc, tab", "\tint x = 0; return bad;", 27, Display),
        (
            "gcc, tab stop",
            "  int a;\tint x = 0; return bad;",
            35,
            Display,
        ),
        (
            "gcc, wide",
            "    const char *s = \"日本\"; return bad;",
            36,
            Display,
        ),
        ("tsc, astral", "const s = \"🦀\"; bad();", 17, Utf16),
    ] {
        let text = Text::new(&format!("first\n{line}\nlast\n"));
        let bad = 6 + line.find("bad").unwrap();
        assert_eq!(column_offset(&text, 1, column - 1, unit), bad, "{what}");
    }

    // After a mark of no width; past the row, and past the text.
    let text = Text::new("e\u{301}x\nend");
    assert_eq!(column_offset(&text, 0, 1, Display), "e\u{301}".len());
    assert_eq!(column_offset(&text, 0, 99, Display), "e\u{301}x".len());
    assert_eq!(column_offset(&text, 9, 0, Bytes), text.len());
}

#[gpui::test]
fn navigation_lands_after_emoji_tabs_and_wide_text(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // rustc counts chars, past an emoji.
    let rust_line = "\tlet _s = \"é🦀\"; bad();";
    // gcc counts cells: a tab to the next stop, and wide text as two.
    let c_line = "\tconst char *s = \"é🦀\"; return bad;";
    fs::write(s.path("src/rust.rs"), format!("{rust_line}\n")).unwrap();
    fs::write(s.path("src/gcc.c"), format!("{c_line}\n")).unwrap();
    fs::write(s.path("src/clang.c"), format!("{c_line}\n")).unwrap();
    run_printing(&mut s, BUILD, &rustc_error("rust", "src/rust.rs:1:17"));
    let gcc = gcc_task(&s, "gcc", "src/gcc.c:1:39: error: from gcc\n");
    run_task(&mut s, gcc);
    // clang counts bytes, and says it's clang.
    let clang = gcc_task(
        &s,
        "clang",
        "src/clang.c:1:35: error: from clang\n1 error generated.\n",
    );
    run_task(&mut s, clang);
    let listed = vec![
        file("src/clang.c", 1),
        error("from clang", "clang", "1:35"),
        file("src/gcc.c", 1),
        error("from gcc", "gcc", "1:39"),
        file("src/rust.rs", 1),
        error("rust", BUILD, "1:17"),
    ];
    s.until("the tasks' problems", |s| rows(s) == listed);
    s.run("problems.show");

    for (row, name, line) in [
        (1, "src/clang.c", c_line),
        (3, "src/gcc.c", c_line),
        (5, "src/rust.rs", rust_line),
    ] {
        click_row(&mut s, row);
        let path = s.path(name);
        s.until("the file open", |s| active_path(s).as_ref() == Some(&path));
        let bad = line.find("bad").unwrap();
        assert_eq!(selection(&s), bad..bad, "{name}");
    }
}

#[gpui::test]
fn merged_rows_sort_by_where_they_are_in_the_text(cx: &mut TestAppContext) {
    // `a` is byte 16, char 7 and UTF-16 unit 10; `b` is byte 19, char 10.
    let text = "\"🦀🦀🦀\"; a; b;\n";
    assert_eq!((text.find('a'), text.find('b')), (Some(16), Some(19)));
    let mut s = setup_with_main(cx, text);
    fs::write(s.path("src/other.rs"), text).unwrap();
    let diagnostic = Diagnostic {
        range: Range::new(Position::new(0, 10), Position::new(0, 11)),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("rustc".into()),
        message: "at a".into(),
        ..Default::default()
    };

    // In an open file, and in one only read from disk, the printed columns
    // would put `b`'s 11 before the server's 17. At one place, worst first.
    let uri = s.uri("src/main.rs");
    let version = s.fake.document(&uri).expect("it's open").version;
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri,
            diagnostics: vec![diagnostic.clone()],
            version: Some(version),
        });
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri: s.uri("src/other.rs"),
            diagnostics: vec![diagnostic],
            version: None,
        });
    let output = ["src/main.rs", "src/other.rs"].map(|path| {
        rustc_error("task at b", &format!("{path}:1:11"))
            + &rustc_warning("task at a", &format!("{path}:1:8"))
    });
    run_printing(&mut s, BUILD, &output.concat());
    let rows_of = |name: &str| {
        vec![
            file(name, 3),
            error("at a", "rustc", "1:17"),
            warning("task at a", BUILD, "1:8"),
            error("task at b", BUILD, "1:11"),
        ]
    };
    let listed = [rows_of("src/main.rs"), rows_of("src/other.rs")].concat();
    s.until("in text order", |s| rows(s) == listed);
}

#[gpui::test]
fn a_task_problem_stays_in_the_active_pane(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.run("pane.split_right");
    let active = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    let panes = s
        .workbench
        .read_with(&s.cx, |w, cx| w.pane_group(cx).panes().len());
    assert_eq!(panes, 2);
    run_printing(&mut s, BUILD, &rustc_error("in main", "src/main.rs:1:4"));
    let listed = vec![file("src/main.rs", 1), error("in main", BUILD, "1:4")];
    s.until("the task's problem", |s| rows(s) == listed);
    s.run("problems.show");

    click_row(&mut s, 1);
    assert_eq!(
        s.workbench.read_with(&s.cx, |w, _| w.pane().clone()),
        active
    );
    assert_eq!(selection(&s), 3..3);
}

/// gcc's and clang's real columns for one row: `a` is gcc's 26 and clang's 19,
/// `b` clang's 23, so by printed column `b` would come before `a`.
#[gpui::test]
fn a_file_only_tasks_list_sorts_by_where_they_are_in_its_text(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    fs::write(s.path("src/two.c"), "\tint f() { return a + b; }\n").unwrap();
    let gcc = gcc_task(&s, "gcc", "src/two.c:1:26: warning: gcc at a\n");
    run_task(&mut s, gcc);
    let clang = gcc_task(
        &s,
        "clang",
        "src/two.c:1:19: error: clang at a\n\
         src/two.c:1:23: error: clang at b\n\
         2 errors generated.\n",
    );
    run_task(&mut s, clang);
    // At one place, the worst first; each with the column its tool printed.
    let listed = vec![
        file("src/two.c", 3),
        error("clang at a", "clang", "1:19"),
        warning("gcc at a", "gcc", "1:26"),
        error("clang at b", "clang", "1:23"),
    ];
    s.until("in text order", |s| rows(s) == listed);
}
