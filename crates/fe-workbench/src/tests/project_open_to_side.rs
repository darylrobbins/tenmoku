//! Open to the Side from the project tree: a file row's menu and key open
//! the file in the pane right of the active one, split off if there is none.

use std::fs;

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext};

use super::{open_root, panel, rows};
use crate::{Direction, Pane, ProjectPanel, Workbench};

fn workbench(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<ProjectPanel>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt"] {
        fs::write(dir.path().join(name), "text\n").unwrap();
    }
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, _| {
        window.activate_window();
    });
    cx.run_until_parked();
    let panel = panel(&workbench, cx);
    assert_eq!(rows(&panel, cx).len(), 2);
    (dir, workbench, panel, cx)
}

/// Each pane's tabs, by file name.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter())
            .map(|pane| {
                (pane.read(cx).tabs().iter())
                    .map(|tab| fe_gpui::pane_item::file_name(tab.path.as_deref()))
                    .collect()
            })
            .collect()
    })
}

fn active_pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn right_click(panel: &Entity<ProjectPanel>, name: &str, cx: &mut VisualTestContext) {
    let ix = (rows(panel, cx).iter())
        .position(|(_, row)| row == name)
        .unwrap();
    let at = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn").center());
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
}

fn choose(title: &str, cx: &mut VisualTestContext) {
    let selector: &'static str = Box::leak(format!("menu-item:{title}").into_boxed_str());
    let bounds = (cx.debug_bounds(selector)).unwrap_or_else(|| panic!("{title} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn file_row_menu_opens_the_file_to_the_side(cx: &mut TestAppContext) {
    let (_dir, workbench, panel, cx) = workbench(cx);
    right_click(&panel, "a.txt", cx);
    let menu = panel
        .read_with(cx, |panel, _| panel.menu_commands())
        .unwrap();
    assert_eq!(
        menu[..3],
        [
            Some("project_panel.open"),
            Some("project_panel.open_to_side"),
            None
        ]
    );
    choose("Open", cx);
    assert_eq!(tabs(&workbench, cx), [["a.txt"]]);
    let left = active_pane(&workbench, cx);

    right_click(&panel, "b.txt", cx);
    choose("Open to the Side", cx);
    assert_eq!(tabs(&workbench, cx), [vec!["a.txt"], vec!["b.txt"]]);
    let right = active_pane(&workbench, cx);
    assert_ne!(right, left, "the new pane is focused");
    let neighbour = workbench.read_with(cx, |workbench, cx| {
        workbench
            .pane_group(cx)
            .neighbour(&left, Direction::Right, cx)
    });
    assert_eq!(neighbour, Some(right), "split to the right");
}

#[gpui::test]
fn to_the_side_reuses_the_right_pane(cx: &mut TestAppContext) {
    let (_dir, workbench, panel, cx) = workbench(cx);
    right_click(&panel, "a.txt", cx);
    choose("Open", cx);
    right_click(&panel, "a.txt", cx);
    choose("Open to the Side", cx);
    assert_eq!(tabs(&workbench, cx).len(), 2);
    let right = active_pane(&workbench, cx);

    // Back to the left pane, another file to the side: a tab on the right.
    cx.update(|window, cx| {
        let left = workbench.read(cx).panes(cx)[0].clone();
        workbench.update(cx, |workbench, cx| workbench.activate(left, window, cx));
    });
    right_click(&panel, "b.txt", cx);
    choose("Open to the Side", cx);
    assert_eq!(tabs(&workbench, cx).len(), 2, "no third pane");
    assert_eq!(active_pane(&workbench, cx), right);
    assert_eq!(tabs(&workbench, cx)[1], ["a.txt", "b.txt"]);
}

#[gpui::test]
fn folder_and_empty_menus_do_not_list_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/m.txt"), "").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let _ = workbench;
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let panel = panel(&workbench, cx);
    right_click(&panel, "src", cx);
    let menu = panel
        .read_with(cx, |panel, _| panel.menu_commands())
        .unwrap();
    assert!(!menu.contains(&Some("project_panel.open_to_side")));
}
