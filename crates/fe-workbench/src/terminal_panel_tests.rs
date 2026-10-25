//! The terminal on the bottom dock: ctrl-` cycling focus, the session's
//! directory and lifetime, and the keys left to the shell.

use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, Keymap, KeymapLayer, Keystroke,
};
use fe_terminal_view::TerminalView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, TerminalPanel, Workbench, tests::open};

/// A workbench showing `a.rs` from a temporary project, focused as the app
/// focuses it, whose terminal runs `/bin/sh`.
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

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

fn dock_shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.read_with(cx, |dock, _| dock.is_visible())
}

fn session(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<TerminalView>> {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, _| terminal.view().cloned())
}

fn terminal_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let view = session(workbench, cx).expect("a session runs");
    cx.update(|window, cx| view.focus_handle(cx).is_focused(window))
}

fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows");
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn toggle(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("ctrl-`");
    draw(cx);
}

/// The session's shell's pid, once it has started.
fn started(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> u32 {
    until(cx, "the shell to start", |cx| {
        view.read_with(cx, |view, _| view.terminal().is_some())
    });
    view.read_with(cx, |view, _| view.terminal().unwrap().pid())
}

fn type_line(view: &Entity<TerminalView>, line: &str, cx: &mut VisualTestContext) {
    let bytes = format!("{line}\n").into_bytes();
    view.read_with(cx, |view, _| view.terminal().unwrap().input(bytes));
}

fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.terminal().unwrap().snapshot().text())
}

#[gpui::test]
fn toggle_cycles_focus(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    assert!(!dock_shown(&workbench, cx), "hidden at launch");
    assert!(
        session(&workbench, cx).is_none(),
        "nothing runs until shown"
    );
    assert!(editor_focused(&workbench, cx));

    // From the editor: the dock shows, with the terminal focused.
    toggle(cx);
    assert!(dock_shown(&workbench, cx));
    assert!(terminal_focused(&workbench, cx));

    // Shown but not focused: the terminal takes focus, the dock stays.
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    assert!(editor_focused(&workbench, cx));
    toggle(cx);
    assert!(dock_shown(&workbench, cx));
    assert!(terminal_focused(&workbench, cx));

    // Focused: the dock hides and the active editor has focus.
    toggle(cx);
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));
}

#[gpui::test]
fn starts_in_the_project_root(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    toggle(cx);
    let view = session(&workbench, cx).expect("the first show starts a session");
    let pid = started(&view, cx);
    let started_count = |cx: &mut VisualTestContext| {
        terminal(&workbench, cx).read_with(cx, |terminal, _| terminal.started)
    };
    assert_eq!(started_count(cx), 1);

    type_line(&view, "pwd", cx);
    let root = dir.path().to_owned();
    let canonical = root.canonicalize().unwrap();
    let in_root = |text: &str| {
        text.lines().any(|line| {
            Path::new(line.trim_end()) == root || Path::new(line.trim_end()) == canonical
        })
    };
    until(cx, "pwd to print the root", |cx| {
        in_root(&screen(&view, cx))
    });

    // Hidden and shown again: the same session.
    toggle(cx);
    assert!(!dock_shown(&workbench, cx));
    toggle(cx);
    assert!(dock_shown(&workbench, cx));
    let again = session(&workbench, cx).expect("the session stays");
    assert_eq!(again.entity_id(), view.entity_id());
    assert_eq!(started(&again, cx), pid);
    assert_eq!(started_count(cx), 1);
}

#[gpui::test]
fn exit_hides_the_panel(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    let view = session(&workbench, cx).unwrap();
    let pid = started(&view, cx);
    assert!(terminal_focused(&workbench, cx));

    type_line(&view, "exit", cx);
    until(cx, "the shell to exit", |cx| {
        session(&workbench, cx).is_none()
    });
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));

    // The next toggle starts a new session.
    toggle(cx);
    assert!(dock_shown(&workbench, cx));
    let next = session(&workbench, cx).expect("a new session");
    assert_ne!(next.entity_id(), view.entity_id());
    assert_ne!(started(&next, cx), pid);
    assert!(terminal_focused(&workbench, cx));
    let count = terminal(&workbench, cx).read_with(cx, |terminal, _| terminal.started);
    assert_eq!(count, 2);

    // An exit while another panel has focus hides the dock all the same,
    // and the editor takes focus.
    let next_pid = started(&next, cx);
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    draw(cx);
    assert!(cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)));
    type_line(&next, "exit", cx);
    until(cx, "the shell to exit", |cx| {
        session(&workbench, cx).is_none()
    });
    assert_ne!(next_pid, pid);
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));
}

fn keymap(source: &str) -> Keymap {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), source, None);
    assert_eq!(diagnostics, []);
    Keymap::from_layers([layer.expect("the default keymap loads")])
}

/// What `keys` runs with `focused`'s context focused in the workbench.
fn run(keymap: &Keymap, keys: &str, focused: &str) -> Option<String> {
    let stack: ContextStack = [ContextFrame::new("workbench"), ContextFrame::new(focused)]
        .into_iter()
        .collect();
    let keystroke: Keystroke = keys.parse().unwrap();
    keymap
        .lookup(&keystroke, &stack)
        .map(|invocation| invocation.command.as_str().to_owned())
}

#[test]
fn terminal_keys() {
    let macos = keymap(DEFAULT_MACOS);
    let terminal = fe_terminal_view::KEY_CONTEXT;
    for (keys, command) in [
        ("f12", "nav.goto_definition"),
        ("shift-f12", "editor.peek_references"),
        ("shift-alt-f12", "nav.find_references"),
        ("ctrl--", "nav.back"),
        ("ctrl-shift--", "nav.forward"),
    ] {
        assert_eq!(
            run(&macos, keys, terminal),
            None,
            "{keys} reaches the shell"
        );
        let editor = fe_gpui::KEY_CONTEXT;
        assert_eq!(
            run(&macos, keys, editor).as_deref(),
            Some(command),
            "{keys}"
        );
    }
    for source in [DEFAULT_MACOS, DEFAULT_LINUX] {
        let keymap = keymap(source);
        for focused in [fe_gpui::KEY_CONTEXT, terminal] {
            let toggle = run(&keymap, "ctrl-`", focused);
            assert_eq!(toggle.as_deref(), Some("terminal.toggle"), "in {focused}");
        }
    }
}

#[gpui::test]
fn toggle_is_in_the_palette(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let id = fe_command::CommandId::parse("terminal.toggle").unwrap();
    let listed = cx
        .read(|cx| kiln::platform::command_actions::typed_command(&id, cx))
        .is_some();
    assert!(listed);
}
