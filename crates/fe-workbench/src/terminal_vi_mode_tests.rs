//! `terminal.vi_mode`: the palette command that turns the active terminal's
//! vi copy mode on and off.

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextPredicate, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_terminal_view::TerminalView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, TerminalPanel, Workbench, tests::open};

/// A workbench showing `a.rs` from a temporary project, focused as the app
/// focuses it, whose terminals run `/bin/sh -c script`.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    script: &str,
) -> (
    Entity<Workbench>,
    &'a mut VisualTestContext,
    tempfile::TempDir,
) {
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
    terminal(&workbench, cx).update(cx, |terminal, _| {
        terminal.set_shell("/bin/sh", &["-c", script])
    });
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

/// Runs `id` from the focused terminal, as a key bound there or the
/// palette would.
fn run_in_terminal(id: &str, cx: &mut VisualTestContext) {
    let context: ContextStack = ["workbench", fe_terminal_view::KEY_CONTEXT]
        .into_iter()
        .map(ContextFrame::new)
        .collect();
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

fn active_view(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalView> {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, _| terminal.view().unwrap().clone())
}

fn vi_mode(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.is_vi_mode())
}

#[gpui::test]
fn vi_mode_is_in_the_palette_in_the_terminal(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let id = fe_command::CommandId::parse("terminal.vi_mode").unwrap();
    let command = cx
        .read(|cx| kiln::platform::command_actions::typed_command(&id, cx))
        .expect("terminal.vi_mode is listed");
    assert_eq!(command.title, "Terminal: Vi Copy Mode");
    let terminal = ContextPredicate::identifier(fe_terminal_view::KEY_CONTEXT);
    assert_eq!(command.context, Some(terminal));
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn vi_mode_toggles_vi_mode_in_the_active_session(cx: &mut TestAppContext) {
    let script = "i=1; while [ $i -le 100 ]; do echo line $i; i=$((i+1)); done; read x";
    let (workbench, cx, _dir) = opened(cx, script);
    cx.simulate_keystrokes("ctrl-`");
    let first = active_view(&workbench, cx);
    until(cx, "line 100", |cx| {
        first.read_with(cx, |view, _| {
            (view.terminal()).is_some_and(|t| t.snapshot().text().contains("line 100"))
        })
    });

    run_in_terminal("terminal.vi_mode", cx);
    assert!(vi_mode(&first, cx));
    let focused = cx.update(|window, cx| first.focus_handle(cx).is_focused(window));
    assert!(focused, "the session is focused");
    // Its keys are vi mode's now: gg goes to the top of history.
    cx.simulate_keystrokes("g g");
    let offset = first.read_with(cx, |view, _| {
        view.terminal().unwrap().snapshot().display_offset
    });
    assert!(offset > 0, "{offset}");

    // Only the active session's.
    run_in_terminal("terminal.new", cx);
    let second = active_view(&workbench, cx);
    assert_ne!(first, second);
    until(cx, "the second session", |cx| {
        second.read_with(cx, |view, _| view.terminal().is_some())
    });
    run_in_terminal("terminal.vi_mode", cx);
    assert!(vi_mode(&second, cx));
    run_in_terminal("terminal.vi_mode", cx);
    assert!(!vi_mode(&second, cx));
    assert!(vi_mode(&first, cx));
}

/// With Linux's keymap, which binds ctrl-b (`dock.toggle`) and other keys
/// vi mode reads, vi mode gets them while it's on, and the keymap after.
#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn vi_mode_keys_win_over_the_keymap(cx: &mut TestAppContext) {
    let script = "i=1; while [ $i -le 100 ]; do echo line $i; i=$((i+1)); done; read x";
    let (workbench, cx, _dir) = opened(cx, script);
    cx.update(|_, cx| {
        let path = std::path::Path::new(fe_keymap::DEFAULT_KEYMAP_PATH);
        let (layer, _) = fe_keymap::KeymapLayer::parse(path, fe_keymap::DEFAULT_LINUX, None);
        fe_gpui::set_user_keymap(cx, fe_keymap::Keymap::from_layers([layer.unwrap()]));
    });
    run_in_terminal("terminal.toggle", cx);
    let view = active_view(&workbench, cx);
    until(cx, "line 100", |cx| {
        view.read_with(cx, |view, _| {
            (view.terminal()).is_some_and(|t| t.snapshot().text().contains("line 100"))
        })
    });
    let offset = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.terminal().unwrap().snapshot().display_offset
        })
    };
    let dock = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| workbench.dock().read(cx).is_visible())
    };

    run_in_terminal("terminal.vi_mode", cx);
    assert!(vi_mode(&view, cx));
    let shown = dock(cx);
    cx.simulate_keystrokes("g g");
    draw(cx);
    let top = offset(cx);
    assert!(top > 20, "{top}");
    cx.simulate_keystrokes("ctrl-f");
    draw(cx);
    let paged = offset(cx);
    assert!(paged < top, "{paged} < {top}");
    cx.simulate_keystrokes("ctrl-b");
    draw(cx);
    assert_eq!(offset(cx), top);
    cx.simulate_keystrokes("ctrl-d ctrl-u ctrl-e ctrl-y ctrl-v escape");
    draw(cx);
    assert_eq!(offset(cx), top);
    assert!(vi_mode(&view, cx));
    assert_eq!(dock(cx), shown, "ctrl-b left the dock alone");

    // Out of vi mode ctrl-b is the keymap's again.
    cx.simulate_keystrokes("q");
    draw(cx);
    assert!(!vi_mode(&view, cx));
    cx.simulate_keystrokes("ctrl-b");
    draw(cx);
    assert_eq!(dock(cx), !shown, "ctrl-b toggles the dock");
}
