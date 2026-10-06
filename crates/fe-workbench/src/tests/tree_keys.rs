//! The project panel's keys, its selected row, and revealing the active
//! file.

use std::{fs, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_project::{FsChange, fixture};
use gpui::{Entity, Focusable as _, Keystroke, TestAppContext, VisualTestContext};

use super::{open_root, panel, rows};
use crate::{ProjectPanel, Workbench};

/// Focuses the workbench, as the app does, then the panel through
/// `dock.show_project`.
fn focus_panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ProjectPanel> {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-e");
    let panel = panel(workbench, cx);
    assert!(
        panel_focused(&panel, cx),
        "dock.show_project focuses the panel"
    );
    panel
}

fn panel_focused(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| panel.focus_handle(cx).is_focused(window))
}

/// The selected row, and the row the last frame drew selected.
fn selected(
    panel: &Entity<ProjectPanel>,
    cx: &mut VisualTestContext,
) -> (Option<usize>, Option<usize>) {
    panel.read_with(cx, |panel, _| {
        (panel.selected_row(), panel.drawn_selected())
    })
}

/// The selected row's name.
fn selected_name(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<String> {
    let ix = selected(panel, cx).0?;
    Some(rows(panel, cx)[ix].1.clone())
}

fn listings(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> u64 {
    panel.read_with(cx, |panel, _| panel.tree().unwrap().stats().listings)
}

/// Presses `keys` without running the executor.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.dispatch_keystroke(Keystroke::parse(keys).unwrap(), cx);
    });
}

/// Whether the selected row was laid out inside the list in the last
/// frame.
fn selection_in_view(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> bool {
    panel.read_with(cx, |panel, _| {
        let Some(ix) = panel.selected_row() else {
            return false;
        };
        let row = panel.row_bounds(ix).expect("drawn");
        let list = panel.list_bounds();
        panel.drawn_selected() == Some(ix)
            && row.top() >= list.top()
            && row.bottom() <= list.bottom()
    })
}

/// Files at `paths` (relative), empty, in a temporary directory.
fn project(paths: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for rel in paths {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    dir
}

fn open_file(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    cx.run_until_parked();
}

#[gpui::test]
fn arrows_move_the_selection(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // Two directories and .gitignore.
    fixture::generate(dir.path(), 2, 3).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = focus_panel(&workbench, cx);
    assert_eq!(rows(&panel, cx).len(), 3);
    assert_eq!(selected(&panel, cx), (None, None));

    cx.simulate_keystrokes("down");
    assert_eq!(selected(&panel, cx), (Some(0), Some(0)), "the first row");
    cx.simulate_keystrokes("down");
    assert_eq!(selected(&panel, cx), (Some(1), Some(1)));
    cx.simulate_keystrokes("up");
    assert_eq!(selected(&panel, cx), (Some(0), Some(0)));
    cx.simulate_keystrokes("up");
    assert_eq!(
        selected(&panel, cx),
        (Some(0), Some(0)),
        "clamped at the top"
    );
    cx.simulate_keystrokes("down down down down");
    assert_eq!(
        selected(&panel, cx),
        (Some(2), Some(2)),
        "clamped at the bottom"
    );

    // With no selection, up selects the first row too.
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = focus_panel(&workbench, cx);
    cx.simulate_keystrokes("up");
    assert_eq!(selected(&panel, cx), (Some(0), Some(0)));
}

#[gpui::test]
fn left_and_right_follow_the_tree(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 2, 3).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = focus_panel(&workbench, cx);
    let collapsed = rows(&panel, cx);
    cx.simulate_keystrokes("down");
    assert_eq!(selected_name(&panel, cx), Some(fixture::dir_name(0)));

    // Listed in the background: nothing happens until the executor runs.
    let before = listings(&panel, cx);
    press("right", cx);
    assert_eq!(rows(&panel, cx), collapsed);
    assert_eq!(listings(&panel, cx), before);
    cx.run_until_parked();
    assert_eq!(listings(&panel, cx), before + 1, "one listing");
    let expanded = rows(&panel, cx);
    assert_eq!(expanded[1], (1, fixture::file_name(0)));
    assert_eq!(expanded.len(), collapsed.len() + 3);
    assert_eq!(
        selected(&panel, cx).0,
        Some(0),
        "the directory stays selected"
    );

    cx.simulate_keystrokes("right");
    assert_eq!(selected(&panel, cx), (Some(1), Some(1)), "its first child");
    cx.simulate_keystrokes("left");
    assert_eq!(selected(&panel, cx), (Some(0), Some(0)), "the parent");
    cx.simulate_keystrokes("left");
    assert_eq!(rows(&panel, cx), collapsed);
    assert_eq!(listings(&panel, cx), before + 1, "collapsing lists nothing");
    assert_eq!(selected(&panel, cx).0, Some(0));

    // At the top level, left does nothing.
    cx.simulate_keystrokes("left");
    assert_eq!(rows(&panel, cx), collapsed);
    assert_eq!(selected(&panel, cx).0, Some(0));

    // Nor does right on a file.
    cx.simulate_keystrokes("down down");
    assert_eq!(selected_name(&panel, cx).as_deref(), Some(".gitignore"));
    cx.simulate_keystrokes("right");
    assert_eq!(rows(&panel, cx), collapsed);
    assert_eq!(selected_name(&panel, cx).as_deref(), Some(".gitignore"));
    assert_eq!(listings(&panel, cx), before + 1);
}

#[gpui::test]
fn enter_opens_or_toggles(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 2, 3).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = focus_panel(&workbench, cx);
    let collapsed = rows(&panel, cx);

    cx.simulate_keystrokes("down enter");
    assert_eq!(rows(&panel, cx).len(), collapsed.len() + 3, "expanded");
    cx.simulate_keystrokes("enter");
    assert_eq!(rows(&panel, cx), collapsed, "collapsed");
    assert!(panel_focused(&panel, cx));

    cx.simulate_keystrokes("enter down down enter");
    assert_eq!(selected_name(&panel, cx), Some(fixture::file_name(1)));
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file opened");
    let path = editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned));
    let expected = dir
        .path()
        .join(fixture::dir_name(0))
        .join(fixture::file_name(1));
    assert_eq!(path, Some(expected));
    let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    assert!(focused, "its editor has focus");
}

#[gpui::test]
fn the_selection_stays_in_view(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 1, 1000).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = focus_panel(&workbench, cx);
    cx.simulate_keystrokes("down right");
    assert_eq!(
        rows(&panel, cx).len(),
        1002,
        "the directory, its files, .gitignore"
    );
    let fit = panel.read_with(cx, |panel, _| {
        let row = panel.row_bounds(0).unwrap();
        (panel.list_bounds().size.height / row.size.height) as usize
    });
    assert!(fit > 5 && fit < 500, "{fit} rows fit");

    for _ in 0..fit + 10 {
        cx.simulate_keystrokes("down");
        assert!(selection_in_view(&panel, cx), "{:?}", selected(&panel, cx));
    }
    assert_eq!(selected(&panel, cx).0, Some(fit + 10));
    let offset = panel.read_with(cx, |panel, _| {
        panel.list_bounds().top() - panel.row_bounds(0).unwrap().top()
    });
    assert!(offset > gpui::px(0.), "the list scrolled");
}

#[gpui::test]
fn reveal_expands_only_unloaded_ancestors(cx: &mut TestAppContext) {
    let mut paths: Vec<String> = (0..1000)
        .map(|ix| format!("a/b/c/{}", fixture::file_name(ix)))
        .collect();
    paths.extend(
        [
            "top.txt",
            "d/e/x.log",
            "d/e/y.txt",
            "g/h/i/z.txt",
            "j/k/m.txt",
            "target/out.txt",
            ".gitignore",
        ]
        .map(String::from),
    );
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    let dir = project(&paths);
    fs::write(dir.path().join(".gitignore"), "*.log\ntarget/\n").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let panel = panel(&workbench, cx);
    let collapsed = rows(&panel, cx);
    assert_eq!(
        collapsed.len(),
        6,
        "a, d, g, j, .gitignore, top.txt: {collapsed:?}"
    );

    let before = listings(&panel, cx);
    let deep = dir.path().join("a/b/c").join(fixture::file_name(990));
    open_file(&workbench, &deep, cx);
    assert_eq!(
        listings(&panel, cx),
        before + 3,
        "a, a/b and a/b/c, once each"
    );
    assert_eq!(rows(&panel, cx).len(), collapsed.len() + 1002);
    assert_eq!(selected_name(&panel, cx), Some(fixture::file_name(990)));
    assert!(selection_in_view(&panel, cx));
    assert!(!panel_focused(&panel, cx), "focus stays on the editor");

    // With the panel focused: the listings run off the main thread, and
    // focus stays on the panel.
    cx.simulate_keystrokes("cmd-shift-e");
    assert!(panel_focused(&panel, cx));
    let before = listings(&panel, cx);
    let shown = rows(&panel, cx);
    let z = dir.path().join("g/h/i/z.txt");
    panel.update(cx, |panel, cx| panel.reveal(z.clone(), cx));
    assert_eq!(rows(&panel, cx), shown, "nothing expanded yet");
    assert_eq!(listings(&panel, cx), before, "nothing listed yet");
    cx.run_until_parked();
    assert_eq!(listings(&panel, cx), before + 3, "g, g/h and g/h/i");
    assert_eq!(selected_name(&panel, cx).as_deref(), Some("z.txt"));
    assert!(selection_in_view(&panel, cx));
    assert!(panel_focused(&panel, cx));

    // The command reveals the active tab's file: loaded, so with no listing.
    let reveal = RunCommand(
        Invocation::of("project_panel.reveal_active")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(reveal), cx));
    cx.run_until_parked();
    assert_eq!(selected_name(&panel, cx), Some(fixture::file_name(990)));
    assert_eq!(listings(&panel, cx), before + 3);
    assert!(panel_focused(&panel, cx));

    // A newer reveal cancels a pending one: j stays collapsed.
    panel.update(cx, |panel, cx| {
        panel.reveal(dir.path().join("j/k/m.txt"), cx);
        panel.reveal(z.clone(), cx);
    });
    cx.run_until_parked();
    assert_eq!(selected_name(&panel, cx).as_deref(), Some("z.txt"));
    let shown = rows(&panel, cx);
    assert!(!shown.contains(&(1, "k".to_string())), "{shown:?}");

    // Ignored, the file has no row: nothing expands or is selected.
    let selection = selected(&panel, cx).0;
    open_file(&workbench, &dir.path().join("d/e/x.log"), cx);
    assert_eq!(rows(&panel, cx), shown);
    assert_eq!(selected(&panel, cx).0, selection);
    let listed = listings(&panel, cx);
    open_file(&workbench, &dir.path().join("target/out.txt"), cx);
    assert_eq!(rows(&panel, cx), shown);
    assert_eq!(listings(&panel, cx), listed, "target/ isn't in the tree");

    // Outside the root, neither.
    let outside = project(&["elsewhere.txt"]);
    open_file(&workbench, &outside.path().join("elsewhere.txt"), cx);
    assert_eq!(rows(&panel, cx), shown);
    assert_eq!(listings(&panel, cx), listed);
    assert_eq!(selected(&panel, cx).0, selection);

    // Back to the first tab, whose file is loaded: selected again, with no
    // listing.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    cx.run_until_parked();
    assert_eq!(selected_name(&panel, cx), Some(fixture::file_name(990)));
    assert_eq!(listings(&panel, cx), listed);
}

#[gpui::test]
fn removing_the_selected_row_moves_the_selection(cx: &mut TestAppContext) {
    let dir = project(&["a.txt", "b.txt", "c.txt"]);
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = focus_panel(&workbench, cx);
    let remove = |name: &str, cx: &mut VisualTestContext| {
        let path = dir.path().join(name);
        fs::remove_file(&path).unwrap();
        panel.update(cx, |panel, cx| {
            panel.apply_fs_changes(&[FsChange::Path(path)], cx)
        });
        cx.run_until_parked();
    };

    cx.simulate_keystrokes("down down");
    assert_eq!(selected_name(&panel, cx).as_deref(), Some("b.txt"));
    remove("b.txt", cx);
    assert_eq!(selected(&panel, cx), (Some(1), Some(1)));
    assert_eq!(
        selected_name(&panel, cx).as_deref(),
        Some("c.txt"),
        "the row now at its index"
    );

    remove("c.txt", cx);
    assert_eq!(
        selected(&panel, cx),
        (Some(0), Some(0)),
        "clamped to the last row"
    );
    assert_eq!(selected_name(&panel, cx).as_deref(), Some("a.txt"));

    remove("a.txt", cx);
    assert_eq!(selected(&panel, cx), (None, None), "no rows left");
}

#[test]
fn the_default_keymap_binds_the_panel_commands() {
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
        "project_panel.select_prev",
        "project_panel.select_next",
        "project_panel.collapse_or_parent",
        "project_panel.expand_or_first_child",
        "project_panel.open",
    ] {
        assert_eq!(bound(command), 1, "{command}");
    }
    assert_eq!(bound("project_panel.reveal_active"), 0, "palette only");
    let registry = crate::project_panel::registry();
    assert!(
        registry
            .get(&fe_command::CommandId::parse("project_panel.reveal_active").unwrap())
            .is_some()
    );
}
