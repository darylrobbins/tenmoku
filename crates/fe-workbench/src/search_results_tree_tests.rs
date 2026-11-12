//! View as Tree / View as List in the search results: the files grouped
//! under folder rows, single-child folder chains compacted, and the folder
//! rows' collapse, counts, Replace and dismiss, as in VS Code's Search view.

use std::{ffi::OsStr, fs, os::unix::ffi::OsStrExt as _, path::Path};

use fe_command::{CommandId, ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_project::{FileMatches, LineMatch, SearchOptions};
use fe_ui::{Icon, TREE_INDENT};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext, point, px};

use super::{ShownRow, registry};
use crate::{SearchPanel, Workbench, tests::open_root};

/// A project of `files`, each a relative path and its text.
fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

/// src/a.rs with two matching lines, src/b.rs and top.rs with one each.
fn src_project() -> tempfile::TempDir {
    project(&[
        ("src/a.rs", "foo a\nx\nfoo b\n"),
        ("src/b.rs", "foo c\n"),
        ("top.rs", "foo d\n"),
    ])
}

/// A workbench rooted at `root`, focused, with `foo` searched on one walker
/// thread (so files come in path order).
fn searched<'a>(
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
    cx.simulate_input("foo");
    cx.simulate_keystrokes("enter");
    draw(cx);
    (workbench, search, cx)
}

/// [`searched`], then switched to the tree by its command.
fn tree<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Entity<Workbench>,
    Entity<SearchPanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, search, cx) = searched(cx, root);
    run("search.view_as_tree", cx);
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

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// The view mode action's icon and tooltip.
fn view_button(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> (Icon, String) {
    search.read_with(cx, |search, _| {
        let action = &search.pane_actions()[2];
        (action.icon, action.title.to_string())
    })
}

fn collapse_button(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> String {
    search.read_with(cx, |search, _| search.pane_actions()[3].title.to_string())
}

/// The rows, a folder's and a file's as its label and a line's as its
/// number.
fn rows(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    let rows = search.read_with(cx, |search, _| search.shown_rows());
    (rows.into_iter())
        .map(|row| match row {
            ShownRow::Folder(label, _) | ShownRow::File(label, _) => label,
            ShownRow::Line(number, ..) => number.to_string(),
        })
        .collect()
}

fn depths(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Vec<usize> {
    search.read_with(cx, |search, _| search.depths())
}

fn selected(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Option<usize> {
    search.read_with(cx, |search, _| search.selected())
}

fn read(dir: &tempfile::TempDir, rel: &str) -> String {
    fs::read_to_string(dir.path().join(rel)).unwrap()
}

/// Shows the replace field with `template` in it, the query focused.
fn replace_with(template: &str, search: &Entity<SearchPanel>, cx: &mut VisualTestContext) {
    run("search.toggle_replace", cx);
    let (replace, query) = search.read_with(cx, |search, _| {
        (search.replace_input().clone(), search.input().clone())
    });
    replace.update(cx, |input, cx| input.set_text(template, cx));
    cx.update(|window, cx| window.focus(&query.focus_handle(cx), cx));
    draw(cx);
}

/// Runs Replace All and returns its question, unanswered.
fn replace_all_question(cx: &mut VisualTestContext) -> Option<String> {
    run("search.replace_all", cx);
    cx.pending_prompt().map(|(question, _)| question)
}

#[gpui::test]
fn tree_groups_files_under_folders(cx: &mut TestAppContext) {
    let dir = src_project();
    let (_workbench, search, cx) = searched(cx, dir.path());
    // The list is the default.
    assert_eq!(
        rows(&search, cx),
        ["src/a.rs", "1", "3", "src/b.rs", "1", "top.rs", "1"]
    );
    assert_eq!(
        view_button(&search, cx),
        (Icon::ListTree, "View as Tree".into())
    );

    click("icon-button:search-view-mode", cx);
    assert_eq!(
        view_button(&search, cx),
        (Icon::List, "View as List".into())
    );
    assert_eq!(
        rows(&search, cx),
        ["src", "a.rs", "1", "3", "b.rs", "1", "top.rs", "1"]
    );
    assert_eq!(depths(&search, cx), [0, 1, 2, 2, 1, 2, 0, 1]);
    assert!(cx.debug_bounds("search-chevron:0:open").is_some());

    // Each depth indents its row's chevron one step.
    let left = |selector: &'static str, cx: &mut VisualTestContext| {
        cx.debug_bounds(selector).expect(selector).left()
    };
    let folder = left("search-chevron:0:open", cx);
    assert_eq!(left("search-chevron:1:open", cx) - folder, TREE_INDENT);
    assert_eq!(left("search-chevron:6:open", cx), folder);

    // The commands, which the palette lists.
    let registry = registry();
    for (id, title) in [
        ("search.view_as_tree", "View as Tree"),
        ("search.view_as_list", "View as List"),
    ] {
        let meta = registry.get(&CommandId::parse(id).unwrap()).expect(id);
        assert_eq!(meta.title, title);
    }
}

#[gpui::test]
fn tree_compacts_single_child_folder_chains(cx: &mut TestAppContext) {
    let dir = project(&[
        ("a/b/c.rs", "foo\n"),
        ("a/b/d/e.rs", "foo\n"),
        ("deep/x/y/z.rs", "foo\n"),
        ("top.rs", "foo\n"),
    ]);
    let (_workbench, search, cx) = tree(cx, dir.path());
    // a holds only b, which holds a file too; x and y hold one folder or
    // file each, so deep/x/y is one row.
    assert_eq!(
        rows(&search, cx),
        [
            "a/b", "d", "e.rs", "1", "c.rs", "1", "deep/x/y", "z.rs", "1", "top.rs", "1"
        ]
    );
    assert_eq!(depths(&search, cx), [0, 1, 2, 3, 1, 2, 0, 1, 2, 0, 1]);

    // A compacted row collapses as one.
    click_row(&search, 6, cx);
    assert_eq!(
        rows(&search, cx),
        [
            "a/b", "d", "e.rs", "1", "c.rs", "1", "deep/x/y", "top.rs", "1"
        ]
    );
}

#[gpui::test]
fn tree_does_not_compact_branching_folders(cx: &mut TestAppContext) {
    let dir = project(&[
        ("branch/one/p.rs", "foo\n"),
        ("branch/two/q.rs", "foo\n"),
        ("mixed/m.rs", "foo\n"),
        ("mixed/sub/n.rs", "foo\n"),
    ]);
    let (_workbench, search, cx) = tree(cx, dir.path());
    // branch holds two folders and mixed a folder and a file: neither
    // merges with what it holds.
    assert_eq!(
        rows(&search, cx),
        [
            "branch", "one", "p.rs", "1", "two", "q.rs", "1", "mixed", "sub", "n.rs", "1", "m.rs",
            "1"
        ]
    );
    assert_eq!(depths(&search, cx), [0, 1, 2, 3, 1, 2, 3, 0, 1, 2, 3, 1, 2]);
}

#[gpui::test]
fn collapsing_a_folder_hides_descendants_but_replace_all_keeps_them(cx: &mut TestAppContext) {
    let dir = src_project();
    let (_workbench, search, cx) = tree(cx, dir.path());
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx), ["src", "top.rs", "1"]);
    assert!(cx.debug_bounds("search-chevron:0:closed").is_some());
    assert_eq!(selected(&search, cx), Some(0));
    // Still in the results, and the counts.
    let status = search.read_with(cx, |search, _| search.status_text());
    assert_eq!(status.as_deref(), Some("4 results in 3 files"));
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx).len(), 8);

    // Left collapses a folder, Right expands it and then goes to its first
    // child, and Enter toggles it.
    run("search.focus_results", cx);
    keys("down down", cx);
    assert_eq!(selected(&search, cx), Some(2));
    keys("left", cx);
    assert_eq!(selected(&search, cx), Some(1), "a line's file");
    keys("left", cx);
    assert_eq!(
        rows(&search, cx),
        ["src", "a.rs", "b.rs", "1", "top.rs", "1"]
    );
    keys("left", cx);
    assert_eq!(selected(&search, cx), Some(0), "a collapsed file's folder");
    keys("left", cx);
    assert_eq!(rows(&search, cx), ["src", "top.rs", "1"]);
    keys("left", cx);
    assert_eq!(selected(&search, cx), Some(0), "a top-level folder stays");
    keys("right", cx);
    assert_eq!(
        rows(&search, cx),
        ["src", "a.rs", "b.rs", "1", "top.rs", "1"]
    );
    keys("right", cx);
    assert_eq!(selected(&search, cx), Some(1), "to its first child");
    keys("up enter", cx);
    assert_eq!(rows(&search, cx), ["src", "top.rs", "1"]);
    keys("enter", cx);
    assert_eq!(rows(&search, cx).len(), 6);

    // A selected line whose folder's collapse hides it leaves the folder
    // selected.
    keys("down down down", cx);
    assert_eq!(selected(&search, cx), Some(3));
    run("search.collapse_all", cx);
    assert_eq!(rows(&search, cx), ["src", "top.rs"]);
    assert_eq!(selected(&search, cx), Some(0));

    replace_with("bar", &search, cx);
    assert_eq!(
        replace_all_question(cx).as_deref(),
        Some("Replace 4 occurrences across 3 files with “bar”?")
    );
    cx.simulate_prompt_answer("Replace");
    draw(cx);
    assert_eq!(read(&dir, "src/a.rs"), "bar a\nx\nbar b\n");
    assert_eq!(read(&dir, "src/b.rs"), "bar c\n");
    assert_eq!(read(&dir, "top.rs"), "bar d\n");
}

#[gpui::test]
fn collapse_all_counts_folders(cx: &mut TestAppContext) {
    let dir = src_project();
    let (_workbench, search, cx) = tree(cx, dir.path());
    assert_eq!(collapse_button(&search, cx), "Collapse All");

    // Every file collapsed by hand, with its folder open: still Collapse
    // All.
    click_row(&search, 1, cx);
    click_row(&search, 2, cx);
    click_row(&search, 3, cx);
    assert_eq!(rows(&search, cx), ["src", "a.rs", "b.rs", "top.rs"]);
    assert_eq!(collapse_button(&search, cx), "Collapse All");
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx), ["src", "top.rs"]);
    assert_eq!(collapse_button(&search, cx), "Expand All");

    click("icon-button:search-collapse", cx);
    assert_eq!(rows(&search, cx).len(), 8, "every folder and file");
    assert_eq!(collapse_button(&search, cx), "Collapse All");
    click("icon-button:search-collapse", cx);
    assert_eq!(rows(&search, cx), ["src", "top.rs"]);
    assert_eq!(collapse_button(&search, cx), "Expand All");

    // An open folder of collapsed files flips it back.
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx), ["src", "a.rs", "b.rs", "top.rs"]);
    assert_eq!(collapse_button(&search, cx), "Collapse All");
    run("search.expand_all", cx);
    assert_eq!(rows(&search, cx).len(), 8);
}

#[gpui::test]
fn folder_row_shows_total_matches(cx: &mut TestAppContext) {
    let dir = project(&[
        ("branch/one/p.rs", "foo foo\n"),
        ("branch/two/q.rs", "foo\nfoo\nfoo\n"),
        ("top.rs", "foo\n"),
    ]);
    let (_workbench, search, cx) = tree(cx, dir.path());
    let shown = search.read_with(cx, |search, _| search.shown_rows());
    assert_eq!(shown[0], ShownRow::Folder("branch".into(), 5));
    assert_eq!(shown[1], ShownRow::Folder("one".into(), 2));
    assert_eq!(shown[2], ShownRow::File("p.rs".into(), 2));
    assert_eq!(shown[4], ShownRow::Folder("two".into(), 3));

    // Collapsed, it still counts them.
    click_row(&search, 0, cx);
    let shown = search.read_with(cx, |search, _| search.shown_rows());
    assert_eq!(shown[0], ShownRow::Folder("branch".into(), 5));
    assert_eq!(shown.len(), 3);
}

#[gpui::test]
fn dismiss_folder_drops_its_files(cx: &mut TestAppContext) {
    let dir = project(&[
        ("src/a.rs", "foo a\nx\nfoo b\n"),
        ("src/b.rs", "foo c\n"),
        ("src/sub/c.rs", "foo\n"),
        ("top.rs", "foo d\n"),
    ]);
    let (_workbench, search, cx) = tree(cx, dir.path());
    assert_eq!(rows(&search, cx)[..5], ["src", "sub", "c.rs", "1", "a.rs"]);
    // A nested folder's, then a top-level one's.
    click("search-dismiss:1", cx);
    assert_eq!(
        rows(&search, cx),
        ["src", "a.rs", "1", "3", "b.rs", "1", "top.rs", "1"]
    );
    click("search-dismiss:0", cx);
    assert_eq!(rows(&search, cx), ["top.rs", "1"]);

    replace_with("bar", &search, cx);
    assert_eq!(
        replace_all_question(cx).as_deref(),
        Some("Replace 1 occurrence across 1 file with “bar”?")
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    run("search.view_as_list", cx);
    assert_eq!(rows(&search, cx), ["top.rs", "1"]);
}

#[gpui::test]
fn replace_in_folder_row(cx: &mut TestAppContext) {
    let dir = project(&[
        ("other/c.rs", "foo e\n"),
        ("src/a.rs", "foo a\nx\nfoo b\n"),
        ("src/b.rs", "foo c\n"),
    ]);
    let (_workbench, search, cx) = tree(cx, dir.path());
    replace_with("bar", &search, cx);
    assert_eq!(rows(&search, cx)[3], "src");
    // Collapsed, it still replaces in every file under it.
    click_row(&search, 3, cx);
    click("search-replace-file:3", cx);
    assert_eq!(read(&dir, "src/a.rs"), "bar a\nx\nbar b\n");
    assert_eq!(read(&dir, "src/b.rs"), "bar c\n");
    assert_eq!(read(&dir, "other/c.rs"), "foo e\n");
    // The search runs again.
    assert_eq!(rows(&search, cx), ["other", "c.rs", "1"]);
}

#[gpui::test]
fn switching_back_to_list(cx: &mut TestAppContext) {
    let dir = src_project();
    let (_workbench, search, cx) = searched(cx, dir.path());
    let flat = ["src/a.rs", "1", "3", "src/b.rs", "1", "top.rs", "1"];
    assert_eq!(rows(&search, cx), flat);
    // The match of src/b.rs selected.
    run("search.focus_results", cx);
    keys("down down down", cx);
    assert_eq!(selected(&search, cx), Some(4));

    run("search.view_as_tree", cx);
    assert_eq!(selected(&search, cx), Some(5), "the same match");
    run("search.view_as_tree", cx);
    assert_eq!(rows(&search, cx).len(), 8, "already a tree");
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx), ["src", "top.rs", "1"]);
    assert_eq!(selected(&search, cx), Some(0));

    // The list shows every file, the folder's selection going to its first.
    click("icon-button:search-view-mode", cx);
    assert_eq!(rows(&search, cx), flat);
    assert_eq!(selected(&search, cx), Some(0));
    let query = search.read_with(cx, |search, cx| search.input().read(cx).text().to_owned());
    assert_eq!(query, "foo");
    run("search.focus_results", cx);
    keys("down down down down down down", cx);
    assert_eq!(selected(&search, cx), Some(6));

    // Back in the tree, the folder is still collapsed, and the selected
    // match's row is its.
    run("search.view_as_tree", cx);
    assert_eq!(rows(&search, cx), ["src", "top.rs", "1"]);
    assert_eq!(selected(&search, cx), Some(2));
    run("search.view_as_list", cx);
    assert_eq!(rows(&search, cx), flat);
    assert_eq!(selected(&search, cx), Some(6));

    // A match in a folder the tree has collapsed: the tree expands it, and
    // the list gets the same match back.
    run("search.focus_results", cx);
    keys("up up", cx);
    assert_eq!(selected(&search, cx), Some(4));
    run("search.view_as_tree", cx);
    assert_eq!(rows(&search, cx).len(), 8);
    assert_eq!(selected(&search, cx), Some(5));
    run("search.view_as_list", cx);
    assert_eq!(selected(&search, cx), Some(4));
}

#[gpui::test]
fn a_collapsed_folder_stays_collapsed_when_its_row_compacts(cx: &mut TestAppContext) {
    let dir = project(&[("a/b/x.rs", "foo\n"), ("a/c/y.rs", "foo\n")]);
    let (_workbench, search, cx) = tree(cx, dir.path());
    assert_eq!(rows(&search, cx), ["a", "b", "x.rs", "1", "c", "y.rs", "1"]);
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx), ["a"]);

    // With a/c's file dismissed, a holds only b: one row, still collapsed.
    run("search.view_as_list", cx);
    click("search-dismiss:2", cx);
    assert_eq!(rows(&search, cx), ["a/b/x.rs", "1"]);
    run("search.view_as_tree", cx);
    assert_eq!(rows(&search, cx), ["a/b"]);
    assert!(cx.debug_bounds("search-chevron:0:closed").is_some());
    assert_eq!(collapse_button(&search, cx), "Expand All");

    // Expanding it expands both.
    click_row(&search, 0, cx);
    assert_eq!(rows(&search, cx), ["a/b", "x.rs", "1"]);
    assert!(search.read_with(cx, |search, _| search.collapsed_folders.is_empty()));
}

/// Folders whose names aren't UTF-8 and read the same are told apart, as
/// are their files. Made up rather than written, as APFS takes only UTF-8
/// names.
#[gpui::test]
fn tree_tells_apart_folders_that_read_the_same(cx: &mut TestAppContext) {
    let dir = src_project();
    let (_workbench, search, cx) = tree(cx, dir.path());
    let root = search.read_with(cx, |search, _| search.root.clone().unwrap());
    let file = |name: &[u8], matches: u64| {
        let folder = OsStr::from_bytes(name);
        FileMatches {
            path: root.join(folder).join("a.rs"),
            rel: format!("{}/a.rs", folder.to_string_lossy()),
            lines: (1..=matches)
                .map(|line_number| LineMatch {
                    line_number,
                    line_offset: 0,
                    ranges: std::iter::once(0..3).collect(),
                    preview: "foo".into(),
                    preview_start: 0,
                })
                .collect(),
        }
    };
    let (ff, fe) = (file(b"d\xff", 1), file(b"d\xfe", 2));
    let kept = ff.path.clone();
    search.update(cx, |search, cx| search.rerun_with(vec![ff, fe], cx));
    draw(cx);
    assert_eq!(
        rows(&search, cx),
        ["d\u{FFFD}", "a.rs", "1", "2", "d\u{FFFD}", "a.rs", "1"]
    );
    let shown = search.read_with(cx, |search, _| search.shown_rows());
    assert_eq!(shown[0], ShownRow::Folder("d\u{FFFD}".into(), 2));
    assert_eq!(shown[4], ShownRow::Folder("d\u{FFFD}".into(), 1));

    // Dismissing one drops only its file, and its Replace would replace
    // only in it.
    search.read_with(cx, |search, _| {
        let targets = search.targets(search.files_under(1));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, kept);
    });
    click("search-dismiss:0", cx);
    assert_eq!(rows(&search, cx), ["d\u{FFFD}", "a.rs", "1"]);
    let left: Vec<_> = search.read_with(cx, |search, _| {
        search.files.iter().map(|file| file.path.clone()).collect()
    });
    assert_eq!(left, [kept]);
}
