//! The terminal dock draws no rail of its own: its button is the sidebar's
//! Terminal action, and `terminal.toggle` shows and hides it.

use std::fs;

use fe_buffer::Buffer;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_shell::{DockView as _, PanelId};

use crate::{Launch, Workbench, tests::open};

const BUTTON: PanelId = PanelId("terminal");

fn opened(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n")))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.set_shell("/bin/sh", &[]));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    // Keeps the temporary directory for the test's length.
    std::mem::forget(dir);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.read_with(cx, |dock, _| dock.is_visible())
}

/// The terminal dock's rail buttons: icons drawn, and the button's bounds.
fn own_rail(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.read_with(cx, |dock, _| {
        let drawn = dock.dock().drawn();
        !drawn.rail_icons().is_empty()
            || drawn.rail_bounds().is_some()
            || drawn.rail_button_bounds(BUTTON).is_some()
    })
}

#[gpui::test]
fn the_terminal_dock_draws_no_rail_button(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    assert!(!own_rail(&workbench, cx));

    cx.simulate_keystrokes("ctrl-`");
    draw(cx);
    assert!(shown(&workbench, cx));
    assert!(!own_rail(&workbench, cx));

    cx.simulate_keystrokes("ctrl-`");
    draw(cx);
    assert!(!shown(&workbench, cx));
    assert!(!own_rail(&workbench, cx));

    // The sidebar's Terminal action is still drawn, and toggles it.
    let sidebar = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let at = sidebar
        .read_with(cx, |dock, _| {
            dock.state().drawn().rail_button_bounds(BUTTON)
        })
        .expect("the sidebar rail has a Terminal button")
        .center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert!(shown(&workbench, cx));
    assert!(!own_rail(&workbench, cx));
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert!(!shown(&workbench, cx));
}
