//! The docks' places: the terminal moves to the bottom, left or right and
//! the sidebar to the left or right, from the palette with no default key,
//! one dock per edge, the terminal keeping its sessions and focus; the
//! places are kept in the window's layout; and the sidebar's panels fill
//! its column, however wide.

use std::fs;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::commands::RunCommand;
use fe_keymap::{DEFAULT_LINUX, DEFAULT_MACOS};
use gpui::{
    Bounds, Entity, EntityId, Focusable as _, Pixels, TestAppContext, VisualTestContext, px,
};
use kiln_shell::{DockPosition, DockView as _, dock::MIN_DOCK_WIDTH};

use crate::{
    DockPanel, Launch, Workbench,
    dock_layout::{displaced, position_name},
    tests::open,
};

use DockPosition::{Bottom, Left, Right};

/// A workbench showing `a.rs` from a temporary project, its editor
/// focused, whose terminal runs `/bin/sh`.
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    // The pty's threads wake the terminal's event task.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.set_shell("/bin/sh", &[]));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

/// Draws a frame, then a few of those it asks for.
fn draw(cx: &mut VisualTestContext) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
            return;
        }
    }
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Where the sidebar and the terminal's dock are: their states' positions,
/// and the shell's docks there, which must be theirs.
fn positions(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (DockPosition, DockPosition) {
    workbench.read_with(cx, |workbench, cx| {
        let sidebar = workbench.dock().read(cx).state().position();
        let terminal = workbench.terminal_dock().read(cx).dock().position();
        let shell = workbench.shell().read(cx);
        let at = |position| shell.dock(position).map(|view| view.entity_id());
        assert_eq!(at(sidebar), Some(workbench.dock().entity_id()));
        assert_eq!(at(terminal), Some(workbench.terminal_dock().entity_id()));
        let third = [Left, Right, Bottom]
            .into_iter()
            .find(|position| ![sidebar, terminal].contains(position));
        assert_eq!(at(third.expect("two docks, three edges")), None);
        (sidebar, terminal)
    })
}

/// Where the sidebar's rail and the terminal dock's column were drawn.
fn drawn(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> [Bounds<Pixels>; 2] {
    workbench.read_with(cx, |workbench, cx| {
        let sidebar = workbench.dock().read(cx).state().drawn().rail_bounds();
        let terminal = workbench.terminal_dock().read(cx).dock().drawn();
        [
            sidebar.expect("drawn"),
            terminal.column_bounds().expect("shown"),
        ]
    })
}

/// The terminal's sessions' views.
fn sessions(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<EntityId> {
    workbench.read_with(cx, |workbench, cx| {
        let terminal = workbench.terminal().read(cx);
        terminal.views().map(|view| view.entity_id()).collect()
    })
}

fn terminal_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        workbench
            .read(cx)
            .terminal()
            .read(cx)
            .is_focused(window, cx)
    })
}

#[gpui::test]
fn the_terminal_moves_right_with_its_session_and_focus(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    assert_eq!(positions(&workbench, cx), (Right, Bottom));
    run("terminal.toggle", cx);
    let started = sessions(&workbench, cx);
    assert_eq!(started.len(), 1);
    assert!(terminal_focused(&workbench, cx));

    run("terminal.move_to_right", cx);
    // The sidebar gives way to the side the terminal didn't come from.
    assert_eq!(positions(&workbench, cx), (Left, Right));
    assert_eq!(sessions(&workbench, cx), started);
    assert!(terminal_focused(&workbench, cx));
    let shown = workbench.read_with(cx, |workbench, cx| {
        workbench.terminal_dock().read(cx).is_visible()
    });
    assert!(shown);
    let viewport = cx.update(|window, _| window.viewport_size());
    let [sidebar, terminal] = drawn(&workbench, cx);
    assert_eq!(sidebar.left(), Pixels::ZERO);
    assert!(terminal.left() > viewport.width / 2., "{terminal:?}");
    assert!(terminal.size.height > terminal.size.width, "{terminal:?}");

    run("terminal.move_to_bottom", cx);
    // The sidebar stays where it went.
    assert_eq!(positions(&workbench, cx), (Left, Bottom));
    assert_eq!(sessions(&workbench, cx), started);
    assert!(terminal_focused(&workbench, cx));
    let [sidebar, terminal] = drawn(&workbench, cx);
    assert_eq!(sidebar.left(), Pixels::ZERO);
    assert!(terminal.size.width > terminal.size.height, "{terminal:?}");
}

#[gpui::test]
fn the_sidebar_moved_onto_the_terminal_sends_it_to_the_other_side(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    run("dock.move_sidebar_left", cx);
    assert_eq!(positions(&workbench, cx), (Left, Bottom));
    run("terminal.move_to_right", cx);
    assert_eq!(positions(&workbench, cx), (Left, Right));

    run("dock.move_sidebar_right", cx);
    assert_eq!(positions(&workbench, cx), (Right, Left));
    // The terminal, side to side, swaps back.
    run("terminal.move_to_right", cx);
    assert_eq!(positions(&workbench, cx), (Left, Right));
    // Where it is already, nothing moves.
    run("dock.move_sidebar_left", cx);
    assert_eq!(positions(&workbench, cx), (Left, Right));
}

#[gpui::test]
fn the_places_are_kept_in_the_layout(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let (first, cx, _dir) = opened(cx);
    run("terminal.move_to_right", cx);
    run("dock.move_sidebar_right", cx);
    assert_eq!(positions(&first, cx), (Right, Left));
    let entry = first.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(entry["docks"]["sidebar"].as_str(), Some("right"));
    assert_eq!(entry["docks"]["terminal"].as_str(), Some("left"));

    let (second, cx, _dir) = opened(&mut app);
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    assert_eq!(positions(&second, cx), (Right, Left));
    run("terminal.toggle", cx);
    let [sidebar, terminal] = drawn(&second, cx);
    let viewport = cx.update(|window, _| window.viewport_size());
    assert_eq!(sidebar.right(), viewport.width);
    assert!(terminal.right() < viewport.width / 2., "{terminal:?}");

    // Places that aren't one dock per edge, or the sidebar at the bottom,
    // are left as built.
    for (sidebar, terminal) in [("left", "left"), ("bottom", "right"), ("up", "left")] {
        let (third, cx, _dir) = opened(&mut app);
        let docks = Value::from_iter([("sidebar", sidebar), ("terminal", terminal)]);
        let entry = Value::from_iter([("docks", docks)]);
        third.update_in(cx, |workbench, window, cx| {
            workbench.restore_layout(&entry, window, cx)
        });
        draw(cx);
        assert_eq!(
            positions(&third, cx),
            (Right, Bottom),
            "{sidebar} {terminal}"
        );
    }
}

#[test]
fn a_dock_moved_onto_the_other_sends_it_where_it_came_from_or_across() {
    // Not onto the other: it stays.
    assert_eq!(displaced(Bottom, Left, Right), Right);
    // From a side: to that side.
    assert_eq!(displaced(Left, Right, Right), Left);
    assert_eq!(displaced(Right, Left, Left), Right);
    assert_eq!(displaced(Right, Bottom, Bottom), Right);
    // From the bottom: across.
    assert_eq!(displaced(Bottom, Right, Right), Left);
    assert_eq!(displaced(Bottom, Left, Left), Right);
    assert_eq!(position_name(Bottom), "bottom");
}

#[test]
fn the_move_commands_are_in_the_palette_with_no_default_key() {
    for id in [
        "terminal.move_to_bottom",
        "terminal.move_to_left",
        "terminal.move_to_right",
        "dock.move_sidebar_left",
        "dock.move_sidebar_right",
    ] {
        let listed = crate::command_catalog()
            .check(
                &fe_command::CommandId::parse(id).unwrap(),
                &fe_command::Value::Null,
            )
            .is_ok();
        assert!(listed, "{id}");
        for keymap in [DEFAULT_MACOS, DEFAULT_LINUX] {
            assert!(!keymap.contains(id), "{id}");
        }
    }
}

/// Shows each of the sidebar's panels in turn: each fills its column,
/// `width` across.
fn assert_panels_fill(width: Pixels, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    for panel in [
        DockPanel::Project,
        DockPanel::Search,
        DockPanel::Outline,
        DockPanel::Changes,
        DockPanel::References,
        DockPanel::Problems,
    ] {
        workbench.update_in(cx, |workbench, window, cx| {
            (workbench.dock()).update(cx, |dock, cx| dock.show(panel, window, cx))
        });
        draw(cx);
        let column = workbench.read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).state().drawn().column_bounds()
        });
        let column = column.expect("shown");
        let drawn = cx.debug_bounds("sidebar-panel").expect("drawn");
        assert_eq!(column.size.width, width, "{panel:?}");
        assert_eq!(
            (drawn.left(), drawn.right()),
            (column.left(), column.right()),
            "{panel:?}"
        );
    }
}

#[gpui::test]
fn the_sidebar_panels_fill_its_column_however_wide(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let (first, cx, _dir) = opened(cx);
    for width in [px(440.), MIN_DOCK_WIDTH, px(300.)] {
        first.update(cx, |workbench, cx| {
            (workbench.dock()).update(cx, |dock, cx| {
                dock.dock_mut().set_size(width);
                cx.notify();
            })
        });
        draw(cx);
        assert_panels_fill(width, &first, cx);
    }
    run("dock.move_sidebar_left", cx);
    assert_panels_fill(px(300.), &first, cx);

    let entry = first.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    let (second, cx, _dir) = opened(&mut app);
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    assert_panels_fill(px(300.), &second, cx);
}

/// Sets the terminal dock's size at its position.
fn size_terminal(size: Pixels, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    workbench.update(cx, |workbench, cx| {
        (workbench.terminal_dock()).update(cx, |dock, cx| {
            dock.dock_mut().set_size(size);
            cx.notify();
        })
    });
    draw(cx);
}

fn terminal_size(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Pixels {
    workbench.read_with(cx, |workbench, cx| {
        workbench.terminal_dock().read(cx).dock().current_size()
    })
}

#[gpui::test]
fn the_terminal_keeps_its_width_and_height_in_the_layout(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let (first, cx, _dir) = opened(cx);
    size_terminal(px(400.), &first, cx);
    run("terminal.move_to_right", cx);
    size_terminal(px(320.), &first, cx);
    let entry = first.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(entry["terminal_dock"]["size"].as_f64(), Some(320.));
    assert_eq!(entry["terminal_dock"]["side_size"].as_f64(), Some(320.));
    assert_eq!(entry["terminal_dock"]["bottom_size"].as_f64(), Some(400.));

    let (second, cx, _dir) = opened(&mut app);
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    assert_eq!(positions(&second, cx), (Left, Right));
    assert_eq!(terminal_size(&second, cx), px(320.));
    let terminal = second.read_with(cx, |workbench, cx| {
        let dock = workbench.terminal_dock().read(cx);
        (
            dock.is_visible(),
            workbench.terminal().read(cx).view().is_some(),
        )
    });
    assert_eq!(terminal, (false, false), "hidden, with no session");
    run("terminal.move_to_bottom", cx);
    assert_eq!(terminal_size(&second, cx), px(400.));
    run("terminal.toggle", cx);
    let [_, column] = drawn(&second, cx);
    assert_eq!(column.size.height, px(400.));
}

/// The terminal dock's state that a never-shown terminal must leave alone:
/// whether it shows, whether the terminal is on the rail and drew a button
/// there, whether its rail took room (a side's always does) and whether a
/// column was drawn.
fn terminal_untouched(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> [bool; 5] {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.terminal_dock().read(cx).dock();
        let terminal = crate::terminal_panel::TERMINAL;
        let rail = dock.drawn().rail_bounds();
        [
            dock.is_visible(),
            dock.on_rail(terminal, cx),
            dock.drawn().rail_button_bounds(terminal).is_some(),
            rail.is_some_and(|rail| rail.size.width > px(0.) && rail.size.height > px(0.)),
            dock.drawn().column_bounds().is_some(),
        ]
    })
}

#[gpui::test]
fn restoring_a_never_shown_terminals_sizes_leaves_it_off_the_rail(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let (first, cx, _dir) = opened(cx);
    size_terminal(px(400.), &first, cx);
    run("terminal.move_to_right", cx);
    size_terminal(px(320.), &first, cx);
    let entry = first.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    // Saved before the dock kept both sizes, with no places.
    let legacy = Value::from_iter([(
        "terminal_dock",
        Value::from_iter([("size", Value::from(300.))]),
    )]);

    for (entry, at, size) in [(entry, Right, px(320.)), (legacy, Left, px(300.))] {
        let (second, cx, _dir) = opened(&mut app);
        // Where the layout puts it, so only the restored sizes differ.
        run(&format!("terminal.move_to_{}", position_name(at)), cx);
        let pane = |cx: &mut VisualTestContext| {
            let pane = second.read_with(cx, |workbench, _| workbench.pane().clone());
            pane.read_with(cx, |pane, _| pane.bounds())
        };
        let before = pane(cx);
        let untouched = terminal_untouched(&second, cx);
        assert_eq!(untouched[..3], [false; 3], "hidden, off the rail");
        second.update_in(cx, |workbench, window, cx| {
            workbench.restore_layout(&entry, window, cx)
        });
        draw(cx);
        assert_eq!(positions(&second, cx).1, at);
        assert_eq!(terminal_size(&second, cx), size);
        assert_eq!(terminal_untouched(&second, cx), untouched, "{size:?}");
        assert_eq!(pane(cx), before, "no room taken");
    }
}
