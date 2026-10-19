//! Activations made through the workspace rather than the workbench.

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::*;
use crate::Pane;

#[gpui::test]
fn a_workspace_round_trip_within_one_update_drops_pending_requests(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    keys("A ! escape", cx);
    dispatch("pane.split_right", cx);
    let panes = workbench.read_with(cx, |workbench, cx| workbench.panes(cx));
    let [first, second] = panes.try_into().expect("two panes");
    let tabs = |pane: &Entity<Pane>, cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, _| pane.tabs().len())
    };
    put_caret(&tab_editor(&workbench, cx), 0, cx);
    ask_open(&workbench, cx);

    // The workspace activates the other pane and back, within one update.
    let workspace = workbench.read_with(cx, |workbench, _| workbench.workspace.clone());
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.activate(first.clone(), window, cx);
            workspace.activate(second.clone(), window, cx);
        })
    });
    draw(cx);
    assert_eq!([tabs(&first, cx), tabs(&second, cx)], [1, 1]);

    // Asked again, it opens.
    dispatch("git.open_line_commit", cx);
    assert_eq!(tabs(&second, cx), 2);
}
