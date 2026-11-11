// SPDX-License-Identifier: Apache-2.0

//! A Workbench window's editor tabs and project tree as a screen reader is
//! given them (A11Y-4): the tab strip is a `TabList` of `Tab`s, the active
//! one selected and a dirty one "modified" in its label; the project tree
//! is the `Tree` "Files", its rows `TreeItem`s with a level, a folder's
//! expanded state and their place among their siblings. Checked after
//! switching tabs, editing a tab dirty and expanding a folder.

use std::path::PathBuf;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_ui::{Node, Role, States, nodes};
use fe_workbench::{Launch, Workbench};
use gpui::{ElementId, Entity, Focusable as _, TestAppContext, VisualTestContext};

/// `src/` holding `one.txt` and `two.txt`, then `a.txt` and `b.txt`, with
/// `a.txt` open. Plain text, so no language server starts.
fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    // See palette_ghost_text.rs: no config directory, no watcher threads.
    for var in ["FASTEDIT_CONFIG_DIR", "XDG_CONFIG_HOME", "HOME"] {
        // SAFETY: no other thread is running yet to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    for (path, text) in [
        ("src/one.txt", "one\n"),
        ("src/two.txt", "two\n"),
        ("a.txt", "alpha\n"),
        ("b.txt", "beta\n"),
    ] {
        std::fs::write(root.join(path), text).unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let a = root.join("a.txt");
    let launch = Launch {
        root: Some(root.to_path_buf()),
        file: Some((a, Some(Buffer::new("alpha\n").into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (dir, workbench, cx)
}

/// What the recorder saw in a frame that drew every view afresh.
fn drawn(cx: &mut VisualTestContext) -> Vec<Node> {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        nodes(window, cx)
    })
}

/// Whether `node` is one of `name`'s numbered elements, as `("tab", 2)`.
fn numbered(node: &Node, name: &str) -> bool {
    matches!(&node.id, ElementId::NamedInteger(n, _) if n.as_ref() == name)
}

/// A tab's label, whether it's selected, and its place in the strip.
type TabSeen = (String, Option<bool>, Option<(usize, usize)>);

/// The editor pane's tabs, as [`TabSeen`].
fn tabs(nodes: &[Node]) -> Vec<TabSeen> {
    nodes
        .iter()
        .filter(|n| n.role == Role::Tab && numbered(n, "tab"))
        .map(|n| {
            let place = n.states.position_in_set.zip(n.states.size_of_set);
            (n.label.to_string(), n.states.selected, place)
        })
        .collect()
}

/// The project tree's rows: each one's label and states.
fn rows(nodes: &[Node]) -> Vec<(String, States)> {
    nodes
        .iter()
        .filter(|n| n.role == Role::TreeItem && numbered(n, "row"))
        .map(|n| (n.label.to_string(), n.states.clone()))
        .collect()
}

fn row(level: usize, expanded: Option<bool>, selected: bool, place: (usize, usize)) -> States {
    States {
        selected: Some(selected),
        expanded,
        level: Some(level),
        position_in_set: Some(place.0),
        size_of_set: Some(place.1),
        ..States::default()
    }
}

fn open(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    cx.run_until_parked();
}

/// Runs the project tree's command `id` with the tree focused, as its key
/// would.
fn run_in_tree(workbench: &Entity<Workbench>, id: &str, cx: &mut VisualTestContext) {
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("project_panel"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
}

#[gpui::test]
fn tabs_are_a_tab_list_with_the_active_one_selected_and_dirty_ones_modified(
    cx: &mut TestAppContext,
) {
    let (dir, workbench, cx) = opened(cx);
    let got = drawn(cx);
    assert!(
        got.iter()
            .any(|n| n.role == Role::TabList && n.label.as_ref() == "Tabs"),
        "{got:#?}"
    );
    assert_eq!(tabs(&got), [("a.txt".into(), Some(true), Some((1, 1)))]);

    // A second file opens in a tab of its own, which is then the selected
    // one.
    open(&workbench, dir.path().join("b.txt"), cx);
    assert_eq!(
        tabs(&drawn(cx)),
        [
            ("a.txt".into(), Some(false), Some((1, 2))),
            ("b.txt".into(), Some(true), Some((2, 2))),
        ]
    );

    // Switching back selects the first.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    assert_eq!(
        tabs(&drawn(cx)),
        [
            ("a.txt".into(), Some(true), Some((1, 2))),
            ("b.txt".into(), Some(false), Some((2, 2))),
        ]
    );

    // An edit makes the tab dirty: "modified" in its label, and in its
    // description.
    let editor = workbench.read_with(cx, |workbench, cx| {
        workbench
            .active_editor(cx)
            .cloned()
            .expect("a.txt's editor")
    });
    editor.update(cx, |editor, cx| {
        editor.edit_and_reveal(|_| vec![(0..0, "x".into())], cx);
    });
    let got = drawn(cx);
    assert_eq!(
        tabs(&got),
        [
            ("a.txt, modified".into(), Some(true), Some((1, 2))),
            ("b.txt".into(), Some(false), Some((2, 2))),
        ]
    );
    let a = got
        .iter()
        .find(|n| n.role == Role::Tab && n.label.starts_with("a.txt"))
        .unwrap();
    assert_eq!(a.states.description.as_deref(), Some("modified"));
    // Its close button keeps the file's own name.
    let closes: Vec<_> = got
        .iter()
        .filter(|n| n.role == Role::Button && n.label.starts_with("Close "))
        .map(|n| n.label.to_string())
        .collect();
    assert!(closes.contains(&"Close a.txt".to_string()), "{closes:?}");
    assert!(closes.contains(&"Close b.txt".to_string()), "{closes:?}");
}

#[gpui::test]
fn project_rows_are_an_outline_with_level_and_expansion(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx);
    let got = drawn(cx);
    let tree: Vec<_> = got.iter().filter(|n| n.role == Role::Tree).collect();
    assert_eq!(tree.len(), 1, "{got:#?}");
    assert_eq!(tree[0].label.as_ref(), "Files");

    // Folders first, each row at the top level and placed among the
    // root's three children; `src` closed. (Which row starts selected is
    // the reveal's business, so it's left out here.)
    let got = rows(&drawn(cx));
    let names: Vec<_> = got.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["src", "a.txt", "b.txt"]);
    let unselected = |(name, states): &(String, States)| {
        (
            name.clone(),
            States {
                selected: Some(false),
                ..states.clone()
            },
        )
    };
    assert_eq!(
        got.iter().map(unselected).collect::<Vec<_>>(),
        [
            ("src".into(), row(1, Some(false), false, (1, 3))),
            ("a.txt".into(), row(1, None, false, (2, 3))),
            ("b.txt".into(), row(1, None, false, (3, 3))),
        ]
    );

    // Expanding `src` shows its files a level down, each placed among
    // its own siblings, and `src` as expanded.
    run_in_tree(&workbench, "project_panel.select_first", cx);
    run_in_tree(&workbench, "project_panel.toggle", cx);
    assert_eq!(
        rows(&drawn(cx)),
        [
            ("src".into(), row(1, Some(true), true, (1, 3))),
            ("one.txt".into(), row(2, None, false, (1, 2))),
            ("two.txt".into(), row(2, None, false, (2, 2))),
            ("a.txt".into(), row(1, None, false, (2, 3))),
            ("b.txt".into(), row(1, None, false, (3, 3))),
        ]
    );

    // Moving down selects the next row, which is then the selected item.
    run_in_tree(&workbench, "project_panel.select_next", cx);
    let selected: Vec<_> = (rows(&drawn(cx)).into_iter())
        .filter(|(_, states)| states.selected == Some(true))
        .map(|(name, _)| name)
        .collect();
    assert_eq!(selected, ["one.txt"]);

    // Collapsing it again.
    run_in_tree(&workbench, "project_panel.select_first", cx);
    run_in_tree(&workbench, "project_panel.toggle", cx);
    assert_eq!(
        rows(&drawn(cx))[0],
        ("src".into(), row(1, Some(false), true, (1, 3)))
    );
    drop(dir);
}
