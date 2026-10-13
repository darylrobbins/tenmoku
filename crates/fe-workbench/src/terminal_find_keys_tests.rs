//! The default macOS keymap's keys drive the terminal's find row, not the
//! pane's find bar.

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_terminal_view::TerminalView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, TerminalPanel, Workbench, tests::open};

/// Prints `foo` on rows 0 and 1, then waits.
const SCRIPT: &str = r"printf 'foo\r\nfoo\r\nend\r\n'; read line";

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

/// The rows of the active terminal's selection's first cell.
fn selected_row(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Option<usize> {
    view.read_with(cx, |view, _| {
        let snapshot = view.terminal().unwrap().snapshot();
        snapshot.selection.first().map(|span| span.row)
    })
}

#[gpui::test]
fn keys_drive_the_row(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n")))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let panel = terminal(&workbench, cx);
    panel.update(cx, |panel, _| panel.set_shell("/bin/sh", &["-c", SCRIPT]));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);

    // The terminal panel, shown, with a session that has printed.
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of("terminal.toggle").unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let view = panel.read_with(cx, |panel, _| panel.view().unwrap().clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !view.read_with(cx, |view, _| {
        view.terminal()
            .is_some_and(|terminal| terminal.snapshot().text().contains("end"))
    }) {
        assert!(Instant::now() < deadline, "timed out waiting for the shell");
        draw(cx);
        thread::sleep(Duration::from_millis(10));
    }
    draw(cx);

    // With the terminal focused, cmd-f opens its row, focus in the input.
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    draw(cx);
    keys("cmd-f", cx);
    let find = |cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| {
            let find = panel.find().unwrap();
            (find.is_open(), find.input().clone())
        })
    };
    let (open, input) = find(cx);
    assert!(open);
    assert!(cx.update(|window, cx| input.focus_handle(cx).is_focused(window)));

    // Enter steps up through the terminal's matches, shift-enter down.
    keys("f o o", cx);
    assert_eq!(selected_row(&view, cx), Some(1));
    keys("enter", cx);
    assert_eq!(selected_row(&view, cx), Some(0));
    keys("shift-enter", cx);
    assert_eq!(selected_row(&view, cx), Some(1));
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert!(pane.read_with(cx, |pane, _| {
        pane.find_bar().is_none_or(|bar| !bar.is_open())
    }));

    // Escape closes the row and focuses the terminal.
    keys("escape", cx);
    assert!(!find(cx).0);
    assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)));
}
