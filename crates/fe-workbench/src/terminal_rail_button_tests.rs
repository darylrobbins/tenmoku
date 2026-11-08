//! The sidebar rail's Terminal button: it opens the terminal, and its
//! highlight follows the bottom dock.

use std::{fs, time::Duration};

use fe_buffer::Buffer;
use fe_ui::Icon;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_shell::PanelId;

use crate::{Launch, Workbench, tests::open};

const BUTTON: PanelId = PanelId("terminal");

fn opened(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
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
    // Keeps the temporary directory for the test's length.
    std::mem::forget(dir);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn dock_shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.read_with(cx, |dock, _| dock.is_visible())
}

fn lit(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    dock.read_with(cx, |dock, _| {
        dock.state().drawn().rail_highlighted().contains(&BUTTON)
    })
}

fn terminal_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    let view = terminal.read_with(cx, |terminal, _| terminal.view().cloned());
    let view = view.expect("a session runs");
    cx.update(|window, cx| view.focus_handle(cx).is_focused(window))
}

fn button(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    dock.read_with(cx, |dock, _| {
        dock.state().drawn().rail_button_bounds(BUTTON)
    })
    .expect("the sidebar rail has a Terminal button")
}

fn click(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let at = button(workbench, cx).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn the_button_opens_and_hides_the_terminal(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let icons = dock.read_with(cx, |dock, _| dock.state().drawn().rail_icons());
    assert_eq!(icons.last(), Some(&Icon::FileTerminal));
    assert!(!lit(&workbench, cx));

    // Its tooltip names the terminal and the key.
    let at = button(&workbench, cx).center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(600));
    draw(cx);
    let tooltip = dock.read_with(cx, |dock, _| dock.state().drawn().last_tooltip());
    let tooltip = tooltip.expect("a tooltip");
    assert!(tooltip.starts_with("Terminal"), "{tooltip}");
    assert!(tooltip.len() > "Terminal".len(), "no key in {tooltip:?}");
    cx.simulate_mouse_move(
        gpui::point(gpui::px(1.), gpui::px(1.)),
        None,
        Modifiers::none(),
    );

    click(&workbench, cx);
    assert!(dock_shown(&workbench, cx));
    assert!(terminal_focused(&workbench, cx));
    assert!(lit(&workbench, cx));

    click(&workbench, cx);
    assert!(!dock_shown(&workbench, cx));
    assert!(!lit(&workbench, cx));
}

#[gpui::test]
fn the_highlight_follows_toggle_and_exit(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    cx.simulate_keystrokes("ctrl-`");
    draw(cx);
    assert!(dock_shown(&workbench, cx));
    assert!(lit(&workbench, cx));
    cx.simulate_keystrokes("ctrl-`");
    draw(cx);
    assert!(!dock_shown(&workbench, cx));
    assert!(!lit(&workbench, cx));

    // The shell's exit hides the dock.
    cx.simulate_keystrokes("ctrl-`");
    draw(cx);
    assert!(lit(&workbench, cx));
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    let view = terminal.read_with(cx, |terminal, _| terminal.view().cloned());
    let view = view.expect("a session");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while view.read_with(cx, |view, _| view.terminal().is_none()) {
        assert!(std::time::Instant::now() < deadline, "the shell to start");
        draw(cx);
        std::thread::sleep(Duration::from_millis(10));
    }
    // Enter is a CR, which Windows' console takes and no other end of line.
    let exit: &[u8] = if cfg!(windows) { b"exit\r" } else { b"exit\n" };
    view.read_with(cx, |view, _| view.terminal().unwrap().input(exit.to_vec()));
    while dock_shown(&workbench, cx) {
        assert!(std::time::Instant::now() < deadline, "the shell to exit");
        draw(cx);
        std::thread::sleep(Duration::from_millis(10));
    }
    draw(cx);
    assert!(!lit(&workbench, cx));
}
