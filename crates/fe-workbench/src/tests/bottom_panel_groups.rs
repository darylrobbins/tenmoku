//! The bottom panel's groups in the workbench: the Git log dragged beside
//! the terminal shows with it, every shown tool window's foot button lit;
//! a lit foot button (and `debug.show_panels`, and `terminal.toggle` from
//! the terminal) hides just that tool window, its group keeping its place;
//! the palette's Split Panel Right, Move Tool Window to Next or Previous
//! Group, Join Panel Groups and Focus Next Panel Group; and the groups kept
//! in the window's layout.

use std::fs;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::commands::RunCommand;
use gpui::{
    Bounds, Entity, FocusHandle, Focusable as _, Modifiers, MouseButton, Pixels, Point,
    TestAppContext, VisualTestContext, point, px,
};
use kiln_shell::{
    DockView as _, PanelId, Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

use super::open;
use crate::{Dock, Launch, Workbench};

const GIT_LOG: PanelId = PanelId("git_log");
const TERMINAL: PanelId = PanelId("terminal");
const PROBLEMS: PanelId = PanelId("problems");
const DEBUG: PanelId = PanelId(crate::debug_panel::PANEL);
const PROJECT: PanelId = PanelId("project");

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

/// The tool windows the bottom dock's drawn groups show, in order.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<PanelId> {
    read(workbench, cx, |dock| dock.shown_panels())
}

/// The groups the last frame drew.
fn drawn_groups(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<usize> {
    read(workbench, cx, |dock| dock.drawn().drawn_groups())
}

fn group_bounds(
    workbench: &Entity<Workbench>,
    group: usize,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    read(workbench, cx, |dock| dock.drawn().group_bounds(group)).expect("the group drawn")
}

/// The rail's buttons lit, as last drawn.
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

/// Runs `id` as a key would in the workbench.
fn run(id: &str, cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Picks the command titled `title` in the command palette, opened with
/// its key from where focus is.
fn palette(title: &str, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker: Entity<Picker<CommandPalette>> = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        assert_eq!(layer.modal(), Some(COMMAND_PALETTE), "the palette shows");
        layer.modal_view().unwrap().clone().downcast().unwrap()
    });
    cx.simulate_input(title);
    draw(cx);
    let first = picker.read_with(cx, |picker, _| picker.delegate.matches()[0].title.clone());
    assert_eq!(first.as_ref(), title);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let modal = workbench.read_with(cx, |workbench, cx| {
        workbench.shell().read(cx).modals().read(cx).modal()
    });
    assert_eq!(modal, None, "the palette closed");
}

fn focused_in(focus: &FocusHandle, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| focus.contains_focused(window, cx))
}

fn editor_focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx))
}

fn terminal_focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    workbench.read_with(cx, |workbench, cx| workbench.terminal().focus_handle(cx))
}

fn git_log_focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    let sidebar = sidebar(workbench, cx);
    sidebar.read_with(cx, |dock, cx| dock.git_log().unwrap().focus_handle(cx))
}

/// Drags the bottom dock's tab `panel` to `to` and drops it there.
fn drag_tab(
    workbench: &Entity<Workbench>,
    panel: PanelId,
    to: Point<Pixels>,
    cx: &mut VisualTestContext,
) {
    let from = read(workbench, cx, |dock| dock.drawn().tab_bounds(panel)).expect("a tab");
    let from = from.center();
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

/// The terminal shown, then the Git log beside it, on its right, by
/// dragging its tab onto the right quarter of the terminal's content; the
/// terminal focused.
fn git_log_beside_the_terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    run("terminal.toggle", cx);
    run("git.log", cx);
    run("terminal.toggle", cx);
    assert_eq!(shown(workbench, cx), [TERMINAL]);
    let content = read(workbench, cx, |dock| dock.drawn().group_content_bounds(0));
    let content = content.expect("drawn");
    let right_quarter = point(
        content.right() - content.size.width / 8.,
        content.center().y,
    );
    drag_tab(workbench, GIT_LOG, right_quarter, cx);
}

#[gpui::test]
fn the_git_log_dragged_beside_the_terminal_shows_both(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    run("terminal.toggle", cx);
    run("git.log", cx);
    run("terminal.toggle", cx);
    let terminal = terminal_focus(&workbench, cx);
    assert!(focused_in(&terminal, cx));
    assert_eq!(lit(&workbench, cx), [PROJECT, TERMINAL]);

    let content = read(&workbench, cx, |dock| dock.drawn().group_content_bounds(0));
    let content = content.expect("drawn");
    let right_quarter = point(
        content.right() - content.size.width / 8.,
        content.center().y,
    );
    drag_tab(&workbench, GIT_LOG, right_quarter, cx);

    // Two groups drawn side by side, the terminal left and the Git log
    // right, each showing its tool window.
    assert_eq!(drawn_groups(&workbench, cx), [0, 1]);
    assert_eq!(shown(&workbench, cx), [TERMINAL, GIT_LOG]);
    let (left, right) = (
        group_bounds(&workbench, 0, cx),
        group_bounds(&workbench, 1, cx),
    );
    assert!(left.right() <= right.left(), "{left:?} {right:?}");
    assert_eq!(left.top(), right.top());
    let tabs = read(&workbench, cx, |dock| {
        (dock.drawn().group_tabs(0), dock.drawn().group_tabs(1))
    });
    assert_eq!(tabs, (vec![TERMINAL], vec![GIT_LOG]));
    // Both foot buttons lit.
    assert_eq!(lit(&workbench, cx), [PROJECT, GIT_LOG, TERMINAL]);
    // Focus stays in the terminal the drop left shown.
    assert!(focused_in(&terminal, cx));
    let bottom = bottom(&workbench, cx);
    let hidden = cx.update(|window, cx| bottom.read(cx).dock().hidden_panel_focused(window, cx));
    assert!(!hidden);
}

#[gpui::test]
fn a_lit_foot_button_hides_only_its_tool_and_brings_it_back_in_place(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    git_log_beside_the_terminal(&workbench, cx);
    assert_eq!(shown(&workbench, cx), [TERMINAL, GIT_LOG]);
    let width = group_bounds(&workbench, 1, cx).size.width;
    let whole = group_bounds(&workbench, 0, cx).size.width + width;

    // The Git log focused, its lit button hides it alone: the terminal
    // fills the panel, which stays shown, and focus goes to the editor.
    run("git.log", cx);
    let log = git_log_focus(&workbench, cx);
    assert!(focused_in(&log, cx));
    click_rail(&workbench, GIT_LOG, cx);
    assert_eq!(shown(&workbench, cx), [TERMINAL]);
    assert!(read(&workbench, cx, |dock| dock.is_visible()));
    assert!(read(&workbench, cx, |dock| dock.is_collapsed(1)));
    assert_eq!(drawn_groups(&workbench, cx), [0]);
    let filled = group_bounds(&workbench, 0, cx).size.width;
    assert!((filled - whole).abs() <= px(1.), "{filled:?} {whole:?}");
    assert_eq!(lit(&workbench, cx), [PROJECT, TERMINAL]);
    assert!(focused_in(&editor_focus(&workbench, cx), cx));

    // Clicked again, it shows back in the right group at its old width.
    click_rail(&workbench, GIT_LOG, cx);
    assert_eq!(shown(&workbench, cx), [TERMINAL, GIT_LOG]);
    assert_eq!(read(&workbench, cx, |dock| dock.group_of(GIT_LOG)), Some(1));
    assert_eq!(group_bounds(&workbench, 1, cx).size.width, width);
    assert_eq!(lit(&workbench, cx), [PROJECT, GIT_LOG, TERMINAL]);

    // The terminal's button hides it alone too, focus elsewhere staying.
    let log = git_log_focus(&workbench, cx);
    assert!(focused_in(&log, cx));
    click_rail(&workbench, TERMINAL, cx);
    assert_eq!(shown(&workbench, cx), [GIT_LOG]);
    assert!(focused_in(&log, cx));
    // With only one tool shown, its lit button hides the whole panel.
    click_rail(&workbench, GIT_LOG, cx);
    assert!(!read(&workbench, cx, |dock| dock.is_visible()));
    assert_eq!(shown(&workbench, cx), []);
    assert_eq!(lit(&workbench, cx), [PROJECT]);
    assert!(focused_in(&editor_focus(&workbench, cx), cx));

    // terminal.toggle with the terminal focused beside the Git log hides
    // just the terminal.
    run("terminal.toggle", cx);
    run("git.log", cx);
    assert_eq!(shown(&workbench, cx), [TERMINAL, GIT_LOG]);
    run("terminal.toggle", cx);
    assert!(focused_in(&terminal_focus(&workbench, cx), cx));
    run("terminal.toggle", cx);
    assert_eq!(shown(&workbench, cx), [GIT_LOG]);
    assert!(read(&workbench, cx, |dock| dock.is_visible()));
    assert!(focused_in(&editor_focus(&workbench, cx), cx));
    // And shows it back on the left.
    run("terminal.toggle", cx);
    assert_eq!(shown(&workbench, cx), [TERMINAL, GIT_LOG]);

    // debug.show_panels with Debug shown beside the terminal hides just
    // Debug.
    run("debug.show_panels", cx);
    let debug = workbench.read_with(cx, |workbench, cx| workbench.debug_panel().focus_handle(cx));
    assert!(focused_in(&debug, cx));
    let group = read(&workbench, cx, |dock| dock.group_of(DEBUG)).unwrap();
    let shown_now = shown(&workbench, cx);
    assert!(
        shown_now.contains(&DEBUG) && shown_now.len() == 2,
        "{shown_now:?}"
    );
    run("debug.show_panels", cx);
    assert!(!shown(&workbench, cx).contains(&DEBUG));
    assert!(read(&workbench, cx, |dock| dock.is_visible()));
    assert!(read(&workbench, cx, |dock| dock.is_collapsed(group)));
    assert!(focused_in(&editor_focus(&workbench, cx), cx));
    run("debug.show_panels", cx);
    assert_eq!(shown(&workbench, cx), shown_now);
}

#[gpui::test]
fn the_commands_split_move_join_and_focus_groups(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let groups = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        read(workbench, cx, |dock| dock.group_count())
    };
    // The Git log alone: no split, no move, no focus, no join.
    run("git.log", cx);
    let log = git_log_focus(&workbench, cx);
    assert!(focused_in(&log, cx));
    for title in [
        "Split Panel Right",
        "Move Tool Window to Next Group",
        "Move Tool Window to Previous Group",
        "Join Panel Groups",
        "Focus Next Panel Group",
    ] {
        palette(title, &workbench, cx);
        assert_eq!(groups(&workbench, cx), 1, "{title}");
        assert_eq!(shown(&workbench, cx), [GIT_LOG], "{title}");
        assert!(focused_in(&log, cx), "{title}");
    }

    // With the terminal and Problems open too, the Git log focused splits
    // off into a group after its own, keeping focus.
    run("terminal.toggle", cx);
    run("problems.show", cx);
    run("git.log", cx);
    assert!(focused_in(&log, cx));
    palette("Split Panel Right", &workbench, cx);
    assert_eq!(groups(&workbench, cx), 2);
    assert_eq!(shown(&workbench, cx), [TERMINAL, GIT_LOG]);
    assert_eq!(
        read(&workbench, cx, |dock| dock.open_panels_in(0)),
        [PROBLEMS, TERMINAL]
    );
    assert!(focused_in(&log, cx));
    // In the last group, it has no next one.
    palette("Move Tool Window to Next Group", &workbench, cx);
    assert_eq!(groups(&workbench, cx), 2);
    assert_eq!(read(&workbench, cx, |dock| dock.group_of(GIT_LOG)), Some(1));

    // Moved to the previous group, its own goes away, the Git log active
    // and focused there.
    palette("Move Tool Window to Previous Group", &workbench, cx);
    assert_eq!(groups(&workbench, cx), 1);
    assert_eq!(shown(&workbench, cx), [GIT_LOG]);
    assert_eq!(
        read(&workbench, cx, |dock| dock.open_panels_in(0)),
        [PROBLEMS, TERMINAL, GIT_LOG]
    );
    assert!(focused_in(&log, cx));
    assert!(read(&workbench, cx, |dock| dock.is_visible()));
    // In the first group, it has no previous one.
    palette("Split Panel Right", &workbench, cx);
    assert_eq!(groups(&workbench, cx), 2);
    palette("Move Tool Window to Previous Group", &workbench, cx);
    palette("Move Tool Window to Previous Group", &workbench, cx);
    assert_eq!(groups(&workbench, cx), 1);

    // Focus Next Panel Group goes to the next group's tool window, and
    // wraps.
    palette("Split Panel Right", &workbench, cx);
    assert_eq!(shown(&workbench, cx), [TERMINAL, GIT_LOG]);
    palette("Focus Next Panel Group", &workbench, cx);
    let terminal = terminal_focus(&workbench, cx);
    assert!(focused_in(&terminal, cx));
    palette("Focus Next Panel Group", &workbench, cx);
    assert!(focused_in(&log, cx));
    palette("Focus Next Panel Group", &workbench, cx);
    assert!(focused_in(&terminal, cx));

    // The terminal focused moves to the next group, Problems keeping the
    // first: active and focused in the Git log's.
    palette("Move Tool Window to Next Group", &workbench, cx);
    assert_eq!(groups(&workbench, cx), 2);
    assert_eq!(shown(&workbench, cx), [PROBLEMS, TERMINAL]);
    assert_eq!(
        read(&workbench, cx, |dock| dock.open_panels_in(1)),
        [GIT_LOG, TERMINAL]
    );
    assert!(focused_in(&terminal, cx));

    // Joined, one group, the terminal shown and focused.
    palette("Join Panel Groups", &workbench, cx);
    assert_eq!(groups(&workbench, cx), 1);
    assert_eq!(shown(&workbench, cx), [TERMINAL]);
    assert_eq!(
        read(&workbench, cx, |dock| dock.open_panels_in(0)),
        [PROBLEMS, GIT_LOG, TERMINAL]
    );
    assert!(focused_in(&terminal, cx));
    assert!(read(&workbench, cx, |dock| dock.is_visible()));
}

#[gpui::test]
fn groups_come_back_with_the_window_layout(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let (first, cx, _dir) = opened(cx);
    git_log_beside_the_terminal(&first, cx);
    assert_eq!(shown(&first, cx), [TERMINAL, GIT_LOG]);
    let entry = first.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    let groups = entry["terminal_dock"]["groups"]
        .as_array()
        .expect("groups kept");
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[1]["panels"], serde_json::json!(["git_log"]));
    let panels = groups[0]["panels"].as_array().unwrap();
    assert!(panels.contains(&Value::from("terminal")), "{panels:?}");

    // Restored, the Git log shows in the right group; the terminal comes
    // back closed, with no session, but in its place on the left.
    let (second, cx, _dir) = opened(&mut app);
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    assert_eq!(shown(&second, cx), [GIT_LOG]);
    assert_eq!(read(&second, cx, |dock| dock.group_count()), 2);
    assert_eq!(read(&second, cx, |dock| dock.group_of(GIT_LOG)), Some(1));
    assert_eq!(read(&second, cx, |dock| dock.group_of(TERMINAL)), Some(0));
    assert_eq!(read(&second, cx, |dock| dock.open_panels()), [GIT_LOG]);
    let terminal = second.read_with(cx, |workbench, _| workbench.terminal().clone());
    assert!(terminal.read_with(cx, |terminal, _| terminal.view().is_none()));
    // terminal.toggle shows it there.
    run("terminal.toggle", cx);
    assert_eq!(shown(&second, cx), [TERMINAL, GIT_LOG]);
    assert!(terminal.read_with(cx, |terminal, _| terminal.view().is_some()));
    let (left, right) = (group_bounds(&second, 0, cx), group_bounds(&second, 1, cx));
    assert!(left.right() <= right.left(), "{left:?} {right:?}");

    // An entry saved before groups restores one group, as it did.
    let mut old = entry.clone();
    old["terminal_dock"]
        .as_object_mut()
        .unwrap()
        .remove("groups");
    old["terminal_dock"]["shown"] = "git_log".into();
    let (third, cx, _dir) = opened(&mut app);
    third.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&old, window, cx)
    });
    draw(cx);
    assert_eq!(shown(&third, cx), [GIT_LOG]);
    assert_eq!(read(&third, cx, |dock| dock.group_count()), 1);
    run("terminal.toggle", cx);
    assert_eq!(shown(&third, cx), [TERMINAL]);
    assert_eq!(
        read(&third, cx, |dock| dock.open_panels()),
        [TERMINAL, GIT_LOG]
    );
}
