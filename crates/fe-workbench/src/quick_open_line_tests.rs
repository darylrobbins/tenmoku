//! Quick open's `path:line[:column]` queries.

use std::{fs, path::PathBuf};

use fe_project::FinderMatch;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::split_location;
use crate::{
    Picker, QuickOpen, Workbench,
    tests::quick_open::{expected, matches, picker, workbench_at},
};

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), "one\ntwo\nthree\nfour\n").unwrap();
    let long: String = (0..300).map(|row| format!("line {row}\n")).collect();
    fs::write(dir.path().join("long.rs"), long).unwrap();
    let ten: String = (0..10).map(|row| format!("row {row}\n")).collect();
    fs::write(dir.path().join("ten.rs"), ten).unwrap();
    fs::write(dir.path().join("other.rs"), "").unwrap();
    dir
}

fn status(picker: &Entity<Picker<QuickOpen>>, cx: &mut VisualTestContext) -> String {
    picker.read_with(cx, |picker, _| picker.delegate.status_text())
}

/// The rows and status line for `query`.
fn rows(
    picker: &Entity<Picker<QuickOpen>>,
    query: &str,
    cx: &mut VisualTestContext,
) -> (Vec<FinderMatch>, String) {
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input(query);
    let result = (matches(picker, cx), status(picker, cx));
    cx.simulate_keystrokes("escape");
    result
}

/// Whether `row` is among the rows the active editor shows.
fn row_visible(workbench: &Entity<Workbench>, row: u32, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx).expect("a file is shown");
        let (top, rows) = editor.read(cx).viewport_rows().expect("a frame was drawn");
        (top..top + rows).contains(&row)
    })
}

/// The active tab's file, caret as (row, column), and the tabs open.
fn at(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (PathBuf, (usize, usize), usize) {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench
            .active_editor(cx)
            .expect("a file is shown")
            .read(cx);
        let model = editor.model();
        let text = model.text();
        let caret = model.selections()[0].range.start;
        let row = text.to_string()[..caret].matches('\n').count();
        let start = text.row_start(row as u32);
        (
            model.path().unwrap().to_owned(),
            (row, caret - start),
            workbench.pane().read(cx).tabs().len(),
        )
    })
}

#[gpui::test]
fn suffix_is_split(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = workbench_at(cx, Some(dir.path()));
    let picker = picker(&workbench, cx);

    let plain = rows(&picker, "main.rs", cx);
    assert!(!plain.0.is_empty());
    for query in ["main.rs:3", "main.rs:3:5", "main.rs:99:1"] {
        assert_eq!(rows(&picker, query, cx), plain, "{query}");
    }
    for query in ["main.rs:", "main.rs:x", "main.rs:0"] {
        let (found, status) = rows(&picker, query, cx);
        let want = expected(dir.path(), query);
        assert_eq!(found, want, "{query}");
        assert_eq!(status, format!("{} results", want.len()), "{query}");
    }
    // A suffix past u32 is still a suffix.
    assert_eq!(rows(&picker, "main.rs:4294967296:99999999999", cx), plain);
    assert_eq!(split_location("main.rs:0"), ("main.rs:0", None));
    assert_eq!(split_location("main.rs:3:0"), ("main.rs:3:0", None));
    assert_eq!(split_location("main.rs:3:"), ("main.rs:3:", None));
    assert_eq!(split_location(":3"), (":3", None));
    assert_eq!(
        split_location("main.rs:4294967296"),
        ("main.rs", Some((u32::MAX - 1, 0)))
    );
    assert_eq!(split_location("main.rs:3"), ("main.rs", Some((2, 0))));
    assert_eq!(split_location("main.rs:3:5"), ("main.rs", Some((2, 4))));
}

#[gpui::test]
fn opens_at_the_line(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = workbench_at(cx, Some(root));

    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("main.rs:3");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(at(&workbench, cx), (root.join("main.rs"), (2, 0), 1));
    assert!(row_visible(&workbench, 2, cx));

    // Already open: its tab is activated and the caret moved.
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("ten.rs");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(at(&workbench, cx).0, root.join("ten.rs"));
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("main.rs:2:3");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(at(&workbench, cx), (root.join("main.rs"), (1, 2), 2));

    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("main.rs:3:5");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(at(&workbench, cx), (root.join("main.rs"), (2, 4), 2));

    // Past the end: the last row.
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("ten.rs:99");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let (path, (row, _), _) = at(&workbench, cx);
    assert_eq!(path, root.join("ten.rs"));
    assert_eq!(row, 10, "the empty row after the last newline");

    // A column past the row's end stops at its end, in a file already open
    // and in one just loaded.
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("main.rs:1:9");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(at(&workbench, cx).1, (0, 3));
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("other.rs:1:9");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(at(&workbench, cx).0, root.join("other.rs"));

    // Far down a long file: scrolled to.
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("long.rs:250:3");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(at(&workbench, cx).1, (249, 2));
    assert!(row_visible(&workbench, 249, cx));

    // A line past u32 clamps to the end.
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("ten.rs:4294967296:4294967296");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(at(&workbench, cx).1, (10, 0));
}
