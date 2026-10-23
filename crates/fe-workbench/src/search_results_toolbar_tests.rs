//! The results toolbar (Refresh, Clear Search Results, Collapse All /
//! Expand All) and the collapsible file groups, with the results list's
//! left, right and enter, as in VS Code's Search view.

use std::{fs, path::Path, sync::atomic::Ordering};

use fe_command::{CommandId, ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_project::SearchOptions;
use fe_ui::{Icon, TextInput};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext, point, px};

use super::{ShownRow, registry};
use crate::{SearchPanel, Workbench, tests::open_root};

/// A project of a.rs, with two matching lines, and b.rs with one.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "foo a\nx\nfoo b\n").unwrap();
    fs::write(dir.path().join("b.rs"), "foo c\n").unwrap();
    dir
}

/// A workbench rooted at `root`, focused, with search shown on one walker
/// thread (so files come in path order).
fn panel<'a>(
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
    search.update(cx, |search, _| {
        search.set_options(SearchOptions {
            threads: 1,
            ..SearchOptions::default()
        })
    });
    cx.simulate_keystrokes("cmd-shift-f");
    draw(cx);
    (workbench, search, cx)
}

/// [`panel`], with `foo` run. Rows: a.rs, its two lines, b.rs, its line.
fn searched<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Entity<Workbench>,
    Entity<SearchPanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, search, cx) = panel(cx, root);
    cx.simulate_input("foo");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(rows(&search, cx).len(), 5);
    (workbench, search, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
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

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

/// Clicks result row `ix` near its left edge, clear of its buttons.
fn click_row(search: &Entity<SearchPanel>, ix: usize, cx: &mut VisualTestContext) {
    let bounds = search.read_with(cx, |search, _| search.row_bounds(ix).expect("drawn"));
    let at = point(bounds.origin.x + px(40.), bounds.center().y);
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

/// Each toolbar button's tooltip and whether it's enabled.
fn toolbar(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> [(&'static str, bool); 4] {
    search.read_with(cx, |search, cx| {
        search
            .toolbar_buttons(cx)
            .map(|(_, _, tooltip, enabled, _)| (tooltip, enabled))
    })
}

/// The rows, a file's as its name and a line's as its number.
fn rows(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    let rows = search.read_with(cx, |search, _| search.shown_rows());
    (rows.into_iter())
        .map(|row| match row {
            ShownRow::Folder(label, _) | ShownRow::File(label, _) => label,
            ShownRow::Line(number, ..) => number.to_string(),
        })
        .collect()
}

fn selected(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Option<usize> {
    search.read_with(cx, |search, _| search.selected())
}

fn status(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Option<String> {
    search.read_with(cx, |search, _| search.status_text())
}

fn focused(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

fn text(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> String {
    input.read_with(cx, |input, _| input.text().to_owned())
}

#[gpui::test]
fn refresh_disabled_without_query(cx: &mut TestAppContext) {
    let dir = project();
    let (_workbench, search, cx) = panel(cx, dir.path());
    assert_eq!(toolbar(&search, cx)[0], ("Refresh", false));
    click("search-toolbar-refresh", cx);
    run("search.refresh", cx);
    assert_eq!(status(&search, cx), None);
    assert!(rows(&search, cx).is_empty());

    // Typed, not yet run: Refresh runs it.
    cx.simulate_input("foo");
    draw(cx);
    assert_eq!(toolbar(&search, cx)[0], ("Refresh", true));
    click("search-toolbar-refresh", cx);
    assert_eq!(status(&search, cx).as_deref(), Some("3 results in 2 files"));
}

#[gpui::test]
fn refresh_reruns_query(cx: &mut TestAppContext) {
    let dir = project();
    let (_workbench, search, cx) = searched(cx, dir.path());
    run("search.toggle_case", cx);
    fs::write(dir.path().join("c.rs"), "foo d\nFOO\n").unwrap();
    fs::write(dir.path().join("b.rs"), "Foo\n").unwrap();
    let generation = search.read_with(cx, |search, _| search.generation);
    click("search-toolbar-refresh", cx);
    assert!(search.read_with(cx, |search, _| search.generation) > generation);
    assert_eq!(rows(&search, cx), ["a.rs", "1", "3", "c.rs", "1"]);
    assert!(
        search.read_with(cx, |search, _| search.match_case),
        "the options stay"
    );

    // And by its command.
    fs::write(dir.path().join("b.rs"), "foo\n").unwrap();
    run("search.refresh", cx);
    assert_eq!(
        rows(&search, cx),
        ["a.rs", "1", "3", "b.rs", "1", "c.rs", "1"]
    );
}

#[gpui::test]
fn clear_disabled_when_pristine(cx: &mut TestAppContext) {
    let dir = project();
    let (_workbench, search, cx) = panel(cx, dir.path());
    assert_eq!(toolbar(&search, cx)[1], ("Clear Search Results", false));
    let fields = search.read_with(cx, |search, _| {
        [
            search.input().clone(),
            search.replace_input().clone(),
            search.include_input().clone(),
            search.exclude_input().clone(),
        ]
    });
    // Any field with text enables it, and clearing disables it again.
    for field in &fields {
        field.update(cx, |input, cx| input.set_text("x", cx));
        draw(cx);
        assert!(toolbar(&search, cx)[1].1);
        click("search-toolbar-clear", cx);
        assert_eq!(text(field, cx), "");
        assert!(!toolbar(&search, cx)[1].1);
    }

    // So do results, though the query that found them has gone.
    let query = fields[0].clone();
    query.update(cx, |input, cx| input.set_text("foo", cx));
    search.update(cx, |search, cx| search.run(cx));
    draw(cx);
    query.update(cx, |input, cx| input.set_text("", cx));
    draw(cx);
    assert!(toolbar(&search, cx)[1].1);
    click("search-toolbar-clear", cx);
    assert!(rows(&search, cx).is_empty());
    assert!(!toolbar(&search, cx)[1].1);
}

#[gpui::test]
fn clear_resets_fields_results_and_focuses_query(cx: &mut TestAppContext) {
    let dir = project();
    let (_workbench, search, cx) = searched(cx, dir.path());
    run("search.toggle_regex", cx);
    run("search.toggle_replace", cx);
    run("search.toggle_details", cx);
    let (query, replace, include, exclude) = search.read_with(cx, |search, _| {
        (
            search.input().clone(),
            search.replace_input().clone(),
            search.include_input().clone(),
            search.exclude_input().clone(),
        )
    });
    replace.update(cx, |input, cx| input.set_text("bar", cx));
    include.update(cx, |input, cx| input.insert("*.rs", cx));
    exclude.update(cx, |input, cx| input.insert("b.rs", cx));
    draw(cx);
    assert_eq!(rows(&search, cx), ["a.rs", "1", "3"]);
    cx.update(|window, cx| window.focus(&replace.focus_handle(cx), cx));

    // With a search running, which it cancels: none of its batches land.
    search.update(cx, |search, cx| search.run(cx));
    let running = search.read_with(cx, |search, _| search.cancel_flag());
    cx.update(|window, cx| search.update(cx, |search, cx| search.clear_results(window, cx)));
    assert!(running.load(Ordering::Relaxed));
    draw(cx);

    for input in [&query, &replace, &include, &exclude] {
        assert_eq!(text(input, cx), "");
    }
    assert!(rows(&search, cx).is_empty());
    assert_eq!(status(&search, cx), None);
    assert!(focused(&query, cx));
    search.read_with(cx, |search, _| {
        assert!(search.regex, "the options stay");
        assert!(search.replace_shown && search.details_shown());
    });

    // The button does the same.
    query.update(cx, |input, cx| input.set_text("foo", cx));
    search.update(cx, |search, cx| search.run(cx));
    draw(cx);
    cx.update(|window, cx| window.focus(&replace.focus_handle(cx), cx));
    click("search-toolbar-clear", cx);
    assert_eq!(text(&query, cx), "");
    assert!(rows(&search, cx).is_empty());
    assert!(focused(&query, cx));
}

/// Cleared while a replacement runs: its writes finish, but it neither
/// searches again nor puts its outcome back in the status.
#[gpui::test]
fn clear_during_a_replacement_leaves_the_status_empty(cx: &mut TestAppContext) {
    let dir = project();
    let (_workbench, search, cx) = searched(cx, dir.path());
    let replace = search.read_with(cx, |search, _| search.replace_input().clone());
    replace.update(cx, |input, cx| input.set_text("bar", cx));
    let (layout, query) = search.read_with(cx, |search, _| (search.layout, search.input().clone()));
    cx.update(|window, cx| {
        search.update(cx, |search, cx| {
            search.replace_in_file(layout, 1, cx);
            assert!(search.replacing.is_some());
            search.clear_results(window, cx);
        })
    });
    draw(cx);
    assert_eq!(
        fs::read_to_string(dir.path().join("b.rs")).unwrap(),
        "bar c\n"
    );
    search.read_with(cx, |search, _| assert!(search.replacing.is_none()));
    assert_eq!(status(&search, cx), None);
    assert!(rows(&search, cx).is_empty());
    assert_eq!(text(&query, cx), "");
    assert!(!toolbar(&search, cx)[1].1);
}

#[gpui::test]
fn click_file_row_collapses_and_expands(cx: &mut TestAppContext) {
    let dir = project();
    let (_workbench, search, cx) = searched(cx, dir.path());
    assert!(cx.debug_bounds("search-chevron:0:open").is_some());
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx), ["a.rs", "b.rs", "1"]);
    assert!(cx.debug_bounds("search-chevron:0:closed").is_some());
    assert!(cx.debug_bounds("search-chevron:1:open").is_some());
    assert_eq!(selected(&search, cx), Some(0));
    // Still in the results, and the counts.
    assert_eq!(status(&search, cx).as_deref(), Some("3 results in 2 files"));
    let shown = search.read_with(cx, |search, _| search.shown_rows());
    assert_eq!(shown[0], ShownRow::File("a.rs".into(), 2));

    click_row(&search, 1, cx);
    assert_eq!(rows(&search, cx), ["a.rs", "b.rs"]);
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx), ["a.rs", "1", "3", "b.rs"]);
    assert!(cx.debug_bounds("search-chevron:0:open").is_some());

    // The row's dismiss button doesn't toggle it.
    let bounds = cx.debug_bounds("search-dismiss:3").expect("dismiss");
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
    assert_eq!(rows(&search, cx), ["a.rs", "1", "3"]);
}

#[gpui::test]
fn collapsed_file_still_replaced_by_replace_all(cx: &mut TestAppContext) {
    let dir = project();
    let (_workbench, search, cx) = searched(cx, dir.path());
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx), ["a.rs", "b.rs", "1"]);
    run("search.toggle_replace", cx);
    let replace = search.read_with(cx, |search, _| search.replace_input().clone());
    replace.update(cx, |input, cx| input.set_text("bar", cx));
    let query = search.read_with(cx, |search, _| search.input().clone());
    cx.update(|window, cx| window.focus(&query.focus_handle(cx), cx));
    draw(cx);
    run("search.replace_all", cx);
    assert_eq!(
        cx.pending_prompt().map(|(question, _)| question).as_deref(),
        Some("Replace 3 occurrences across 2 files with “bar”?")
    );
    cx.simulate_prompt_answer("Replace");
    draw(cx);
    let read = |name| fs::read_to_string(dir.path().join(name)).unwrap();
    assert_eq!(read("a.rs"), "bar a\nx\nbar b\n");
    assert_eq!(read("b.rs"), "bar c\n");

    // Its own Replace button too, collapsed.
    fs::write(dir.path().join("a.rs"), "foo\n").unwrap();
    replace.update(cx, |input, cx| input.set_text("baz", cx));
    query.update(cx, |input, cx| input.set_text("foo", cx));
    search.update(cx, |search, cx| search.run(cx));
    draw(cx);
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx), ["a.rs"]);
    click("search-replace-file:0", cx);
    assert_eq!(read("a.rs"), "baz\n");
}

#[gpui::test]
fn left_right_enter_on_file_rows(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, search, cx) = searched(cx, dir.path());
    run("search.focus_results", cx);
    assert_eq!(selected(&search, cx), Some(1));
    cx.simulate_keystrokes("up");
    draw(cx);
    assert_eq!(selected(&search, cx), Some(0), "up stops on the file");

    cx.simulate_keystrokes("left");
    draw(cx);
    assert_eq!(rows(&search, cx), ["a.rs", "b.rs", "1"]);
    assert_eq!(selected(&search, cx), Some(0));
    cx.simulate_keystrokes("left");
    draw(cx);
    assert_eq!(rows(&search, cx), ["a.rs", "b.rs", "1"]);
    assert_eq!(selected(&search, cx), Some(0), "a collapsed file stays");
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(selected(&search, cx), Some(1), "past the hidden lines");
    cx.simulate_keystrokes("up");
    cx.simulate_keystrokes("right");
    draw(cx);
    assert_eq!(rows(&search, cx), ["a.rs", "1", "3", "b.rs", "1"]);
    assert_eq!(selected(&search, cx), Some(0));
    cx.simulate_keystrokes("right");
    draw(cx);
    assert_eq!(selected(&search, cx), Some(1), "to its first line");
    cx.simulate_keystrokes("right");
    draw(cx);
    assert_eq!(selected(&search, cx), Some(1), "a line has no children");

    // Enter on a file's row opens it at its first match.
    cx.simulate_keystrokes("down down");
    draw(cx);
    assert_eq!(selected(&search, cx), Some(3));
    cx.simulate_keystrokes("enter");
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows");
    let (path, selection) = editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (
            model.path().unwrap().to_owned(),
            model.selections()[0].range.clone(),
        )
    });
    assert_eq!(path, dir.path().join("b.rs"));
    assert_eq!(selection, 0..3);
}

#[gpui::test]
fn left_on_line_selects_parent(cx: &mut TestAppContext) {
    let dir = project();
    let (_workbench, search, cx) = searched(cx, dir.path());
    run("search.focus_results", cx);
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(selected(&search, cx), Some(2));
    cx.simulate_keystrokes("left");
    draw(cx);
    assert_eq!(selected(&search, cx), Some(0));
    assert_eq!(rows(&search, cx).len(), 5, "nothing collapses");
    cx.simulate_keystrokes("down down down down");
    draw(cx);
    assert_eq!(selected(&search, cx), Some(4));
    cx.simulate_keystrokes("left");
    draw(cx);
    assert_eq!(selected(&search, cx), Some(3));

    // A line hidden by a collapse leaves its file selected.
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(selected(&search, cx), Some(4));
    run("search.collapse_all", cx);
    assert_eq!(rows(&search, cx), ["a.rs", "b.rs"]);
    assert_eq!(selected(&search, cx), Some(1));
}

#[gpui::test]
fn collapse_all_flips_to_expand_all(cx: &mut TestAppContext) {
    let dir = project();
    let (_workbench, search, cx) = panel(cx, dir.path());
    assert_eq!(toolbar(&search, cx)[3], ("Collapse All", false));
    cx.simulate_input("foo");
    cx.simulate_keystrokes("enter");
    draw(cx);
    let icon = |cx: &mut VisualTestContext| {
        search.read_with(cx, |search, cx| search.toolbar_buttons(cx)[3].1)
    };
    assert_eq!(toolbar(&search, cx)[3], ("Collapse All", true));
    assert_eq!(icon(cx), Icon::FoldVertical);

    click("search-toolbar-collapse", cx);
    assert_eq!(rows(&search, cx), ["a.rs", "b.rs"]);
    assert_eq!(toolbar(&search, cx)[3], ("Expand All", true));
    assert_eq!(icon(cx), Icon::UnfoldVertical);
    click("search-toolbar-collapse", cx);
    assert_eq!(rows(&search, cx), ["a.rs", "1", "3", "b.rs", "1"]);
    assert_eq!(toolbar(&search, cx)[3], ("Collapse All", true));

    // It flips once the last group is collapsed by hand, and back once one
    // is expanded.
    click_row(&search, 0, cx);
    assert_eq!(toolbar(&search, cx)[3].0, "Collapse All");
    click_row(&search, 1, cx);
    assert_eq!(toolbar(&search, cx)[3].0, "Expand All");
    click_row(&search, 1, cx);
    assert_eq!(toolbar(&search, cx)[3].0, "Collapse All");

    // The commands, which the palette lists.
    run("search.collapse_all", cx);
    assert_eq!(rows(&search, cx), ["a.rs", "b.rs"]);
    run("search.expand_all", cx);
    assert_eq!(rows(&search, cx).len(), 5);
    let registry = registry();
    for (id, title) in [
        ("search.refresh", "Refresh"),
        ("search.clear_results", "Clear Search Results"),
        ("search.collapse_all", "Collapse All"),
        ("search.expand_all", "Expand All"),
    ] {
        let meta = registry.get(&CommandId::parse(id).unwrap()).expect(id);
        assert_eq!(meta.title, title);
    }
}

#[gpui::test]
fn new_search_resets_collapse(cx: &mut TestAppContext) {
    let dir = project();
    let (_workbench, search, cx) = searched(cx, dir.path());
    click("search-toolbar-collapse", cx);
    assert_eq!(rows(&search, cx), ["a.rs", "b.rs"]);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(rows(&search, cx).len(), 5, "a new search");
    click("search-toolbar-collapse", cx);
    click("search-toolbar-refresh", cx);
    assert_eq!(rows(&search, cx).len(), 5, "a refresh");

    // A dismissal keeps each other file's state, and new files of the same
    // run come in expanded.
    fs::write(dir.path().join("c.rs"), "foo\n").unwrap();
    click("search-toolbar-refresh", cx);
    click_row(&search, 3, cx);
    assert_eq!(rows(&search, cx), ["a.rs", "1", "3", "b.rs", "c.rs", "1"]);
    click("search-dismiss:0", cx);
    assert_eq!(rows(&search, cx), ["b.rs", "c.rs", "1"]);
    let (generation, more) = search.read_with(cx, |search, _| {
        (search.generation, vec![search.files[1].clone()])
    });
    search.update(cx, |search, cx| search.append(generation, more, cx));
    draw(cx);
    assert_eq!(rows(&search, cx), ["b.rs", "c.rs", "1", "c.rs", "1"]);
}
