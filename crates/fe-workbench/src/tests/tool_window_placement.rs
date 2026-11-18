//! Tool windows moved between the bottom panel and the sidebar: the Git
//! log's tab dragged onto the sidebar shows there, with a button in the
//! rail's top group; Move Tool Window to Sidebar and to Bottom Panel move
//! the tool window in play either way, its foot button following; the
//! places come back with the window's layout; and focus, the docks' keys
//! and vim's window keys treat each by the dock now holding it.

use std::fs;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::commands::RunCommand;
use gpui::{
    Entity, FocusHandle, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};
use kiln_shell::{DockView as _, PanelId};

use super::open;
use crate::{Dock, DockPanel, Launch, Workbench};

const GIT_LOG: PanelId = PanelId("git_log");
const TERMINAL: PanelId = PanelId("terminal");
const PROBLEMS: PanelId = PanelId("problems");
const RUNS: PanelId = PanelId(crate::runs_panel::PANEL);
const DEBUG: PanelId = PanelId(crate::debug_panel::PANEL);

fn search() -> PanelId {
    DockPanel::Search.id()
}

/// A workbench on a temporary project showing `a.rs`, focused, in an
/// active window. The terminal starts no shell.
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
            break;
        }
    }
    cx.run_until_parked();
}

fn sidebar(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Dock> {
    workbench.read_with(cx, |workbench, _| workbench.dock().clone())
}

fn bottom(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<kiln_shell::Dock> {
    workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone())
}

/// Reads the bottom dock's state.
fn read<R>(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
    read: impl FnOnce(&kiln_shell::DockState) -> R,
) -> R {
    let bottom = bottom(workbench, cx);
    bottom.read_with(cx, |dock, _| read(dock.dock()))
}

/// Reads the sidebar's state.
fn read_sidebar<R>(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
    read: impl FnOnce(&kiln_shell::DockState) -> R,
) -> R {
    let sidebar = sidebar(workbench, cx);
    sidebar.read_with(cx, |dock, _| read(dock.state()))
}

/// The tool windows the bottom dock's drawn groups show, in order.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<PanelId> {
    read(workbench, cx, |dock| dock.shown_panels())
}

/// The rail's buttons lit, as last drawn.
fn lit(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<PanelId> {
    read_sidebar(workbench, cx, |dock| dock.drawn().rail_highlighted())
}

/// The rail's buttons, top to bottom, as last drawn.
fn rail(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<PanelId> {
    read_sidebar(workbench, cx, |dock| dock.drawn().rail_ids())
}

/// The rail's foot: its actions.
fn foot(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    read_sidebar(workbench, cx, |dock| dock.action_ids())
}

/// The sidebar's panel shown, while it shows.
fn sidebar_shows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PanelId> {
    read_sidebar(workbench, cx, |dock| dock.rail_highlighted())
}

/// Clicks the rail's button `id`.
fn click_rail(workbench: &Entity<Workbench>, id: PanelId, cx: &mut VisualTestContext) {
    let button = read_sidebar(workbench, cx, |dock| dock.drawn().rail_button_bounds(id));
    cx.simulate_click(button.expect("drawn").center(), Modifiers::none());
    draw(cx);
}

/// Runs `id` as a key would in the workbench.
fn run(id: &str, cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn focused_in(focus: &FocusHandle, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| focus.contains_focused(window, cx))
}

fn editor_focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx))
}

fn git_log_focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    let sidebar = sidebar(workbench, cx);
    sidebar.read_with(cx, |dock, cx| dock.git_log().unwrap().focus_handle(cx))
}

fn problems_focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    let sidebar = sidebar(workbench, cx);
    sidebar.read_with(cx, |dock, cx| dock.problems().focus_handle(cx))
}

fn search_focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    let sidebar = sidebar(workbench, cx);
    sidebar.read_with(cx, |dock, cx| dock.search().focus_handle(cx))
}

/// Drags from `from` to `to` and drops it there.
fn drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
    let none = Modifiers::none();
    cx.simulate_mouse_down(from, MouseButton::Left, none);
    cx.simulate_mouse_move(
        point(from.x + px(3.), from.y + px(3.)),
        MouseButton::Left,
        none,
    );
    draw(cx);
    cx.simulate_mouse_move(to, MouseButton::Left, none);
    draw(cx);
    cx.simulate_mouse_up(to, MouseButton::Left, none);
    draw(cx);
}

/// Problems moved to the sidebar and Search to the bottom panel, by the
/// commands, Search shown and focused there.
fn problems_and_search_moved(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    run("problems.show", cx);
    run("dock.move_tool_window_to_sidebar", cx);
    run("search.show", cx);
    run("dock.move_tool_window_to_bottom_panel", cx);
    assert_eq!(shown(workbench, cx), [search()]);
    assert!(read_sidebar(workbench, cx, |dock| dock.panel_ids()).contains(&PROBLEMS));
    assert!(focused_in(&search_focus(workbench, cx), cx));
}

#[gpui::test]
fn the_git_log_dragged_onto_the_sidebar_becomes_a_sidebar_panel(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    run("terminal.toggle", cx);
    run("git.log", cx);
    assert_eq!(shown(&workbench, cx), [GIT_LOG]);
    let before = read(&workbench, cx, |dock| dock.panel_ids());
    assert!(read_sidebar(&workbench, cx, |dock| dock.is_visible()));
    let top = rail(&workbench, cx);
    assert!(foot(&workbench, cx).contains(&GIT_LOG.0));

    // Its tab dropped on the sidebar's column.
    let from = read(&workbench, cx, |dock| dock.drawn().tab_bounds(GIT_LOG));
    let column = read_sidebar(&workbench, cx, |dock| dock.drawn().column_bounds());
    drag(
        from.expect("a tab").center(),
        column.expect("shown").center(),
        cx,
    );

    // It shows in the sidebar under its heading, focused.
    assert_eq!(sidebar_shows(&workbench, cx), Some(GIT_LOG));
    let title = cx.update(|_, cx| {
        let sidebar = workbench.read(cx).dock().read(cx);
        (
            sidebar.title(cx),
            sidebar.git_log().unwrap().read(cx).title(cx),
        )
    });
    assert_eq!(title.0, title.1);
    assert!(focused_in(&git_log_focus(&workbench, cx), cx));
    // A button in the rail's top group, lit, and none at the foot.
    let mut expected = top.clone();
    let at = expected
        .iter()
        .position(|id| id.0 == foot(&workbench, cx)[0]);
    expected.retain(|id| *id != GIT_LOG);
    expected.insert(at.unwrap_or(expected.len()), GIT_LOG);
    assert_eq!(rail(&workbench, cx), expected);
    assert!(!foot(&workbench, cx).contains(&GIT_LOG.0));
    assert_eq!(foot(&workbench, cx), [PROBLEMS.0, RUNS.0, TERMINAL.0]);
    assert!(lit(&workbench, cx).contains(&GIT_LOG));
    let tooltip_name = cx.update(|_, cx| {
        let sidebar = workbench.read(cx).dock().read(cx);
        sidebar.state().panel_title(GIT_LOG, cx)
    });
    assert_eq!(tooltip_name, Some(title.1.clone()));

    // The bottom panel's other tools are unchanged, the terminal shown.
    let others: Vec<_> = before.into_iter().filter(|id| *id != GIT_LOG).collect();
    assert_eq!(read(&workbench, cx, |dock| dock.panel_ids()), others);
    assert_eq!(shown(&workbench, cx), [TERMINAL]);
    assert!(lit(&workbench, cx).contains(&TERMINAL));

    // git.log shows it in the sidebar.
    run("dock.toggle", cx);
    assert_eq!(sidebar_shows(&workbench, cx), None);
    run("dock.show_project", cx);
    assert_eq!(
        sidebar_shows(&workbench, cx),
        Some(DockPanel::Explorer.id())
    );
    run("git.log", cx);
    assert_eq!(sidebar_shows(&workbench, cx), Some(GIT_LOG));
    assert!(focused_in(&git_log_focus(&workbench, cx), cx));
    assert_eq!(shown(&workbench, cx), [TERMINAL]);
    // Its lit button hides the sidebar, focus going to the editor.
    click_rail(&workbench, GIT_LOG, cx);
    assert_eq!(sidebar_shows(&workbench, cx), None);
    assert!(focused_in(&editor_focus(&workbench, cx), cx));
    click_rail(&workbench, GIT_LOG, cx);
    assert_eq!(sidebar_shows(&workbench, cx), Some(GIT_LOG));
}

#[gpui::test]
fn the_move_commands_work_both_ways(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // Problems focused moves to the sidebar, shown and focused there; the
    // bottom panel, which showed it alone, hides.
    run("problems.show", cx);
    assert!(focused_in(&problems_focus(&workbench, cx), cx));
    run("dock.move_tool_window_to_sidebar", cx);
    assert_eq!(sidebar_shows(&workbench, cx), Some(PROBLEMS));
    assert!(focused_in(&problems_focus(&workbench, cx), cx));
    assert!(!read(&workbench, cx, |dock| dock.panel_ids()).contains(&PROBLEMS));
    assert!(!read(&workbench, cx, |dock| dock.is_visible()));
    assert!(!foot(&workbench, cx).contains(&PROBLEMS.0));
    assert!(rail(&workbench, cx).contains(&PROBLEMS));
    // Moving it to the sidebar again does nothing.
    run("dock.move_tool_window_to_sidebar", cx);
    assert_eq!(sidebar_shows(&workbench, cx), Some(PROBLEMS));

    // Search focused moves to the bottom panel's focused group, its foot
    // button lit; the terminal's group, split off from the Git log's.
    run("terminal.toggle", cx);
    run("git.log", cx);
    run("dock.split_panel_right", cx);
    assert_eq!(shown(&workbench, cx), [TERMINAL, GIT_LOG]);
    run("terminal.toggle", cx);
    run("search.show", cx);
    assert!(focused_in(&search_focus(&workbench, cx), cx));
    assert_eq!(sidebar_shows(&workbench, cx), Some(search()));
    let group = read(&workbench, cx, |dock| {
        dock.active().and_then(|active| dock.group_of(active))
    });
    run("dock.move_tool_window_to_bottom_panel", cx);
    assert!(read(&workbench, cx, |dock| dock.panel_ids()).contains(&search()));
    assert_eq!(read(&workbench, cx, |dock| dock.group_of(search())), group);
    assert!(shown(&workbench, cx).contains(&search()));
    assert!(focused_in(&search_focus(&workbench, cx), cx));
    assert!(!read_sidebar(&workbench, cx, |dock| dock.panel_ids()).contains(&search()));
    assert_eq!(
        foot(&workbench, cx),
        [RUNS.0, GIT_LOG.0, TERMINAL.0, search().0]
    );
    assert!(lit(&workbench, cx).contains(&search()));
    // The sidebar shows another panel in its place.
    assert_ne!(sidebar_shows(&workbench, cx), Some(search()));

    // Its lit foot button hides it; search.show shows it back there.
    click_rail(&workbench, search(), cx);
    assert!(!shown(&workbench, cx).contains(&search()));
    assert!(!lit(&workbench, cx).contains(&search()));
    run("search.show", cx);
    assert!(shown(&workbench, cx).contains(&search()));
    assert!(focused_in(&search_focus(&workbench, cx), cx));
    assert!(lit(&workbench, cx).contains(&search()));
    // Moving it to the bottom panel again does nothing.
    run("dock.move_tool_window_to_bottom_panel", cx);
    assert!(shown(&workbench, cx).contains(&search()));

    // The Explorer isn't moved.
    run("dock.show_project", cx);
    let project = workbench.read_with(cx, |workbench, cx| workbench.panel().focus_handle(cx));
    assert!(focused_in(&project, cx));
    run("dock.move_tool_window_to_bottom_panel", cx);
    let explorer = DockPanel::Explorer.id();
    assert!(read_sidebar(&workbench, cx, |dock| dock.panel_ids()).contains(&explorer));
    assert!(!read(&workbench, cx, |dock| dock.panel_ids()).contains(&explorer));
    assert_eq!(sidebar_shows(&workbench, cx), Some(explorer));
    // Nor is anything with the editor focused and no dock showing.
    run("dock.toggle", cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let bottom = bottom(&workbench, cx);
    bottom.update(cx, |dock, cx| {
        dock.dock_mut().set_visible(false);
        cx.notify();
    });
    draw(cx);
    let (in_bottom, in_sidebar) = (
        read(&workbench, cx, |dock| dock.panel_ids()),
        read_sidebar(&workbench, cx, |dock| dock.panel_ids()),
    );
    run("dock.move_tool_window_to_sidebar", cx);
    run("dock.move_tool_window_to_bottom_panel", cx);
    assert_eq!(read(&workbench, cx, |dock| dock.panel_ids()), in_bottom);
    assert_eq!(
        read_sidebar(&workbench, cx, |dock| dock.panel_ids()),
        in_sidebar
    );

    // Moved back, Problems is at the foot again, first.
    run("problems.show", cx);
    run("dock.move_tool_window_to_bottom_panel", cx);
    assert_eq!(
        foot(&workbench, cx),
        [PROBLEMS.0, RUNS.0, GIT_LOG.0, TERMINAL.0, search().0]
    );
    assert!(focused_in(&problems_focus(&workbench, cx), cx));
}

#[gpui::test]
fn placements_come_back_with_the_window_layout(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let (first, cx, _dir) = opened(cx);
    // Search in the bottom panel, in a group of its own, and the Git log in
    // the sidebar, shown there.
    run("problems.show", cx);
    run("search.show", cx);
    run("dock.move_tool_window_to_bottom_panel", cx);
    run("dock.split_panel_right", cx);
    assert_eq!(shown(&first, cx), [PROBLEMS, search()]);
    run("git.log", cx);
    run("dock.move_tool_window_to_sidebar", cx);
    assert_eq!(sidebar_shows(&first, cx), Some(GIT_LOG));
    let entry = first.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(
        entry["placements"],
        serde_json::json!({"git_log": "sidebar", "search": "bottom"})
    );
    assert_eq!(entry["dock"]["active"], Value::from("git_log"));

    // A new workbench puts them back before the docks' state: Search in
    // its group, active, and the Git log active in the sidebar.
    let (second, cx, _dir) = opened(&mut app);
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    assert_eq!(read(&second, cx, |dock| dock.group_count()), 2);
    assert_eq!(read(&second, cx, |dock| dock.group_of(search())), Some(1));
    assert!(shown(&second, cx).contains(&search()));
    assert!(!read(&second, cx, |dock| dock.panel_ids()).contains(&GIT_LOG));
    assert_eq!(sidebar_shows(&second, cx), Some(GIT_LOG));
    assert!(read_sidebar(&second, cx, |dock| dock.panel_ids()).contains(&GIT_LOG));
    assert!(foot(&second, cx).contains(&search().0));
    assert!(!foot(&second, cx).contains(&GIT_LOG.0));
    assert_eq!(
        second.read_with(cx, |workbench, cx| workbench.layout_entry(cx))["placements"],
        entry["placements"]
    );
    // search.show and git.log show them there.
    run("search.show", cx);
    assert!(focused_in(&search_focus(&second, cx), cx));
    run("git.log", cx);
    assert!(focused_in(&git_log_focus(&second, cx), cx));
    assert_eq!(sidebar_shows(&second, cx), Some(GIT_LOG));

    // An entry without placements restores today's places.
    let mut old = entry.clone();
    old.as_object_mut().unwrap().remove("placements");
    old["dock"]["active"] = "search".into();
    old["terminal_dock"]
        .as_object_mut()
        .unwrap()
        .remove("groups");
    // Restored on a workbench with Search moved, it's put back.
    let (third, cx, _dir) = opened(&mut app);
    run("search.show", cx);
    run("dock.move_tool_window_to_bottom_panel", cx);
    assert!(read(&third, cx, |dock| dock.panel_ids()).contains(&search()));
    third.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&old, window, cx)
    });
    draw(cx);
    assert_eq!(sidebar_shows(&third, cx), Some(search()));
    assert_eq!(
        read(&third, cx, |dock| dock.panel_ids()),
        [PROBLEMS, RUNS, DEBUG, TERMINAL, GIT_LOG]
    );
    assert_eq!(
        foot(&third, cx),
        [PROBLEMS.0, RUNS.0, GIT_LOG.0, TERMINAL.0]
    );
    assert!(read_sidebar(&third, cx, |dock| dock.panel_ids()).contains(&search()));
    let entry = third.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert!(entry.get("placements").is_none(), "{entry}");

    // The terminal moved to the sidebar comes back closed, with no session.
    let (fourth, cx, _dir) = opened(&mut app);
    run("terminal.toggle", cx);
    run("dock.move_tool_window_to_sidebar", cx);
    assert_eq!(sidebar_shows(&fourth, cx), Some(TERMINAL));
    let entry = fourth.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(
        entry["placements"],
        serde_json::json!({"terminal": "sidebar"})
    );
    let (fifth, cx, _dir) = opened(&mut app);
    fifth.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    assert!(read_sidebar(&fifth, cx, |dock| dock.panel_ids()).contains(&TERMINAL));
    assert_eq!(sidebar_shows(&fifth, cx), None);
    let terminal = fifth.read_with(cx, |workbench, _| workbench.terminal().clone());
    assert!(terminal.read_with(cx, |terminal, _| terminal.view().is_none()));
    run("terminal.toggle", cx);
    assert_eq!(sidebar_shows(&fifth, cx), Some(TERMINAL));
    assert!(terminal.read_with(cx, |terminal, _| terminal.view().is_some()));
}

#[gpui::test]
fn focus_and_window_keys_follow_a_moved_tool_window(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    problems_and_search_moved(&workbench, cx);
    let (problems, search) = (problems_focus(&workbench, cx), search_focus(&workbench, cx));
    let focused = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.bottom_panel_focused(window, cx)
        })
    };

    // focus_bottom_panel focuses Search, which the bottom panel shows; it
    // has the bottom panel's focus, Problems the sidebar's.
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let shown = workbench.update_in(cx, |workbench, window, cx| {
        workbench.focus_bottom_panel(window, cx)
    });
    assert!(shown);
    assert!(focused_in(&search, cx));
    assert!(focused(&workbench, cx));
    run("problems.show", cx);
    assert!(focused_in(&problems, cx));
    assert!(!focused(&workbench, cx));

    // dock.grow grows the dock holding the focused tool window.
    let sizes = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        let sidebar = read_sidebar(workbench, cx, |dock| dock.current_size());
        (sidebar, read(workbench, cx, |dock| dock.current_size()))
    };
    let before = sizes(&workbench, cx);
    run("dock.grow", cx);
    let grown = sizes(&workbench, cx);
    assert!(
        grown.0 > before.0 && grown.1 == before.1,
        "{before:?} {grown:?}"
    );
    run("search.show", cx);
    assert!(focused_in(&search, cx));
    run("dock.grow", cx);
    let again = sizes(&workbench, cx);
    assert!(
        again.1 > grown.1 && again.0 == grown.0,
        "{grown:?} {again:?}"
    );

    // Vim's window keys: down from Problems in the sidebar goes to Search
    // in the bottom panel, and up from there back to Problems.
    run("problems.show", cx);
    assert!(focused_in(&problems, cx));
    run("dock.focus_down", cx);
    assert!(focused_in(&search, cx));
    run("dock.focus_up", cx);
    assert!(focused_in(&problems, cx));
    // Toward the panes from Problems is the editor.
    let toward = match read_sidebar(&workbench, cx, |dock| dock.position()) {
        kiln_shell::DockPosition::Left => "dock.focus_right",
        _ => "dock.focus_left",
    };
    run(toward, cx);
    assert!(focused_in(&editor_focus(&workbench, cx), cx));
    // Up from Search, come from the editor, is the editor.
    run("search.show", cx);
    run("dock.focus_up", cx);
    assert!(focused_in(&editor_focus(&workbench, cx), cx));

    // A hidden dock's focused tool window leaves focus for the editor:
    // Search's with the bottom panel hidden, Problems' with the sidebar.
    run("search.show", cx);
    assert!(focused_in(&search, cx));
    let bottom = bottom(&workbench, cx);
    workbench.update_in(cx, |workbench, window, cx| {
        bottom.update(cx, |dock, cx| {
            dock.dock_mut().set_visible(false);
            cx.notify();
        });
        workbench.leave_hidden_panel(window, cx);
    });
    draw(cx);
    assert!(focused_in(&editor_focus(&workbench, cx), cx));
    run("problems.show", cx);
    assert!(focused_in(&problems, cx));
    run("dock.toggle", cx);
    assert_eq!(sidebar_shows(&workbench, cx), None);
    assert!(focused_in(&editor_focus(&workbench, cx), cx));
    // Shown again, focus left where it is.
    run("problems.show", cx);
    run("search.show", cx);
    assert!(focused_in(&search, cx));
    assert_eq!(sidebar_shows(&workbench, cx), Some(PROBLEMS));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.leave_hidden_panel(window, cx)
    });
    assert!(focused_in(&search, cx));
}

#[gpui::test]
fn window_keys_across_groups_notify_the_bottom_panel(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // The terminal and the Git log side by side, the Git log focused.
    run("terminal.toggle", cx);
    run("git.log", cx);
    run("dock.split_panel_right", cx);
    assert_eq!(shown(&workbench, cx), [TERMINAL, GIT_LOG]);
    assert!(focused_in(&git_log_focus(&workbench, cx), cx));
    let bottom = bottom(&workbench, cx);
    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = notified.clone();
    cx.update(|_, cx| {
        cx.observe(&bottom, move |_, _| count.set(count.get() + 1))
            .detach()
    });

    // <C-w>h to the terminal's group: the bottom panel notifies, its
    // focused group the terminal's, both foot buttons still lit.
    run("dock.focus_left", cx);
    let terminal = workbench.read_with(cx, |workbench, cx| workbench.terminal().focus_handle(cx));
    assert!(focused_in(&terminal, cx));
    assert!(notified.get() > 0);
    assert_eq!(
        read(&workbench, cx, |dock| dock.rail_highlighted()),
        Some(TERMINAL)
    );
    let lit_now = lit(&workbench, cx);
    assert!(
        lit_now.contains(&TERMINAL) && lit_now.contains(&GIT_LOG),
        "{lit_now:?}"
    );
    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert!(entry["terminal_dock"].get("shown").is_none(), "{entry}");
    run("dock.focus_right", cx);
    assert!(focused_in(&git_log_focus(&workbench, cx), cx));
    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(entry["terminal_dock"]["shown"], Value::from("git_log"));
}
