//! Follow-ups to the search panel: focus when the dock's rail switches
//! away from search, a click across a rerun, and highlights and selections
//! in lines that aren't valid UTF-8 or whose byte matches split a char.

use std::{fs, ops::Range, path::Path};

use fe_project::{SearchOptions, SearchQuery};
use gpui::{Entity, Focusable as _, Modifiers, MouseButton, TestAppContext, VisualTestContext};

use super::open_root;
use crate::{DockPanel, SearchPanel, Workbench, search_panel::ShownRow};

/// A project with `files` (relative path, bytes) in a temporary directory.
fn project(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in files {
        fs::write(dir.path().join(name), bytes).unwrap();
    }
    dir
}

/// A workbench rooted at `root`, focused as the app focuses it, with search
/// shown, searching on one walker thread so files come in path order.
fn searching<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Entity<Workbench>,
    Entity<SearchPanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let search = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    search.update(cx, |search, _| search.set_options(one_thread()));
    cx.simulate_keystrokes("cmd-shift-f");
    (workbench, search, cx)
}

fn one_thread() -> SearchOptions {
    SearchOptions {
        threads: 1,
        ..SearchOptions::default()
    }
}

/// Replaces the query with `query` and presses Enter.
fn run(query: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input(query);
    cx.simulate_keystrokes("enter");
}

fn shown(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Vec<ShownRow> {
    search.read_with(cx, |search, _| search.shown_rows())
}

fn input_focused(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> bool {
    let input = search.read_with(cx, |search, _| search.input().clone());
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

/// Where result row `ix` was laid out in the last frame.
fn result_center(
    search: &Entity<SearchPanel>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> gpui::Point<gpui::Pixels> {
    search.read_with(cx, |search, _| {
        search.row_bounds(ix).expect("drawn").center()
    })
}

/// The paths of the pane's tabs, as file names.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.tabs()
            .iter()
            .map(|tab| crate::pane::file_name(tab.path.as_deref()))
            .collect()
    })
}

/// The active editor's text and its one selection, and whether it's
/// focused.
fn editor(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (String, Range<usize>, bool) {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows");
    let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let selections = model.selections();
        assert_eq!(selections.len(), 1);
        (
            model.text().to_string(),
            selections[0].range.clone(),
            focused,
        )
    })
}

/// A line row: its number, preview and highlights.
fn line_row(number: u64, preview: &str, highlights: &[(usize, usize)]) -> ShownRow {
    let highlights = highlights.iter().map(|&(start, end)| start..end).collect();
    ShownRow::Line(number, preview.into(), highlights)
}

/// Where `needle` is in `text`.
fn find(text: &str, needle: &str) -> Range<usize> {
    let start = text.find(needle).unwrap();
    start..start + needle.len()
}

#[gpui::test]
fn header_tabs_move_focus_off_the_search_input(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", b"a needle\n")]);
    let (workbench, search, cx) = searching(cx, dir.path());
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let state = |cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, _| (dock.is_visible(), dock.active()))
    };
    let click_tab = |panel: DockPanel, cx: &mut VisualTestContext| {
        let button = dock.read_with(cx, |dock, _| dock.rail_button_bounds(panel).expect("drawn"));
        cx.simulate_click(button.center(), Modifiers::none());
    };
    // Project's button focuses the tree, as cmd-shift-e does.
    let panel_focused = |cx: &mut VisualTestContext| {
        let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
        cx.update(|window, cx| panel.focus_handle(cx).is_focused(window))
    };

    assert!(input_focused(&search, cx));
    click_tab(DockPanel::Project, cx);
    assert_eq!(state(cx), (true, DockPanel::Project));
    assert!(!input_focused(&search, cx));
    assert!(panel_focused(cx));
    cx.simulate_keystrokes("cmd-b");
    assert!(!state(cx).0, "cmd-b works without another click");
    cx.simulate_keystrokes("cmd-shift-f");
    assert_eq!(state(cx), (true, DockPanel::Search));
    assert!(input_focused(&search, cx));

    // With a file open, the outline's button focuses its editor.
    run("needle", cx);
    let row = result_center(&search, 1, cx);
    cx.simulate_click(row, Modifiers::none());
    assert_eq!(tabs(&workbench, cx), ["a.txt"]);
    for panel in [DockPanel::Project, DockPanel::Outline] {
        cx.simulate_keystrokes("cmd-shift-f");
        assert!(input_focused(&search, cx));
        click_tab(panel, cx);
        assert_eq!(state(cx), (true, panel));
        if panel == DockPanel::Project {
            assert!(panel_focused(cx));
        } else {
            assert!(editor(&workbench, cx).2, "the editor has focus");
        }
        cx.simulate_keystrokes("cmd-b");
        assert!(!state(cx).0);
        cx.simulate_keystrokes("cmd-b");
        assert_eq!(state(cx), (true, panel));
    }
    cx.simulate_keystrokes("cmd-shift-f");
    assert_eq!(state(cx), (true, DockPanel::Search));
    assert!(input_focused(&search, cx));

    // The search button focuses the input, as cmd-shift-f does.
    click_tab(DockPanel::Project, cx);
    click_tab(DockPanel::Search, cx);
    assert_eq!(state(cx), (true, DockPanel::Search));
    assert!(input_focused(&search, cx));
}

#[gpui::test]
fn a_rerun_between_press_and_release_opens_no_other_match(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", b"first\n"), ("b.txt", b"second\n")]);
    let (workbench, search, cx) = searching(cx, dir.path());
    run("first", cx);
    let line = |rel: &str, preview: &str| {
        vec![
            ShownRow::File(rel.into(), 1),
            line_row(1, preview, &[(0, preview.len())]),
        ]
    };
    assert_eq!(shown(&search, cx), line("a.txt", "first"));

    // Pressed on a.txt's line; b.txt's takes its place before the release.
    let row = result_center(&search, 1, cx);
    cx.simulate_mouse_down(row, MouseButton::Left, Modifiers::none());
    let mut second = Vec::new();
    fe_project::search(
        dir.path(),
        &SearchQuery::literal("second"),
        &one_thread(),
        Default::default(),
        |batch| second.extend(batch),
    )
    .unwrap();
    search.update(cx, |search, cx| search.rerun_with(second, cx));
    assert_eq!(shown(&search, cx), line("b.txt", "second"));
    assert_eq!(result_center(&search, 1, cx), row);
    cx.simulate_mouse_up(row, MouseButton::Left, Modifiers::none());
    assert_eq!(tabs(&workbench, cx), Vec::<String>::new(), "nothing opens");

    // A click on the new row opens it.
    cx.simulate_click(row, Modifiers::none());
    assert_eq!(tabs(&workbench, cx), ["b.txt"]);
}

#[gpui::test]
fn invalid_utf8_before_a_match_shifts_its_highlight_and_selection(cx: &mut TestAppContext) {
    // b.txt: a 2-byte invalid sequence before a match past the preview's
    // reach, so the preview starts mid-line.
    let mut long = b"\xF0\x9F".to_vec();
    long.extend(b"x".repeat(1_000));
    long.extend(b" needle\n");
    let dir = project(&[("a.txt", b"ab\xFFneedle\n"), ("b.txt", &long)]);
    let (workbench, search, cx) = searching(cx, dir.path());
    run("needle", cx);

    let rows = shown(&search, cx);
    assert_eq!(rows.len(), 4, "{rows:?}");
    assert_eq!(rows[1], line_row(1, "ab\u{FFFD}needle", &[(5, 11)]));
    let ShownRow::Line(_, preview, highlights) = &rows[3] else {
        panic!("{rows:?}");
    };
    assert_eq!(highlights, &[find(preview, "needle")]);

    for ix in [1, 3] {
        let row = result_center(&search, ix, cx);
        cx.simulate_click(row, Modifiers::none());
        let (text, selection, focused) = editor(&workbench, cx);
        assert_eq!(selection, find(&text, "needle"), "row {ix}");
        assert_eq!(&text[selection], "needle");
        assert!(focused);
        cx.simulate_keystrokes("cmd-shift-f");
    }
    let (text, _, _) = editor(&workbench, cx);
    assert!(
        text.starts_with('\u{FFFD}'),
        "the editor decodes it lossily too"
    );
}

#[gpui::test]
fn byte_matches_in_one_char_highlight_it_once(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "éX\n".as_bytes())]);
    let (workbench, search, cx) = searching(cx, dir.path());
    cx.simulate_keystrokes("alt-cmd-r");
    assert!(search.read_with(cx, |search, cx| search.query(cx).regex));
    run(r"(?-u:\xC3|\xA9)", cx);

    // Both bytes of é match, each widened to all of it; X doesn't.
    assert_eq!(
        shown(&search, cx),
        [
            ShownRow::File("a.txt".into(), 2),
            line_row(1, "éX", &[(0, 2)]),
        ]
    );

    // The editor selects all of é too, not a caret before it.
    let row = result_center(&search, 1, cx);
    cx.simulate_click(row, Modifiers::none());
    let (text, selection, _) = editor(&workbench, cx);
    assert_eq!(&text[selection], "é");
}

#[test]
fn decoding_moves_offsets_past_invalid_sequences() {
    // a, \xFF, b, \xF0\x9F (a 4-byte char cut short), c, 😀, d: decoded,
    // a, U+FFFD, b, U+FFFD, c, 😀, d.
    let bytes = b"a\xFFb\xF0\x9Fc\xF0\x9F\x98\x80d";
    let decoded = String::from_utf8_lossy(bytes);
    assert_eq!(decoded, "a\u{FFFD}b\u{FFFD}c😀d");
    let cases = [
        // Unit boundaries map exactly, whichever way they round.
        ((0, false), 0),
        ((1, true), 1),
        ((2, false), 4),
        ((3, true), 5),
        ((5, false), 8),
        ((6, false), 9),
        ((10, true), 13),
        ((11, false), 14),
        // Inside an invalid sequence or a char: its start, or its end.
        ((4, false), 5),
        ((4, true), 8),
        ((7, false), 9),
        ((8, true), 13),
        // Past the end: the end.
        ((20, false), 14),
    ];
    let offsets: Vec<_> = cases.iter().map(|&(offset, _)| offset).collect();
    let expected: Vec<_> = cases.iter().map(|&(_, decoded)| decoded).collect();
    assert_eq!(crate::search_panel::decode(bytes, &offsets), expected);
    assert_eq!(&decoded[9..13], "😀");
}
