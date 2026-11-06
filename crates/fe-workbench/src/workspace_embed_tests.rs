//! The workbench's panes are its workspace's: a split, a tab moved by key,
//! a pane closing once emptied and a pane split off since launch all go
//! through kiln's [`Workspace`], which the workbench follows.

use std::{fs, path::PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_shell::Workspace;

use crate::{
    Launch, Pane, Workbench, pane_group::PaneGroup, pane_toolbar::ToolbarRequest, tests::open,
};

/// A workbench rooted at a folder of `a.txt` and `b.rs`, with `a.txt` open.
fn workbench(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for (name, text) in [("a.txt", "alpha\n"), ("b.rs", "fn b() {}\n")] {
        fs::write(root.join(name), text).unwrap();
    }
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    open_file(&workbench, root.join("a.txt"), cx);
    (workbench, cx, dir)
}

/// Opens `path` in the active pane, and waits for it to show.
fn open_file(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    pane.update_in(cx, |pane, window, cx| pane.open(path, window, cx));
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn workspace(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Workspace<Pane>> {
    workbench.read_with(cx, |workbench, _| workbench.workspace().clone())
}

/// The workspace's panes, in layout order.
fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    let workspace = workspace(workbench, cx);
    workspace.read_with(cx, |workspace, _| {
        workspace.group().panes().into_iter().cloned().collect()
    })
}

/// The workspace's active pane, which must be the workbench's.
fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    let workspace = workspace(workbench, cx);
    let active = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());
    let ours = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(
        ours, active,
        "the workbench follows the workspace's active pane"
    );
    active
}

/// The pane's tabs as file names.
fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .map(|tab| crate::pane::file_name(tab.path.as_deref()))
            .collect()
    })
}

#[gpui::test]
fn the_pane_group_is_the_workspaces(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = workbench(cx);
    let workspace = workspace(&workbench, cx);
    workbench.read_with(cx, |workbench, cx| {
        let ours: &PaneGroup = workbench.pane_group(cx);
        assert!(std::ptr::eq(ours, workspace.read(cx).group()));
    });
    let shell = workspace.read_with(cx, |workspace, _| workspace.shell().clone());
    workbench.read_with(cx, |workbench, _| assert_eq!(*workbench.shell(), shell));
}

#[gpui::test]
fn a_split_goes_through_the_workspace(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = workbench(cx);
    let first = active(&workbench, cx);

    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let panes = panes(&workbench, cx);
    assert_eq!(panes.len(), 2);
    assert_eq!(panes[0], first);
    let split = active(&workbench, cx);
    assert_eq!(split, panes[1]);
    assert_eq!(tabs(&split, cx), ["a.txt"]);
}

#[gpui::test]
fn a_tab_moved_by_key_to_an_edge_splits_through_the_workspace(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = workbench(cx);
    let root = dunce::canonicalize(dir.path()).unwrap();
    open_file(&workbench, root.join("b.rs"), cx);
    let first = active(&workbench, cx);
    assert_eq!(tabs(&first, cx), ["a.txt", "b.rs"]);
    let moves = workbench.read_with(cx, |workbench, _| workbench.moves);

    // No pane to the right: the tab moves to one split off there.
    cx.simulate_keystrokes("ctrl-cmd-right");
    draw(cx);
    let panes = panes(&workbench, cx);
    assert_eq!(panes.len(), 2);
    assert_eq!(panes[0], first);
    assert_eq!(tabs(&first, cx), ["a.txt"]);
    assert_eq!(tabs(&panes[1], cx), ["b.rs"]);
    assert_eq!(active(&workbench, cx), panes[1]);
    assert_eq!(workbench.read_with(cx, |w, _| w.moves), moves + 1);
}

#[gpui::test]
fn a_pane_emptied_by_closing_its_last_tab_closes(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = workbench(cx);
    let first = active(&workbench, cx);
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    assert_eq!(panes(&workbench, cx).len(), 2);

    cx.simulate_keystrokes("cmd-w");
    draw(cx);
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&first));
    assert_eq!(active(&workbench, cx), first);
    assert_eq!(tabs(&first, cx), ["a.txt"]);
}

#[gpui::test]
fn a_pane_split_off_after_launch_is_heard(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = workbench(cx);
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let split = active(&workbench, cx);
    let shown = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, _| workbench.toolbar_picker().is_some())
    };
    assert!(!shown(cx));

    // Its toolbar shows as the first pane's does, and its requests reach
    // the workbench.
    assert!(split.read_with(cx, |pane, _| pane.toolbar().visible));
    split.update(cx, |_, cx| cx.emit(ToolbarRequest::PaneCommands));
    draw(cx);
    assert!(shown(cx));
}
