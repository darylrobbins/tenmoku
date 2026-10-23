//! The default keymap's keys show, hide, add and cycle the terminal
//! sessions.

use std::fs;

use fe_buffer::Buffer;
use fe_terminal_view::TerminalView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, TerminalPanel, Workbench, tests::open};

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
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.set_shell("/bin/sh", &[]));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

fn dock_shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.read_with(cx, |dock, _| dock.is_visible())
}

fn sessions(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<Entity<TerminalView>> {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, _| terminal.views().cloned().collect())
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    terminal(workbench, cx).read_with(cx, |terminal, _| terminal.active())
}

/// Whether session `ix` is active and has focus.
fn active_and_focused(
    workbench: &Entity<Workbench>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> bool {
    let view = sessions(workbench, cx)[ix].clone();
    active(workbench, cx) == ix && cx.update(|window, cx| view.focus_handle(cx).is_focused(window))
}

fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows");
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// The editor pane's tab count and its active tab's index.
fn editor_tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (usize, Option<usize>) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    pane.read_with(cx, |pane, _| (pane.tabs().len(), pane.active()))
}

#[gpui::test]
fn ctrl_backtick_shows_then_hides(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    assert!(editor_focused(&workbench, cx));

    keys("ctrl-`", cx);
    assert!(dock_shown(&workbench, cx));
    assert_eq!(sessions(&workbench, cx).len(), 1);
    assert!(active_and_focused(&workbench, 0, cx));

    keys("ctrl-`", cx);
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));
}

#[gpui::test]
fn new_and_cycle_keys(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);

    // A second editor tab, so cycling the editor's tabs would show.
    let second = dir.path().join("b.rs");
    fs::write(&second, "fn b() {}\n").unwrap();
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    pane.update_in(cx, |pane, window, cx| pane.open(second, window, cx));
    draw(cx);
    let tabs = editor_tabs(&workbench, cx);
    assert_eq!(tabs.0, 2);

    keys("ctrl-`", cx);
    keys("ctrl-shift-`", cx);
    assert_eq!(sessions(&workbench, cx).len(), 2);
    assert!(active_and_focused(&workbench, 1, cx));
    assert_eq!(editor_tabs(&workbench, cx), tabs);

    keys("cmd-shift-[", cx);
    assert!(active_and_focused(&workbench, 0, cx));
    assert_eq!(editor_tabs(&workbench, cx), tabs);
    keys("cmd-shift-]", cx);
    assert!(active_and_focused(&workbench, 1, cx));
    assert_eq!(editor_tabs(&workbench, cx), tabs);
}
