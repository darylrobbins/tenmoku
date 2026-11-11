//! The terminal panel in B3 (`docs/design/design-system.md#terminal`,
//! T349): each session is a tab in the panel's own strip, where a session
//! list stood on its right; a tab shows a spinner while its shell's command
//! runs (an OSC 133;C with no D after it) or its task does, and the error
//! mark once that exited with a code other than 0, else the terminal icon;
//! and the active tab has the accent line along its top.

use std::{
    collections::BTreeMap,
    fs, thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_terminal_view::{TerminalView, TerminalViewEvent};
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext, px,
};

use kiln_shell::DockView as _;

use super::open;
use crate::{
    Launch, TerminalPanel, Workbench,
    task_defs::{Source, Task, TaskCommand},
};

/// A workbench on a temporary project showing `a.txt`, which starts no
/// language server, focused as the app focuses it, in an active window.
/// With `fake`, its terminals start no process: a test emits their exits
/// itself.
fn opened(
    cx: &mut TestAppContext,
    fake: bool,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    if !fake {
        // The pty's threads wake the terminal's event task.
        cx.executor().allow_parking();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "a\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.fake_sessions = fake);
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

/// Draws until `done` holds. Panics after 5 s, naming `what`.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Runs `id` as a key would in the workbench.
fn run(id: &str, cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

fn views(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<TerminalView>> {
    terminal(workbench, cx).read_with(cx, |terminal, _| terminal.views().cloned().collect())
}

/// The labels of the tabs the last frame drew.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    terminal(workbench, cx).read_with(cx, |terminal, _| {
        (terminal.drawn_tabs().iter())
            .map(|(label, _)| label.to_string())
            .collect()
    })
}

fn tab_bounds(
    workbench: &Entity<Workbench>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    let terminal = terminal(workbench, cx);
    terminal
        .read_with(cx, |terminal, _| terminal.tab_bounds(ix))
        .expect("the tab was drawn")
}

/// The icon the tab labelled `label` drew before its label, by name, as the
/// last frame drew it: the spinner or the error mark if it drew one, else
/// the terminal icon. (The dock's own tab for the panel, named for the
/// active session, has the terminal icon too.)
fn mark(label: &str, cx: &mut VisualTestContext) -> &'static str {
    let drawn = |icon: &str, cx: &mut VisualTestContext| {
        (cx.debug_bounds(format!("tab-icon:{label}:{icon}").leak())).is_some()
    };
    let (spinner, error) = (drawn("loader-circle", cx), drawn("circle-x", cx));
    assert!(!(spinner && error), "{label} has one mark");
    match (spinner, error) {
        (true, _) => "loader-circle",
        (_, true) => "circle-x",
        _ if drawn("file-terminal", cx) => "file-terminal",
        _ => panic!("{label} is drawn with no icon"),
    }
}

/// Where the accent line of the tab labelled `label` was drawn, if it was.
fn accent(label: &str, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(format!("tab-accent:{label}").leak())
}

/// Types `bytes` into `view`'s shell.
fn type_in(view: &Entity<TerminalView>, bytes: &'static [u8], cx: &mut VisualTestContext) {
    view.read_with(cx, |view, _| view.terminal().unwrap().input(bytes));
}

fn task(label: &str, cwd: &std::path::Path) -> Task {
    Task {
        label: label.to_owned(),
        command: TaskCommand::Shell("true".to_owned()),
        cwd: cwd.to_owned(),
        env: BTreeMap::new(),
        group: None,
        default: false,
        source: Source::File,
        problem_matcher: None,
    }
}

#[gpui::test]
fn each_session_is_a_tab_in_the_panels_strip(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx, true);
    run("terminal.toggle", cx);
    assert_eq!(tabs(&workbench, cx), ["Terminal 1"]);
    run("terminal.new", cx);
    run("terminal.new", cx);
    assert_eq!(
        tabs(&workbench, cx),
        ["Terminal 1", "Terminal 2", "Terminal 3"]
    );

    // The tabs are side by side along the panel's top, above the session,
    // which has the panel's whole width: no list beside it.
    let strip = terminal(&workbench, cx).read_with(cx, |terminal, _| terminal.strip_bounds());
    let session = cx
        .debug_bounds("terminal-session")
        .expect("the session is drawn");
    let column = (workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone()))
        .read_with(cx, |dock, _| dock.dock().drawn().column_bounds())
        .expect("the bottom panel shows");
    let bounds: Vec<_> = (0..3).map(|ix| tab_bounds(&workbench, ix, cx)).collect();
    for pair in bounds.windows(2) {
        assert_eq!(pair[0].top(), pair[1].top());
        assert!(pair[1].left() >= pair[0].right() - px(0.5), "{pair:?}");
    }
    assert_eq!(bounds[0].top(), strip.top());
    assert!(strip.bottom() <= session.top(), "{strip:?} {session:?}");
    assert_eq!(session.left(), column.left());
    assert_eq!(session.size.width, column.size.width);

    // A click on a tab shows its session.
    cx.simulate_click(bounds[0].center(), Modifiers::none());
    draw(cx);
    assert_eq!(terminal(&workbench, cx).read_with(cx, |t, _| t.active()), 0);
}

/// A shell that marks a command running and stops, then on each line typed
/// ends it with 1, starts another, and ends that with 0.
const COMMANDS: &str = concat!(
    r"printf '\033]133;A\007$ \033]133;B\007make\r\n\033]133;C\007'; read a; ",
    r"printf '\033]133;D;1\007\033]133;A\007$ '; read b; printf '\033]133;C\007'; read c; ",
    r"printf '\033]133;D;0\007\033]133;A\007$ '; exec cat",
);

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn shell_marks_show_a_spinner_then_the_exit(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx, false);
    terminal(&workbench, cx).update(cx, |terminal, _| {
        terminal.set_shell("/bin/sh", &["-c", COMMANDS])
    });
    run("terminal.toggle", cx);
    let view = views(&workbench, cx).remove(0);

    // C with no D: the spinner.
    until(cx, "the spinner", |cx| {
        mark("Terminal 1", cx) == "loader-circle"
    });
    // D with 1: the error mark, which stays at the next prompt.
    type_in(&view, b"\r", cx);
    until(cx, "the error mark", |cx| {
        mark("Terminal 1", cx) == "circle-x"
    });
    // The next command runs: the spinner again.
    type_in(&view, b"\r", cx);
    until(cx, "the spinner again", |cx| {
        mark("Terminal 1", cx) == "loader-circle"
    });
    // D with 0: neither, the terminal icon.
    type_in(&view, b"\r", cx);
    until(cx, "the terminal icon", |cx| {
        mark("Terminal 1", cx) == "file-terminal"
    });
}

/// A session behind the active one isn't drawn: its tab follows its
/// command all the same.
#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn a_tab_behind_the_active_one_follows_its_command(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx, false);
    let terminal = terminal(&workbench, cx);
    terminal.update(cx, |terminal, _| {
        terminal.set_shell("/bin/sh", &["-c", COMMANDS])
    });
    run("terminal.toggle", cx);
    let first = views(&workbench, cx).remove(0);
    until(cx, "the spinner", |cx| {
        mark("Terminal 1", cx) == "loader-circle"
    });

    terminal.update(cx, |terminal, _| {
        terminal.set_shell("/bin/sh", &["-c", "exec cat"])
    });
    run("terminal.new", cx);
    assert_eq!(terminal.read_with(cx, |terminal, _| terminal.active()), 1);
    type_in(&first, b"\r", cx);
    until(cx, "the error mark", |cx| {
        mark("Terminal 1", cx) == "circle-x"
    });
    type_in(&first, b"\r", cx);
    until(cx, "the spinner again", |cx| {
        mark("Terminal 1", cx) == "loader-circle"
    });
    assert_eq!(mark("Terminal 2", cx), "file-terminal");
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn a_shell_without_marks_shows_the_terminal_icon(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx, false);
    terminal(&workbench, cx).update(cx, |terminal, _| {
        terminal.set_shell("/bin/sh", &["-c", "printf ready; exec cat"])
    });
    run("terminal.toggle", cx);
    let view = views(&workbench, cx).remove(0);
    until(cx, "the shell's output", |cx| {
        view.read_with(cx, |view, _| {
            (view.terminal()).is_some_and(|t| t.snapshot().text().contains("ready"))
        })
    });
    assert_eq!(mark("Terminal 1", cx), "file-terminal");
}

#[gpui::test]
fn task_tabs_show_a_spinner_then_how_they_ended(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx, true);
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    for label in ["build", "lint"] {
        let task = task(label, dir.path());
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| workbench.run_task(task, window, cx))
        });
        draw(cx);
    }
    assert_eq!(tabs(&workbench, cx), ["build", "lint"]);
    assert_eq!(mark("build", cx), "loader-circle");
    assert_eq!(mark("lint", cx), "loader-circle");

    let [build, lint] = [0, 1].map(|ix| views(&workbench, cx)[ix].clone());
    build.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(2))));
    draw(cx);
    assert_eq!(mark("build", cx), "circle-x");
    lint.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(0))));
    draw(cx);
    // Passed, its name has the check mark, and the tab no other.
    assert_eq!(tabs(&workbench, cx), ["build", "lint \u{2713}"]);
    assert_eq!(mark("lint \u{2713}", cx), "file-terminal");
}

#[gpui::test]
fn the_active_tab_has_the_accent_line(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx, true);
    run("terminal.toggle", cx);
    run("terminal.new", cx);
    let panel = terminal(&workbench, cx).read_with(cx, |terminal, cx| terminal.focus_handle(cx));
    assert!(cx.update(|window, cx| panel.contains_focused(window, cx)));

    // Along the active tab's top, 2 px, its width; on no other.
    let tab = tab_bounds(&workbench, 1, cx);
    let line = accent("Terminal 2", cx).expect("the active tab has the accent line");
    assert_eq!(line.origin, tab.origin);
    // The tab's right border is its own.
    assert!(
        (tab.size.width - line.size.width).abs() <= px(1.),
        "{line:?} {tab:?}"
    );
    assert_eq!(line.size.height, px(2.));
    assert_eq!(accent("Terminal 1", cx), None);

    // It follows the active tab.
    run("terminal.next", cx);
    assert!(accent("Terminal 1", cx).is_some());
    assert_eq!(accent("Terminal 2", cx), None);
}
