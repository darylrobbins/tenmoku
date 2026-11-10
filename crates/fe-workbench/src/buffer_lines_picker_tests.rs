//! Search Lines in Buffer: the active editor's rows that aren't blank,
//! matched by subsequence with the matched chars highlighted, Enter going to
//! the row, and a buffer past the cap listed up to it, saying so.

use std::{fs, ops::Range, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{MAX_ROW_CHARS, MAX_ROWS, excerpt};
use crate::{PickerDelegate as _, Workbench, tests::open_root, workbench::Palette};

/// A workbench showing `notes.txt` holding `text`, focused.
fn opened<'a>(
    text: &str,
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    fs::write(&path, text).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, _| window.activate_window());
    open(&workbench, &path, cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    (dir, workbench, editor, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Opens `path` in the active pane, focused.
fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|window, cx| {
        let focus = pane.read(cx).focus_target(cx);
        window.focus(&focus, cx);
    });
    draw(cx);
}

/// Runs command `id` where focus is, in the workbench's context.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

type Rows = Vec<(u32, String, Vec<Range<usize>>)>;

/// The picker's rows (one-based row, text, highlights) and its status.
fn rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (Rows, Option<String>) {
    workbench.read_with(cx, |workbench, cx| {
        let picker = (workbench.buffer_lines_picker(cx)).expect("the lines picker shows");
        let delegate = &picker.read(cx).delegate;
        let status = delegate.status().map(|status| status.to_string());
        (delegate.rows(), status)
    })
}

/// Types `query` into the picker's input.
fn type_query(workbench: &Entity<Workbench>, query: &str, cx: &mut VisualTestContext) {
    let picker = (workbench.read_with(cx, |workbench, cx| workbench.buffer_lines_picker(cx)))
        .expect("the lines picker shows");
    let input = picker.read_with(cx, |picker, _| picker.input().clone());
    // Setting the text doesn't say it changed.
    input.update(cx, |input, cx| input.set_text(query, cx));
    picker.update(cx, |picker, cx| {
        picker.delegate.update_matches(query.to_owned(), cx)
    });
    draw(cx);
}

fn palette(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Palette> {
    workbench.read_with(cx, |workbench, _| workbench.palette)
}

#[gpui::test]
fn skips_blank_rows(cx: &mut TestAppContext) {
    let text = "alpha\n\n   \n  beta  \n\tgamma\n";
    let (_dir, workbench, _, cx) = opened(text, cx);
    run("search.buffer_lines", cx);
    assert_eq!(palette(&workbench, cx), Some(Palette::BufferLines));
    let (listed, status) = rows(&workbench, cx);
    let shown: Vec<(u32, String)> = (listed.into_iter())
        .map(|(row, text, _)| (row, text))
        .collect();
    // Each from its first char that isn't whitespace, trailing space gone.
    assert_eq!(
        shown,
        [
            (1, "alpha".to_owned()),
            (4, "beta".to_owned()),
            (5, "gamma".to_owned())
        ]
    );
    assert_eq!(status, None);
}

#[gpui::test]
fn subsequence_match_highlights(cx: &mut TestAppContext) {
    let text = "fn main() {}\n    let total = sum();\nfn other() {}\n";
    let (_dir, workbench, editor, cx) = opened(text, cx);
    run("search.buffer_lines", cx);

    // `tsm` is t…s…m of `let total = sum();` alone, ignoring case.
    type_query(&workbench, "TSM", cx);
    let (listed, _) = rows(&workbench, cx);
    assert_eq!(
        listed,
        [(
            2,
            "let total = sum();".to_owned(),
            vec![2..3, 12..13, 14..15]
        )]
    );

    // Enter puts the caret at the row's first char that isn't whitespace,
    // focused.
    let picker = workbench.read_with(cx, |workbench, cx| workbench.buffer_lines_picker(cx));
    let picker = picker.expect("the lines picker shows");
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(0, window, cx)));
    draw(cx);
    assert_eq!(palette(&workbench, cx), None);
    let at = text.find("let").unwrap();
    let selection = editor.read_with(cx, |editor, _| editor.model().primary_selection().range);
    assert_eq!(selection, at..at);
    let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    assert!(focused, "the editor has focus");
}

#[gpui::test]
fn caps_a_huge_buffer_and_says_so(cx: &mut TestAppContext) {
    let extra = 25;
    let text: String = (1..=MAX_ROWS + extra)
        .map(|row| format!("row {row}\n"))
        .collect();
    let (_dir, workbench, _, cx) = opened(&text, cx);
    run("search.buffer_lines", cx);
    let (listed, status) = rows(&workbench, cx);
    assert_eq!(listed.len(), MAX_ROWS as usize);
    assert_eq!(listed.last().unwrap().0, MAX_ROWS);
    assert_eq!(status.as_deref(), Some("The first 50,000 rows"));

    // A row past the cap matches nothing: it isn't listed.
    type_query(&workbench, &format!("row {}", MAX_ROWS + 1), cx);
    let (listed, status) = rows(&workbench, cx);
    assert_eq!(listed, []);
    assert_eq!(status.as_deref(), Some("No matching lines"));
}

/// A row is matched whole: a query found past the chars a row shows still
/// lists it, and its excerpt shows the match, highlighted.
// One highlighted range, not a range of them.
#[allow(clippy::single_range_in_vec_init)]
#[gpui::test]
fn matches_past_the_chars_a_row_shows(cx: &mut TestAppContext) {
    let long = format!("{}needle{}", "a".repeat(1_200), "b".repeat(3_000));
    let text = format!("short\n{long}\n");
    let (_dir, workbench, _, cx) = opened(&text, cx);
    run("search.buffer_lines", cx);
    type_query(&workbench, "needle", cx);
    let (listed, status) = rows(&workbench, cx);
    assert_eq!(listed, [(2, long.clone(), vec![1_200..1_206])]);
    assert_eq!(status, None);

    let (shown, highlights) = excerpt(&long, &listed[0].2);
    assert_eq!(shown.chars().count(), MAX_ROW_CHARS + 2);
    assert!(shown.starts_with('…') && shown.ends_with('…'), "{shown}");
    assert_eq!(highlights.len(), 1);
    assert_eq!(&shown[highlights[0].clone()], "needle");
}

/// A row no longer than the cap shows whole; a longer one with nothing
/// matched shows its start.
// One highlighted range, not a range of them.
#[allow(clippy::single_range_in_vec_init)]
#[test]
fn excerpts_cut_only_long_rows() {
    assert_eq!(excerpt("short", &[1..2]), ("short".to_owned(), vec![1..2]));
    let long = "é".repeat(MAX_ROW_CHARS + 5);
    let (shown, highlights) = excerpt(&long, &[]);
    assert_eq!(shown, format!("{}…", "é".repeat(MAX_ROW_CHARS)));
    assert!(highlights.is_empty(), "{highlights:?}");
}

/// Focus moving off the picker hides it without a dismissal: the preview's
/// scroll goes back all the same, and focus stays where it went.
#[gpui::test]
fn focus_moving_away_restores_the_scroll(cx: &mut TestAppContext) {
    let text: String = (1..=300).map(|row| format!("row {row}\n")).collect();
    let (_dir, workbench, editor, cx) = opened(&text, cx);
    let top = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.scroll_top());
    assert_eq!(top(cx), 0.);
    run("search.buffer_lines", cx);
    let picker = (workbench.read_with(cx, |workbench, cx| workbench.buffer_lines_picker(cx)))
        .expect("the lines picker shows");
    picker.update(cx, |picker, cx| picker.select(249, cx));
    draw(cx);
    assert!(top(cx) > 0., "the row is previewed");
    drop(picker);

    let elsewhere = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));
    cx.update(|window, cx| window.focus(&elsewhere, cx));
    draw(cx);
    let shown = workbench.read_with(cx, |workbench, cx| workbench.buffer_lines_picker(cx));
    assert!(shown.is_none(), "the picker is hidden");
    assert_eq!(top(cx), 0., "the scroll is back");
    assert!(cx.update(|window, _| elsewhere.is_focused(window)));
}
