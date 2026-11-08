//! Search TODO Comments: the project's keywords from a search off the main
//! thread, each with its kind, those an open editor's parse puts outside a
//! comment left out; a leading keyword narrowing the kind, the walk's
//! ignore rules applied, Enter opening at the keyword, and the cap said.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{MAX_ROWS, TodoPicker};
use crate::{
    Picker, PickerDelegate as _, Workbench, tests::open_root, todo_comments::Kind,
    workbench::Palette,
};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench rooted at a temp dir holding `files`, its window active.
fn rooted<'a>(
    files: &[(&str, &str)],
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, _| window.activate_window());
    draw(cx);
    (dir, workbench, cx)
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

/// Runs `todo.list` where focus is, in the workbench's context, and waits
/// for the search to land.
fn list(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Picker<TodoPicker>> {
    // An action reaches the root's listeners only from focus within.
    cx.update(|window, cx| {
        if window.focused(cx).is_none() {
            window.focus(&workbench.focus_handle(cx), cx);
        }
    });
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of("todo.list").unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let picker = picker(workbench, cx);
    for _ in 0..100 {
        if !picker.read_with(cx, |picker, _| picker.delegate.searching()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        draw(cx);
    }
    assert!(!picker.read_with(cx, |picker, _| picker.delegate.searching()));
    draw(cx);
    picker
}

fn picker(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Picker<TodoPicker>> {
    (workbench.read_with(cx, |workbench, cx| workbench.todo_picker(cx))).expect("the picker shows")
}

type Label = (Kind, String, String, String);

fn rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Label> {
    let picker = picker(workbench, cx);
    let labels = picker.read_with(cx, |picker, _| picker.delegate.labels());
    // Written with `/` here; the picker shows the system's separator.
    (labels.into_iter())
        .map(|(kind, keyword, text, at)| (kind, keyword, text, crate::tests::slashed(at)))
        .collect()
}

fn label(kind: Kind, keyword: &str, text: &str, at: &str) -> Label {
    (kind, keyword.to_owned(), text.to_owned(), at.to_owned())
}

fn status(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let picker = picker(workbench, cx);
    picker.read_with(cx, |picker, _| {
        picker.delegate.status().map(|it| it.to_string())
    })
}

/// Types `query` into the picker's input.
fn query(workbench: &Entity<Workbench>, query: &str, cx: &mut VisualTestContext) {
    let picker = picker(workbench, cx);
    let input = picker.read_with(cx, |picker, _| picker.input().clone());
    input.update(cx, |input, cx| input.set_text(query, cx));
    picker.update(cx, |picker, cx| {
        picker.delegate.update_matches(query.to_owned(), cx)
    });
    draw(cx);
}

const MAIN_RS: &str = "\
fn main() {
    // TODO: make it fast
    let s = \"FIXME: not a comment\";
    // HACK(bob): hold it together
}
";

const LIB_RS: &str = "\
// NOTE: read this first
// TODO():empty user is no tag to the scan, but the search lists it
let s = \"BUG: a string, unparsed\";
";

const NOTES_TXT: &str = "\
WARN: a warning
PERF: slow
";

#[gpui::test]
fn lists_todo_comments_across_the_project_with_their_kind(cx: &mut TestAppContext) {
    let files = [
        ("src/main.rs", MAIN_RS),
        ("src/lib.rs", LIB_RS),
        ("notes.txt", NOTES_TXT),
        (
            "plain.txt",
            "nothing to see\nTODOS: no colon after the keyword\n",
        ),
    ];
    let (dir, workbench, cx) = rooted(&files, cx);
    // `main.rs` open, its parse up to date: the string's keyword goes.
    let editor = open(&workbench, &dir.path().join("src/main.rs"), cx);
    for _ in 0..20 {
        let parsed = editor.read_with(cx, |view, _| {
            view.syntax()
                .is_some_and(|syntax| syntax.tree().is_some() && !syntax.needs_parse())
        });
        if parsed {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        draw(cx);
    }

    list(&workbench, cx);
    assert_eq!(
        workbench.read_with(cx, |workbench, _| workbench.palette),
        Some(Palette::Todos)
    );
    assert_eq!(
        rows(&workbench, cx),
        [
            label(Kind::Warn, "WARN", "a warning", "notes.txt:1"),
            label(Kind::Perf, "PERF", "slow", "notes.txt:2"),
            // No editor: the search's hits, a string's included.
            label(Kind::Note, "NOTE", "read this first", "src/lib.rs:1"),
            label(
                Kind::Todo,
                "TODO",
                "empty user is no tag to the scan, but the search lists it",
                "src/lib.rs:2"
            ),
            label(Kind::Fix, "BUG", "a string, unparsed\";", "src/lib.rs:3"),
            label(Kind::Todo, "TODO", "make it fast", "src/main.rs:2"),
            label(Kind::Hack, "HACK", "hold it together", "src/main.rs:4"),
        ]
    );
    assert_eq!(status(&workbench, cx), None);

    // Typing matches the text and the path.
    query(&workbench, "main", cx);
    assert_eq!(
        rows(&workbench, cx),
        [
            label(Kind::Todo, "TODO", "make it fast", "src/main.rs:2"),
            label(Kind::Hack, "HACK", "hold it together", "src/main.rs:4"),
        ]
    );
    query(&workbench, "zzz", cx);
    assert_eq!(rows(&workbench, cx), []);
    assert_eq!(
        status(&workbench, cx).as_deref(),
        Some("No matching TODO comments")
    );
}

#[gpui::test]
fn a_leading_keyword_filters_by_kind(cx: &mut TestAppContext) {
    let text = "\
// TODO: later
// FIXME: broken
// BUG: also broken
// TODO: tidy later
// NOTE: later on
";
    let (_dir, workbench, cx) = rooted(&[("a.txt", text)], cx);
    list(&workbench, cx);
    assert_eq!(rows(&workbench, cx).len(), 5);

    // `FIX ` is the kind, so `FIXME:` and `BUG:` too.
    query(&workbench, "FIX ", cx);
    assert_eq!(
        rows(&workbench, cx),
        [
            label(Kind::Fix, "FIXME", "broken", "a.txt:2"),
            label(Kind::Fix, "BUG", "also broken", "a.txt:3"),
        ]
    );

    // The rest of the query matches as ever, within the kind.
    query(&workbench, "TODO tidy", cx);
    assert_eq!(
        rows(&workbench, cx),
        [label(Kind::Todo, "TODO", "tidy later", "a.txt:4")]
    );

    // A keyword in lower case, or with no space after it, is just a query.
    query(&workbench, "later", cx);
    assert_eq!(rows(&workbench, cx).len(), 3);
    query(&workbench, "todo later", cx);
    assert_eq!(rows(&workbench, cx), []);
}

#[gpui::test]
fn respects_gitignore(cx: &mut TestAppContext) {
    let files = [
        (".gitignore", "ignored/\n*.log\n"),
        ("kept.txt", "TODO: kept\n"),
        ("ignored/a.txt", "TODO: in an ignored folder\n"),
        ("build.log", "TODO: in an ignored file\n"),
    ];
    let (_dir, workbench, cx) = rooted(&files, cx);
    list(&workbench, cx);
    assert_eq!(
        rows(&workbench, cx),
        [label(Kind::Todo, "TODO", "kept", "kept.txt:1")]
    );
}

/// The active editor's file and caret, as a byte offset.
fn landed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, usize) {
    let editor = active_editor(workbench, cx);
    editor.read_with(cx, |view, _| {
        let model = view.model();
        let path = model.path().expect("a file").to_owned();
        (path, model.primary_selection().head())
    })
}

/// Runs command `id` where focus is, in the workbench's context.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

#[gpui::test]
fn enter_opens_at_the_line(cx: &mut TestAppContext) {
    let far = format!("{}    // TODO(me): far down\n", "filler\n".repeat(120));
    let a_text = "one\ntwo\nthree\nfour\nfive\nsix\n";
    let files = [("a.txt", a_text), ("b.txt", far.as_str())];
    let (dir, workbench, cx) = rooted(&files, cx);
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(300.)));
    let a = open(&workbench, &dir.path().join("a.txt"), cx);
    // An earlier jump, from `a.txt`'s start to its fifth row.
    workbench.update_in(cx, |workbench, window, cx| {
        let path = dir.path().join("a.txt");
        workbench
            .open_document_at(path, Some(5), None, window, cx)
            .detach();
    });
    draw(cx);
    let fifth = a_text.find("five").unwrap();
    assert_eq!(landed(&workbench, cx), (dir.path().join("a.txt"), fifth));

    let picker = list(&workbench, cx);
    assert_eq!(
        rows(&workbench, cx),
        [label(Kind::Todo, "TODO", "far down", "b.txt:121")]
    );

    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(0, window, cx)));
    draw(cx);
    assert_eq!(
        workbench.read_with(cx, |workbench, _| workbench.palette),
        None
    );
    let editor = active_editor(&workbench, cx);
    assert_ne!(editor, a);
    let (path, caret) = editor.read_with(cx, |view, _| {
        let model = view.model();
        let head = model.primary_selection().head();
        let point = model.text().offset_to_point(head);
        (model.path().map(Path::to_owned), (point.row, point.column))
    });
    assert_eq!(path, Some(dir.path().join("b.txt")));
    assert_eq!(caret, (120, 7), "at the keyword");
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    let shown = editor.read_with(cx, |view, _| view.shows_buffer_row(120).expect("drawn"));
    assert!(shown, "scrolled to it");

    // Back returns from the pick, then from the earlier jump: the pick is
    // one jump, not two.
    run("nav.back", cx);
    assert_eq!(landed(&workbench, cx), (dir.path().join("a.txt"), fifth));
    run("nav.back", cx);
    assert_eq!(landed(&workbench, cx), (dir.path().join("a.txt"), 0));
}

#[gpui::test]
fn a_long_line_lists_every_tag(cx: &mut TestAppContext) {
    // Past the search's preview: a second tag far along the line, and a tag
    // whose user is longer than the preview.
    let far = format!("// TODO: first {} FIXME: second\n", "x".repeat(600));
    let user = format!("// HACK({}): long user\n", "u".repeat(600));
    let files = [("far.txt", far.as_str()), ("user.txt", user.as_str())];
    let (_dir, workbench, cx) = rooted(&files, cx);
    list(&workbench, cx);
    let found = rows(&workbench, cx);
    let kinds: Vec<_> = (found.iter())
        .map(|(kind, keyword, _, at)| (*kind, keyword.as_str(), at.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            (Kind::Todo, "TODO", "far.txt:1"),
            (Kind::Fix, "FIXME", "far.txt:1"),
            (Kind::Hack, "HACK", "user.txt:1"),
        ]
    );
    assert_eq!(found[1].2, "second");
    assert_eq!(found[2].2, "long user");
    // The first tag's text runs on through the second, up to the cap.
    assert!(found[0].2.starts_with("first xxx"));
    assert_eq!(found[0].2.chars().count(), super::MAX_TEXT_CHARS);
}

#[gpui::test]
fn enter_lands_on_the_keyword_past_a_long_unicode_prefix(cx: &mut TestAppContext) {
    // 800 bytes of 400 chars before it: the preview starts past them.
    let line = format!("{} // TODO: here\n", "é".repeat(400));
    let (dir, workbench, cx) = rooted(&[("wide.txt", line.as_str())], cx);
    let picker = list(&workbench, cx);
    assert_eq!(
        rows(&workbench, cx),
        [label(Kind::Todo, "TODO", "here", "wide.txt:1")]
    );
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(0, window, cx)));
    draw(cx);
    let todo = line.find("TODO").unwrap();
    assert_eq!(landed(&workbench, cx), (dir.path().join("wide.txt"), todo));
}

#[gpui::test]
fn caps_at_5000_and_says_so(cx: &mut TestAppContext) {
    let under = "// TODO: one\n".repeat(10);
    let over: String = (0..MAX_ROWS + 1)
        .map(|n| format!("// TODO: number {n}\n"))
        .collect();
    let (dir, workbench, cx) = rooted(&[("few.txt", under.as_str())], cx);
    list(&workbench, cx);
    assert_eq!(rows(&workbench, cx).len(), 10);
    assert_eq!(status(&workbench, cx), None, "not capped");
    cx.simulate_keystrokes("escape");
    draw(cx);

    fs::write(dir.path().join("many.txt"), over).unwrap();
    list(&workbench, cx);
    assert_eq!(rows(&workbench, cx).len(), MAX_ROWS);
    assert_eq!(
        status(&workbench, cx).as_deref(),
        Some("Stopped at the first 5,000 matches")
    );

    // With nothing matching, the list still says it was cut short.
    query(&workbench, "zzz", cx);
    assert_eq!(
        status(&workbench, cx).as_deref(),
        Some("No matching TODO comments: stopped at the first 5,000 matches")
    );
}

#[gpui::test]
fn a_capped_search_the_parse_empties_says_so(cx: &mut TestAppContext) {
    // Every hit in a string of a file open with its parse, past the cap.
    let strings: String = (0..MAX_ROWS + 1)
        .map(|n| format!("    \"TODO: number {n}\",\n"))
        .collect();
    let text = format!("const S: &[&str] = &[\n{strings}];\n");
    let (dir, workbench, cx) = rooted(&[("main.rs", text.as_str())], cx);
    let editor = open(&workbench, &dir.path().join("main.rs"), cx);
    wait_for_parse(&editor, cx);
    list(&workbench, cx);
    assert_eq!(rows(&workbench, cx), []);
    assert_eq!(
        status(&workbench, cx).as_deref(),
        Some("No TODO comments: stopped at the first 5,000 matches")
    );
}

/// Waits for `editor`'s parse to be of its text.
fn wait_for_parse(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    for _ in 0..100 {
        let parsed = editor.read_with(cx, |view, _| {
            view.syntax()
                .is_some_and(|syntax| syntax.tree().is_some() && !syntax.needs_parse())
        });
        if parsed {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        draw(cx);
    }
    panic!("never parsed");
}
