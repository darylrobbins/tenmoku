//! The bottom panel in B3 (`docs/design/design-system.md`, Workbench
//! anatomy and Measure): Problems, the terminal and the Git log are tool
//! windows of the bottom dock, opened from the rail's foot; its header is a
//! 34 px tab strip, a tab per open tool window, ending in the pane
//! controls; it is 250 px tall as built; the Git log is no longer a view
//! under Changes; and a layout saved before, Problems in the sidebar,
//! still loads. Also the sidebar's ⋯ menu as focus leaves it or the dock
//! hides (T1541 F8).

use std::fs;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::commands::RunCommand;
use fe_ui::TAB_HEIGHT;
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext, px,
};
use kiln_shell::{
    DockPosition, DockView as _, PanelId,
    dock::{HIDE_BUTTON, MORE_BUTTON},
};

use super::open;
use crate::{Dock, DockPanel, Launch, Workbench, dock::DockOp};

const PROBLEMS: PanelId = PanelId("problems");
const GIT_LOG: PanelId = PanelId("git_log");
const TERMINAL: PanelId = PanelId("terminal");

/// A workbench on a temporary project showing `a.rs`, focused as the app
/// focuses it, in an active window, as focus listeners need. The terminal
/// starts no shell.
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
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn sidebar(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Dock> {
    workbench.read_with(cx, |workbench, _| workbench.dock().clone())
}

fn bottom(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<kiln_shell::Dock> {
    workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone())
}

/// Whether the bottom dock shows, and the tool window it shows.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PanelId> {
    let bottom = bottom(workbench, cx);
    bottom.read_with(cx, |dock, _| dock.dock().rail_highlighted())
}

/// The bottom dock's tabs, as last drawn.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<PanelId> {
    let bottom = bottom(workbench, cx);
    bottom.read_with(cx, |dock, _| dock.dock().drawn().tabs())
}

/// The rail's foot buttons lit, as last drawn.
fn lit(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<PanelId> {
    let sidebar = sidebar(workbench, cx);
    sidebar.read_with(cx, |dock, _| dock.state().drawn().rail_highlighted())
}

/// Clicks the rail's button `id`.
fn click_rail(workbench: &Entity<Workbench>, id: PanelId, cx: &mut VisualTestContext) {
    let sidebar = sidebar(workbench, cx);
    let button = sidebar.read_with(cx, |dock, _| {
        dock.state().drawn().rail_button_bounds(id).expect("drawn")
    });
    cx.simulate_click(button.center(), Modifiers::none());
    draw(cx);
}

fn tab_bounds(
    workbench: &Entity<Workbench>,
    id: PanelId,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    let bottom = bottom(workbench, cx);
    bottom.read_with(cx, |dock, _| {
        dock.dock().drawn().tab_bounds(id).expect("a tab drawn")
    })
}

/// Runs `id` as a key would in the workbench.
fn run(id: &str, cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn focused_in(focus: &gpui::FocusHandle, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| focus.contains_focused(window, cx))
}

#[gpui::test]
fn the_rails_foot_opens_tool_windows_in_the_bottom_panel(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let sidebar = sidebar(&workbench, cx);
    let ids = sidebar.read_with(cx, |dock, _| dock.state().drawn().rail_ids());
    assert_eq!(ids[ids.len() - 3..], [PROBLEMS, GIT_LOG, TERMINAL]);
    assert!(!sidebar.read_with(cx, |dock, _| dock.state().panel_ids().contains(&PROBLEMS)));
    assert_eq!(shown(&workbench, cx), None);
    assert!(tabs(&workbench, cx).is_empty());

    // Problems opens in the bottom panel, focused; the sidebar keeps its
    // Explorer, and Problems' button alone is lit.
    click_rail(&workbench, PROBLEMS, cx);
    assert_eq!(shown(&workbench, cx), Some(PROBLEMS));
    assert_eq!(tabs(&workbench, cx), [PROBLEMS]);
    assert_eq!(lit(&workbench, cx), [PanelId("project"), PROBLEMS]);
    let sidebar_shows = sidebar.read_with(cx, |dock, _| (dock.is_visible(), dock.active()));
    assert_eq!(sidebar_shows, (true, DockPanel::Explorer));
    let problems = sidebar.read_with(cx, |dock, cx| dock.problems().focus_handle(cx));
    assert!(focused_in(&problems, cx));

    // The Git log, then the terminal, join it, a tab each, in the order
    // they were added: Problems, the terminal, the Git log.
    click_rail(&workbench, GIT_LOG, cx);
    assert_eq!(shown(&workbench, cx), Some(GIT_LOG));
    assert_eq!(lit(&workbench, cx), [PanelId("project"), GIT_LOG]);
    let log = sidebar.read_with(cx, |dock, cx| dock.git_log().unwrap().focus_handle(cx));
    assert!(focused_in(&log, cx));
    click_rail(&workbench, TERMINAL, cx);
    assert_eq!(shown(&workbench, cx), Some(TERMINAL));
    assert_eq!(tabs(&workbench, cx), [PROBLEMS, TERMINAL, GIT_LOG]);
    assert_eq!(lit(&workbench, cx), [PanelId("project"), TERMINAL]);

    // A lit button hides the bottom panel, focus going to the editor.
    click_rail(&workbench, TERMINAL, cx);
    assert_eq!(shown(&workbench, cx), None);
    assert_eq!(lit(&workbench, cx), [PanelId("project")]);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));
    assert!(focused_in(&editor, cx));

    // problems.show shows it again, the tabs as they were.
    run("problems.show", cx);
    assert_eq!(shown(&workbench, cx), Some(PROBLEMS));
    assert_eq!(tabs(&workbench, cx), [PROBLEMS, TERMINAL, GIT_LOG]);
}

#[gpui::test]
fn its_header_is_a_34_px_tab_strip_ending_in_the_pane_controls(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // The sidebar hidden, the bottom panel's controls are the only ones.
    run("dock.toggle", cx);
    click_rail(&workbench, PROBLEMS, cx);
    click_rail(&workbench, GIT_LOG, cx);
    let bottom = bottom(&workbench, cx);
    let (title, controls) = bottom.read_with(cx, |dock, _| {
        let drawn = dock.dock().drawn();
        (drawn.title_bounds().expect("shown"), drawn.controls())
    });
    assert_eq!(TAB_HEIGHT, px(34.));
    assert_eq!(title.size.height, TAB_HEIGHT);
    let ids: Vec<_> = controls.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids.last(), Some(&HIDE_BUTTON));

    // A tab per open tool window along the strip, then the controls at
    // its right end.
    let (problems, log) = (
        tab_bounds(&workbench, PROBLEMS, cx),
        tab_bounds(&workbench, GIT_LOG, cx),
    );
    // Over the strip's 1 px bottom border, as the editor's tabs are.
    for tab in [problems, log] {
        assert_eq!(
            (tab.top(), tab.bottom()),
            (title.top(), title.bottom() - px(1.))
        );
    }
    assert_eq!(problems.left(), title.left());
    assert_eq!(problems.right(), log.left());
    let hide = cx.debug_bounds("icon-button:dock-hide").expect("drawn");
    assert!(log.right() < hide.left(), "{log:?} {hide:?}");
    assert!(title.right() - hide.right() <= px(8.), "{hide:?} {title:?}");
    // The shown one is active: marked while it has focus.
    assert!(cx.debug_bounds("tab-accent:Graph").is_some());

    // A tab shows its tool window; its close button closes it, the next
    // one showing, and with none open the panel hides.
    cx.simulate_click(problems.center(), Modifiers::none());
    draw(cx);
    assert_eq!(shown(&workbench, cx), Some(PROBLEMS));
    bottom.update_in(cx, |dock, window, cx| dock.tab_closed(PROBLEMS, window, cx));
    draw(cx);
    assert_eq!(shown(&workbench, cx), Some(GIT_LOG));
    assert_eq!(tabs(&workbench, cx), [GIT_LOG]);
    bottom.update_in(cx, |dock, window, cx| dock.tab_closed(GIT_LOG, window, cx));
    draw(cx);
    assert_eq!(shown(&workbench, cx), None);

    // — hides it.
    click_rail(&workbench, TERMINAL, cx);
    let hide = cx.debug_bounds("icon-button:dock-hide").expect("drawn");
    cx.simulate_click(hide.center(), Modifiers::none());
    draw(cx);
    assert_eq!(shown(&workbench, cx), None);
}

#[gpui::test]
fn it_is_250_px_tall_as_built(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let bottom = bottom(&workbench, cx);
    let size = bottom.read_with(cx, |dock, _| dock.dock().bottom_size());
    assert_eq!(size, px(250.));
    click_rail(&workbench, PROBLEMS, cx);
    let column = bottom.read_with(cx, |dock, _| dock.dock().drawn().column_bounds());
    assert_eq!(column.expect("shown").size.height, px(250.));
    assert_eq!(
        bottom.read_with(cx, |dock, _| dock.dock().position()),
        DockPosition::Bottom
    );
}

#[gpui::test]
fn the_git_log_is_no_longer_under_changes(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let sidebar = sidebar(&workbench, cx);
    let views = sidebar.read_with(cx, |dock, cx| {
        let stack = dock.source_control().read(cx).stack().read(cx);
        stack.view_ids()
    });
    assert_eq!(views, [crate::dock::source_control::CHANGES]);

    // Its show op shows it in the bottom panel; the sidebar stays.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_dock_op(DockOp::ShowGitLog, window, cx)
    });
    draw(cx);
    assert_eq!(shown(&workbench, cx), Some(GIT_LOG));
    let sidebar_shows = sidebar.read_with(cx, |dock, _| dock.active());
    assert_eq!(sidebar_shows, DockPanel::Explorer);
}

#[gpui::test]
fn old_layouts_load(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // Saved before: Problems shown in the sidebar on the right, the
    // terminal's dock at the bottom, 240 px, and Source Control's Graph.
    let entry = serde_json::json!({
        "docks": {"sidebar": "right", "terminal": "bottom"},
        "dock": {
            "visible": true,
            "active": "problems",
            "joined": ["references"],
            "size": 260.0,
            "side_size": 260.0,
            "bottom_size": 240.0,
            "panels": {"scm": {"views": {
                "changes": {"collapsed": false, "weight": 1.0},
                "graph": {"collapsed": false, "weight": 1.0},
            }}},
        },
        "terminal_dock": {"size": 240.0, "side_size": 240.0, "bottom_size": 240.0},
        "breadcrumbs": true,
    });
    let entry: Value = entry;
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    // The sidebar stays on the rail's side, showing the Explorer.
    let sidebar = sidebar(&workbench, cx);
    let state = sidebar.read_with(cx, |dock, _| {
        (dock.state().position(), dock.is_visible(), dock.active())
    });
    assert_eq!(state, (DockPosition::Left, true, DockPanel::Explorer));
    let views = sidebar.read_with(cx, |dock, cx| {
        dock.source_control().read(cx).stack().read(cx).view_ids()
    });
    assert_eq!(views, [crate::dock::source_control::CHANGES]);
    // Problems shows in the bottom panel, at its saved height.
    assert_eq!(shown(&workbench, cx), Some(PROBLEMS));
    assert_eq!(tabs(&workbench, cx), [PROBLEMS]);
    let bottom = bottom(&workbench, cx);
    let size = bottom.read_with(cx, |dock, _| dock.dock().bottom_size());
    assert_eq!(size, px(240.));
    assert_eq!(
        bottom.read_with(cx, |dock, _| dock.dock().position()),
        DockPosition::Bottom
    );
    // Its tab stays as another tool window opens (F2).
    run("terminal.toggle", cx);
    assert_eq!(tabs(&workbench, cx), [PROBLEMS, TERMINAL]);
    // And the Git log, saved active before it was Source Control's.
    let mut old = entry.clone();
    old["dock"]["active"] = "git_log".into();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&old, window, cx)
    });
    draw(cx);
    assert_eq!(shown(&workbench, cx), Some(GIT_LOG));
}

/// Whether the sidebar's ⋯ menu is open.
fn menu_open(sidebar: &Entity<Dock>, cx: &mut VisualTestContext) -> bool {
    sidebar.read_with(cx, |dock, _| dock.state().is_menu_open())
}

fn open_menu(cx: &mut VisualTestContext) {
    let more = cx
        .debug_bounds(format!("icon-button:{MORE_BUTTON}").leak())
        .expect("drawn");
    cx.simulate_click(more.center(), Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn the_menu_closes_as_a_key_moves_focus_out_of_it(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let sidebar = sidebar(&workbench, cx);
    open_menu(cx);
    assert!(menu_open(&sidebar, cx));
    // Search takes focus, as Cmd-Shift-F gives it: the menu closes, the
    // focus staying in search.
    run("search.show", cx);
    assert!(!menu_open(&sidebar, cx));
    assert!(cx.debug_bounds("dock-menu").is_none());
    let search = sidebar.read_with(cx, |dock, cx| dock.search().focus_handle(cx));
    assert!(focused_in(&search, cx));
    assert_eq!(
        sidebar.read_with(cx, |dock, _| dock.active()),
        DockPanel::Search
    );
}

#[gpui::test]
fn hiding_the_dock_closes_its_menu(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let sidebar = sidebar(&workbench, cx);
    open_menu(cx);
    assert!(menu_open(&sidebar, cx));
    // Cmd-B: the menu closes, focus going back to the editor that had it.
    run("dock.toggle", cx);
    assert!(!sidebar.read_with(cx, |dock, _| dock.is_visible()));
    assert!(!menu_open(&sidebar, cx));
    let editor = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));
    assert!(focused_in(&editor, cx));
    // Shown again, it has no menu open.
    run("dock.toggle", cx);
    assert!(!menu_open(&sidebar, cx));
    assert!(cx.debug_bounds("dock-menu").is_none());
}

#[gpui::test]
fn a_shown_tool_window_is_saved_and_restored_with_its_tab(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let (first, cx, _dir) = opened(cx);
    click_rail(&first, GIT_LOG, cx);
    let entry = first.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(entry["terminal_dock"]["shown"].as_str(), Some("git_log"));
    // The terminal shown is not saved: it comes back hidden.
    click_rail(&first, TERMINAL, cx);
    let terminal = first.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(terminal["terminal_dock"].get("shown"), None);

    let (second, cx, _dir) = opened(&mut app);
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    assert_eq!(shown(&second, cx), Some(GIT_LOG));
    assert_eq!(tabs(&second, cx), [GIT_LOG]);
    // Another opening keeps its tab.
    click_rail(&second, PROBLEMS, cx);
    assert_eq!(tabs(&second, cx), [PROBLEMS, GIT_LOG]);
}

/// Tells the workbench the terminal's last shell exited.
fn exit_terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |_, cx| {
        cx.emit(crate::terminal_panel::TerminalPanelEvent::Exited)
    });
    draw(cx);
}

#[gpui::test]
fn the_last_shell_exiting_closes_the_terminal_tab_alone(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let sidebar = sidebar(&workbench, cx);
    let problems = sidebar.read_with(cx, |dock, cx| dock.problems().focus_handle(cx));
    let editor = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));

    // Problems shown and focused, the terminal open behind it: its tab
    // closes, Problems stays, focus with it.
    click_rail(&workbench, PROBLEMS, cx);
    run("terminal.toggle", cx);
    click_rail(&workbench, PROBLEMS, cx);
    assert!(focused_in(&problems, cx));
    exit_terminal(&workbench, cx);
    assert_eq!(shown(&workbench, cx), Some(PROBLEMS));
    assert_eq!(tabs(&workbench, cx), [PROBLEMS]);
    assert!(focused_in(&problems, cx));

    // The terminal shown and focused: Problems shows in its place, the
    // editor taking focus.
    run("terminal.toggle", cx);
    assert_eq!(shown(&workbench, cx), Some(TERMINAL));
    exit_terminal(&workbench, cx);
    assert_eq!(shown(&workbench, cx), Some(PROBLEMS));
    assert_eq!(tabs(&workbench, cx), [PROBLEMS]);
    assert!(focused_in(&editor, cx));

    // The terminal alone open: the panel hides.
    let bottom = bottom(&workbench, cx);
    bottom.update_in(cx, |dock, window, cx| dock.tab_closed(PROBLEMS, window, cx));
    run("terminal.toggle", cx);
    assert_eq!(tabs(&workbench, cx), [TERMINAL]);
    exit_terminal(&workbench, cx);
    assert_eq!(shown(&workbench, cx), None);
    assert!(focused_in(&editor, cx));
}

#[gpui::test]
fn zen_mode_takes_focus_out_of_the_bottom_panel(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));
    for id in [PROBLEMS, GIT_LOG] {
        click_rail(&workbench, id, cx);
        assert!(!focused_in(&editor, cx), "{id:?}");
        run("workbench.toggle_zen_mode", cx);
        assert_eq!(shown(&workbench, cx), None, "{id:?}");
        assert!(focused_in(&editor, cx), "{id:?}");
        // Out of it, the tool window comes back.
        run("workbench.toggle_zen_mode", cx);
        assert_eq!(shown(&workbench, cx), Some(id), "{id:?}");
    }
}

#[gpui::test]
fn up_from_a_tool_window_goes_back_only_to_where_a_key_came_from(cx: &mut TestAppContext) {
    use crate::Direction;
    let (workbench, cx, _dir) = opened(cx);
    let sidebar = sidebar(&workbench, cx);
    let tree = sidebar.read_with(cx, |dock, cx| dock.project().focus_handle(cx));
    let editor = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));
    let keys = |direction: Direction, cx: &mut VisualTestContext| {
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.focus_from_dock(direction, window, cx)
        });
        draw(cx);
    };
    for id in [PROBLEMS, GIT_LOG] {
        let tool = sidebar.read_with(cx, |dock, cx| match id {
            PROBLEMS => dock.problems().focus_handle(cx),
            _ => dock.git_log().unwrap().focus_handle(cx),
        });
        click_rail(&workbench, id, cx);
        // From the sidebar down, and back up to it.
        cx.update(|window, cx| window.focus(&tree, cx));
        draw(cx);
        keys(Direction::Down, cx);
        assert!(focused_in(&tool, cx), "{id:?}");
        keys(Direction::Up, cx);
        assert!(focused_in(&tree, cx), "{id:?}");
        // Down again, then the editor, then down from the editor: up
        // now goes to the editor, not the sidebar.
        keys(Direction::Down, cx);
        assert!(focused_in(&tool, cx), "{id:?}");
        cx.update(|window, cx| window.focus(&editor, cx));
        draw(cx);
        let entered = workbench.update_in(cx, |workbench, window, cx| {
            workbench.focus_dock_beyond(Direction::Down, window, cx)
        });
        draw(cx);
        assert!(entered, "{id:?}");
        assert!(focused_in(&tool, cx), "{id:?}");
        keys(Direction::Up, cx);
        assert!(focused_in(&editor, cx), "{id:?}");
        assert!(!focused_in(&tree, cx), "{id:?}");
    }
}
