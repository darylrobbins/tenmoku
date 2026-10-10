use std::{
    fs,
    ops::Range,
    path::Path,
    sync::{Arc, atomic::AtomicBool, atomic::Ordering},
};

use fe_project::{SearchOptions, SearchQuery, fixture};
use gpui::{
    Entity, Focusable as _, Keystroke, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase, VisualTestContext, point, px,
};

use super::{click_row, open_root, panel, rows};
use crate::{DockPanel, SearchPanel, Workbench, search_panel::ShownRow};

/// A project with `files` (relative path, contents) in a temporary
/// directory.
fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

/// A workbench rooted at `root`, focused as the app focuses it, with search
/// shown and set to `options`.
fn searching<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    options: SearchOptions,
) -> (
    Entity<Workbench>,
    Entity<SearchPanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let search = search(&workbench, cx);
    search.update(cx, |search, _| search.set_options(options));
    cx.simulate_keystrokes("cmd-shift-f");
    (workbench, search, cx)
}

/// Searching on one walker thread, so files come in path order.
fn one_thread() -> SearchOptions {
    SearchOptions {
        threads: 1,
        ..SearchOptions::default()
    }
}

fn search(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<SearchPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    })
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

fn status(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Option<String> {
    search.read_with(cx, |search, _| search.status_text())
}

/// The files with rows, in order.
fn files(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    let rows = shown(search, cx);
    rows.into_iter()
        .filter_map(|row| match row {
            ShownRow::File(rel, _) => Some(rel),
            ShownRow::Line(..) => None,
        })
        .collect()
}

fn input_focused(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> bool {
    let input = search.read_with(cx, |search, _| search.input().clone());
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

/// Clicks result row `ix` where the last frame laid it out.
fn click_result(search: &Entity<SearchPanel>, ix: usize, cx: &mut VisualTestContext) {
    let bounds = search.read_with(cx, |search, _| search.row_bounds(ix).expect("drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
}

/// Presses `keys` without running the executor.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.dispatch_keystroke(Keystroke::parse(keys).unwrap(), cx);
    });
}

#[gpui::test]
fn the_dock_switches_panels(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 2, 3).unwrap();
    fs::write(dir.path().join("notes.txt"), "a needle\n").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let state = |cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, _| (dock.is_visible(), dock.active()))
    };
    let panel = panel(&workbench, cx);
    click_row(&panel, &fixture::dir_name(0), cx);
    let tree = rows(&panel, cx);
    let listings = |cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| panel.tree().unwrap().stats().listings)
    };
    let listed = listings(cx);
    assert_eq!(state(cx), (true, DockPanel::Project));

    cx.simulate_keystrokes("cmd-shift-f");
    let search = search(&workbench, cx);
    assert_eq!(state(cx), (true, DockPanel::Search));
    assert!(input_focused(&search, cx));
    run("needle", cx);
    let results = shown(&search, cx);
    assert_eq!(results.len(), 2, "{results:?}");

    cx.simulate_keystrokes("cmd-shift-e");
    assert_eq!(state(cx), (true, DockPanel::Project));
    assert!(!input_focused(&search, cx), "focus left the hidden input");
    assert_eq!(rows(&panel, cx), tree);
    assert_eq!(listings(cx), listed, "switching lists nothing");
    let built = panel.read_with(cx, |panel, _| panel.built());
    assert!(!built.is_empty(), "the tree is drawn again");

    cx.simulate_keystrokes("cmd-b");
    assert!(!state(cx).0);
    cx.simulate_keystrokes("cmd-b");
    assert_eq!(state(cx), (true, DockPanel::Project));

    // The rail's Search button shows search again: its results stayed, as
    // did its query.
    let button = dock.read_with(cx, |dock, _| {
        dock.rail_button_bounds(DockPanel::Search).expect("drawn")
    });
    cx.simulate_click(button.center(), Modifiers::none());
    assert_eq!(state(cx), (true, DockPanel::Search));
    assert_eq!(shown(&search, cx), results);
    let query = search.read_with(cx, |search, cx| search.query(cx).pattern);
    assert_eq!(query, "needle");
    assert_eq!(listings(cx), listed);
}

#[gpui::test]
fn results_stream_grouped_by_file(cx: &mut TestAppContext) {
    let dir = project(&[
        ("b.txt", "no\nthe needle, the needle\n"),
        ("a.txt", "needle first\n"),
        ("sub/c.txt", "x\ny\nNeedle\n"),
        ("d.txt", "nothing here\n"),
    ]);
    let (_, search, cx) = searching(cx, dir.path(), one_thread());
    run("needle", cx);

    let row = |rel: &str, count| ShownRow::File(rel.into(), count);
    let line = |number, preview: &str, ranges: &[(usize, usize)]| {
        let ranges = ranges.iter().map(|&(start, end)| start..end).collect();
        ShownRow::Line(number, preview.into(), ranges)
    };
    assert_eq!(
        shown(&search, cx),
        [
            row("a.txt", 1),
            line(1, "needle first", &[(0, 6)]),
            row("b.txt", 2),
            line(2, "the needle, the needle", &[(4, 10), (16, 22)]),
            row("sub/c.txt", 1),
            line(3, "Needle", &[(0, 6)]),
        ]
    );
    assert_eq!(status(&search, cx).as_deref(), Some("4 results in 3 files"));

    // The highlights are the search's ranges, in its order.
    let mut expected = Vec::new();
    fe_project::search(
        dir.path(),
        &SearchQuery::literal("needle"),
        &one_thread(),
        Arc::new(AtomicBool::new(false)),
        |batch| expected.extend(batch),
    )
    .unwrap();
    let lines: Vec<_> = shown(&search, cx)
        .into_iter()
        .filter_map(|row| match row {
            ShownRow::Line(_, _, ranges) => Some(ranges),
            ShownRow::File(..) => None,
        })
        .collect();
    let ranges: Vec<_> = expected
        .iter()
        .flat_map(|file| file.lines.iter().map(|line| line.ranges.clone()))
        .collect();
    assert_eq!(lines, ranges);
}

#[gpui::test]
fn options_toggle_and_rerun(cx: &mut TestAppContext) {
    let dir = project(&[
        ("a.txt", "foo1\n"),
        ("b.txt", "foobar\n"),
        ("c.txt", "foo end\n"),
    ]);
    let (_, search, cx) = searching(cx, dir.path(), one_thread());

    // Literal: no match, until the regex toggle reruns it.
    run("fo+[0-9]", cx);
    assert_eq!(status(&search, cx).as_deref(), Some("No results"));
    let regex = search.read_with(cx, |search, _| search.toggle_bounds(0).expect("drawn"));
    cx.simulate_click(regex.center(), Modifiers::none());
    assert_eq!(files(&search, cx), ["a.txt"]);
    assert!(search.read_with(cx, |search, cx| search.query(cx).regex));

    run("FOO", cx);
    assert_eq!(files(&search, cx), ["a.txt", "b.txt", "c.txt"]);
    cx.simulate_keystrokes("alt-cmd-c");
    assert_eq!(status(&search, cx).as_deref(), Some("No results"));
    assert_eq!(shown(&search, cx), []);
    cx.simulate_keystrokes("alt-cmd-c");
    assert_eq!(files(&search, cx), ["a.txt", "b.txt", "c.txt"]);

    run("foo", cx);
    cx.simulate_keystrokes("alt-cmd-w");
    assert_eq!(files(&search, cx), ["c.txt"], "foo1 and foobar are skipped");

    // An invalid regex: its error, and no rows.
    run("fo(", cx);
    let query = search.read_with(cx, |search, cx| search.query(cx));
    let empty = tempfile::tempdir().unwrap();
    let error = fe_project::search(empty.path(), &query, &one_thread(), Arc::default(), |_| {})
        .unwrap_err();
    let line = search.read_with(cx, |search, _| search.status());
    assert_eq!(line, Some((error.to_string().into(), true)));
    assert_eq!(shown(&search, cx), []);
}

#[gpui::test]
fn a_new_search_cancels_the_old(cx: &mut TestAppContext) {
    let dir = project(&[
        ("a.txt", "first\nsecond\n"),
        ("b.txt", "first\n"),
        ("c.txt", "second first second\n"),
    ]);
    let (_, search, cx) = searching(cx, dir.path(), one_thread());
    let input = search.read_with(cx, |search, _| search.input().clone());
    let set_query = |query: &str, cx: &mut VisualTestContext| {
        input.update(cx, |input, cx| input.set_text(query, cx));
    };

    set_query("first", cx);
    press("enter", cx);
    let first = search.read_with(cx, |search, _| search.cancel_flag());
    assert!(!first.load(Ordering::Relaxed));
    set_query("second", cx);
    press("enter", cx);
    let second = search.read_with(cx, |search, _| search.cancel_flag());
    assert!(first.load(Ordering::Relaxed), "the first run is cancelled");
    assert!(!second.load(Ordering::Relaxed));

    cx.run_until_parked();
    let rows = shown(&search, cx);
    assert_eq!(files(&search, cx), ["a.txt", "c.txt"], "{rows:?}");
    for row in rows {
        if let ShownRow::Line(_, preview, _) = row {
            assert!(preview.contains("second"), "{preview}");
        }
    }
    assert_eq!(status(&search, cx).as_deref(), Some("3 results in 2 files"));
}

#[gpui::test]
fn capped_results_say_so(cx: &mut TestAppContext) {
    let text = "hit\n".repeat(5);
    let files: Vec<_> = (0..4)
        .map(|ix| (format!("f{ix}.txt"), text.clone()))
        .collect();
    let files: Vec<_> = files
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    let dir = project(&files);
    let options = SearchOptions {
        max_matches: 10,
        threads: 1,
    };
    let (_, search, cx) = searching(cx, dir.path(), options);
    run("hit", cx);

    let rows = shown(&search, cx);
    let lines = rows
        .iter()
        .filter(|row| matches!(row, ShownRow::Line(..)))
        .count();
    assert_eq!(lines, 10, "{rows:?}");
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Showing the first 10 results")
    );
}

#[gpui::test]
fn clicking_a_match_selects_it(cx: &mut TestAppContext) {
    let dir = project(&[
        ("a.txt", "one\ntwo target\n"),
        ("b.txt", "x\r\ny\r\nzz target\r\n"),
    ]);
    let (workbench, search, cx) = searching(cx, dir.path(), one_thread());
    run("target", cx);
    assert_eq!(files(&search, cx), ["a.txt", "b.txt"]);
    let tabs = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            (pane.tabs().len(), pane.active())
        })
    };
    // The active editor's text, its selection, and whether it's focused.
    let editor = |cx: &mut VisualTestContext| {
        let editor = workbench
            .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
            .expect("an editor shows");
        let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
        editor.read_with(cx, |editor, _| {
            let model = editor.model();
            let selections = model.selections();
            assert_eq!(selections.len(), 1);
            assert!(!selections[0].reversed, "the caret is at the end");
            (
                model.text().to_string(),
                selections[0].range.clone(),
                focused,
            )
        })
    };
    let target = |text: &str| {
        let start = text.find("target").unwrap();
        start..start + "target".len()
    };

    // Rows: a.txt, its line, b.txt, its line.
    click_result(&search, 1, cx);
    assert_eq!(tabs(cx), (1, Some(0)));
    let (text, selection, focused) = editor(cx);
    assert_eq!(text, "one\ntwo target\n");
    assert_eq!(selection, target(&text));
    assert!(focused);

    // CRLF line endings: the buffer has none, the search's offsets do.
    cx.simulate_keystrokes("cmd-shift-f");
    click_result(&search, 3, cx);
    assert_eq!(tabs(cx), (2, Some(1)));
    let (text, selection, focused) = editor(cx);
    assert_eq!(text, "x\ny\nzz target\n");
    assert_eq!(selection, target(&text));
    assert!(focused);

    // Already open: its tab activates and selects the match again.
    cx.simulate_keystrokes("cmd-up");
    let (_, moved, _) = editor(cx);
    assert_eq!(moved, 0..0);
    cx.simulate_keystrokes("cmd-shift-f");
    click_result(&search, 3, cx);
    assert_eq!(tabs(cx), (2, Some(1)));
    let (text, selection, focused) = editor(cx);
    assert_eq!(selection, target(&text));
    assert!(focused);
    click_result(&search, 1, cx);
    assert_eq!(tabs(cx), (2, Some(0)));
    let (text, selection, focused) = editor(cx);
    assert_eq!(selection, target(&text));
    assert!(focused);
}

#[gpui::test]
fn only_visible_rows_are_built(cx: &mut TestAppContext) {
    let dir = project(&[("many.txt", &"match me\n".repeat(1_000))]);
    let (_, search, cx) = searching(cx, dir.path(), one_thread());
    run("match", cx);
    let count = shown(&search, cx).len();
    assert_eq!(count, 1_001, "a header and 1,000 lines");

    // The rows that fit, one more for a partly scrolled one, and the first
    // row measured twice (the list's request_layout and prepaint).
    let (fit, list) = search.read_with(cx, |search, _| {
        let row = search.row_bounds(0).unwrap();
        let list = search.list_bounds();
        ((list.size.height / row.size.height).ceil() as usize, list)
    });
    assert!(fit > 10 && fit < 1000, "{fit} rows fit");
    let built = |cx: &mut VisualTestContext| search.read_with(cx, |search, _| search.built());
    let total = |built: &[Range<usize>]| built.iter().map(|range| range.len()).sum::<usize>();
    let first = built(cx);
    assert!(!first.is_empty());
    assert!(total(&first) <= fit + 1 + 2, "{first:?} for {fit} rows");

    // Scrolling far past the end builds the last rows only.
    cx.simulate_event(ScrollWheelEvent {
        position: list.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-1e9))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    let last = built(cx);
    assert!(total(&last) <= fit + 1 + 2, "{last:?} for {fit} rows");
    assert_eq!(last.last().unwrap().end, count, "{last:?}");
}

#[test]
fn the_default_keymap_binds_the_search_commands() {
    let store = fe_keymap::KeymapStore::load(
        fe_keymap::DEFAULT_MACOS,
        None,
        Some(crate::command_catalog()),
    );
    assert_eq!(store.diagnostics(), []);
    let bound = |command: &str| {
        let id = fe_command::CommandId::parse(command).unwrap();
        store.keymap().bindings_for(&id).len()
    };
    for command in [
        "search.show",
        "dock.show_project",
        "dock.toggle",
        "search.toggle_regex",
        "search.toggle_case",
        "search.toggle_whole_word",
    ] {
        assert_eq!(bound(command), 1, "{command}");
    }
    // Search's input's, and vim's command line's.
    assert_eq!(bound("text_input.submit"), 2);
}
