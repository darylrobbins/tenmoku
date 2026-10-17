//! The docks' sizes: the sidebar and the terminal's dock dragged by their
//! inner edges are kept in the window's layout and drawn at those sizes
//! when it's restored, the terminal's dock still hidden until shown; and
//! `dock.grow` and `dock.shrink` size the dock holding focus, else the
//! sidebar, from the palette, with no default key, stopping where the
//! centre keeps its least.

use std::fs;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::commands::RunCommand;
use fe_keymap::{DEFAULT_LINUX, DEFAULT_MACOS};
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};
use kiln_shell::{
    DockView as _,
    dock::{DrawnDock, MIN_DOCK_HEIGHT},
    pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH},
};

use crate::{Launch, Workbench, dock_layout::STEP, tests::open};

/// The sidebar's and the terminal dock's sizes as built.
const BUILT: Pixels = px(240.);

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

/// The active pane's bounds: the whole centre, with one pane.
fn centre(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).bounds())
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

/// The sidebar's state as drawn, and its size.
fn sidebar(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (DrawnDock, Pixels) {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx).state();
        (dock.drawn().clone(), dock.current_size())
    })
}

/// The terminal dock's state as drawn, its size and whether it shows.
fn terminal_dock(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (DrawnDock, Pixels, bool) {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.terminal_dock().read(cx).dock();
        (dock.drawn().clone(), dock.current_size(), dock.is_visible())
    })
}

fn column(drawn: &DrawnDock) -> Bounds<Pixels> {
    drawn.column_bounds().expect("the dock shows")
}

/// Drags the handle on `drawn`'s inner edge by `by`.
fn drag(drawn: &DrawnDock, by: Point<Pixels>, cx: &mut VisualTestContext) {
    let at = drawn.handle_bounds().expect("the dock shows").center();
    let to = at + by;
    let none = Modifiers::none();
    cx.simulate_mouse_down(at, MouseButton::Left, none);
    cx.simulate_mouse_move(at + point(px(3.), px(3.)), MouseButton::Left, none);
    cx.simulate_mouse_move(to, MouseButton::Left, none);
    draw(cx);
    cx.simulate_mouse_up(to, MouseButton::Left, none);
    draw(cx);
}

#[gpui::test]
fn dragged_sizes_are_kept_in_the_layout(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let (first, cx, _dir) = opened(cx);
    run("terminal.toggle", cx);
    let (drawn, size) = sidebar(&first, cx);
    assert_eq!(size, BUILT);
    drag(&drawn, point(px(-80.), px(0.)), cx);
    let (drawn, size, shown) = terminal_dock(&first, cx);
    assert!(shown);
    assert_eq!(size, BUILT);
    drag(&drawn, point(px(0.), px(-160.)), cx);
    assert_eq!(sidebar(&first, cx).1, px(320.));
    assert_eq!(terminal_dock(&first, cx).1, px(400.));

    let entry = first.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(entry["dock"]["size"].as_f64(), Some(320.));
    assert_eq!(entry["terminal_dock"]["size"].as_f64(), Some(400.));
    // Only the sizes, at its position and at each: the terminal's dock
    // comes back hidden.
    let mut keys: Vec<_> = (entry["terminal_dock"].as_object().unwrap().keys()).collect();
    keys.sort();
    assert_eq!(keys, ["bottom_size", "side_size", "size"]);

    let (second, cx, _dir) = opened(&mut app);
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    let (drawn, size) = sidebar(&second, cx);
    assert_eq!((size, column(&drawn).size.width), (px(320.), px(320.)));
    let (_, size, shown) = terminal_dock(&second, cx);
    assert_eq!((size, shown), (px(400.), false));
    let terminal = second.read_with(cx, |workbench, _| workbench.terminal().clone());
    let session = terminal.read_with(cx, |terminal, _| terminal.view().is_some());
    assert!(!session, "no session until shown");
    run("terminal.toggle", cx);
    let (drawn, _, shown) = terminal_dock(&second, cx);
    assert!(shown);
    assert_eq!(column(&drawn).size.height, px(400.));

    // A saved entry without the terminal's dock leaves it as built.
    let (third, cx, _dir) = opened(&mut app);
    let old = Value::from_iter([("breadcrumbs", Value::from(true))]);
    third.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&old, window, cx)
    });
    assert_eq!(terminal_dock(&third, cx).1, BUILT);
}

#[gpui::test]
fn grow_and_shrink_size_the_focused_dock(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // An editor focused: the sidebar.
    run("dock.grow", cx);
    assert_eq!(sidebar(&workbench, cx).1, BUILT + STEP);
    let (drawn, _) = sidebar(&workbench, cx);
    assert_eq!(column(&drawn).size.width, BUILT + STEP);
    assert_eq!(terminal_dock(&workbench, cx).1, BUILT);
    run("dock.shrink", cx);
    assert_eq!(sidebar(&workbench, cx).1, BUILT);
    // Hidden, it shows as it grows.
    run("dock.toggle", cx);
    assert!(!workbench.read_with(cx, |workbench, cx| workbench.dock().read(cx).is_visible()));
    run("dock.grow", cx);
    assert!(workbench.read_with(cx, |workbench, cx| workbench.dock().read(cx).is_visible()));
    assert_eq!(sidebar(&workbench, cx).1, BUILT + STEP);

    // The terminal focused: its dock.
    run("terminal.toggle", cx);
    run("dock.grow", cx);
    let (drawn, size, _) = terminal_dock(&workbench, cx);
    assert_eq!(size, BUILT + STEP);
    assert_eq!(column(&drawn).size.height, BUILT + STEP);
    assert_eq!(sidebar(&workbench, cx).1, BUILT + STEP);
    for _ in 0..10 {
        run("dock.shrink", cx);
    }
    assert_eq!(terminal_dock(&workbench, cx).1, MIN_DOCK_HEIGHT);

    // Grown as far as they go, the centre keeps its least, beside the
    // status bar and the docks' rails.
    for _ in 0..40 {
        run("dock.grow", cx);
    }
    assert_eq!(centre(&workbench, cx).size.height, MIN_PANE_HEIGHT);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    for _ in 0..60 {
        run("dock.grow", cx);
    }
    assert_eq!(centre(&workbench, cx).size.width, MIN_PANE_WIDTH);
}

#[test]
fn grow_and_shrink_are_in_the_palette_with_no_default_key() {
    for id in ["dock.grow", "dock.shrink"] {
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
