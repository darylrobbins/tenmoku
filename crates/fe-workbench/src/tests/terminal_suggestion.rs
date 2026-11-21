//! The terminal's history suggestion (`docs/design/design-system.md#terminal`,
//! T1596): at the live prompt, with the cursor at the end of the typed
//! command and no program running, the rest of the newest command any
//! terminal ran that starts with it shows faint after the cursor, painted
//! over the grid; Right or End sends it as input. Not in the alternate
//! screen, in vi mode, or with `terminal.history_suggestions` off.

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::TerminalSettings;
use fe_gpui::commands::RunCommand;
use fe_terminal_view::TerminalView;
use fe_ui::ActiveTheme as _;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::tests::open;
use crate::{Launch, TerminalPanel, Workbench};

fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "a\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
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

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

const PROMPT: &str = "\x1b]133;A\x07$ \x1b]133;B\x07";

/// A finished command: its prompt, `line`, and an end with code 0.
fn command(line: &str) -> String {
    format!("{PROMPT}{line}\r\n\x1b]133;C\x07out\r\n\x1b]133;D;0\x07")
}

/// Opens a session that writes `bytes`, waits until `ready` says the
/// terminal has taken them all in, and records what each frame paints.
fn typed(
    workbench: &Entity<Workbench>,
    typed: &str,
    cx: &mut VisualTestContext,
) -> Entity<TerminalView> {
    let want = typed.to_owned();
    let ready = move |t: &fe_terminal_view::Terminal| t.typed_command().as_deref() == Some(&want);
    session(workbench, &typing(typed), ready, cx)
}

fn session(
    workbench: &Entity<Workbench>,
    bytes: &str,
    ready: impl Fn(&fe_terminal_view::Terminal) -> bool,
    cx: &mut VisualTestContext,
) -> Entity<TerminalView> {
    let panel = terminal(workbench, cx);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.open_with_session(bytes, window, cx))
    });
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let toggle = RunCommand(Invocation::of("terminal.toggle").unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(toggle), cx));
    draw(cx);
    let view = panel.read_with(cx, |panel, _| panel.view().unwrap().clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        let done = view.read_with(cx, |view, _| view.terminal().is_some_and(&ready));
        if done {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the session"
        );
        thread::sleep(Duration::from_millis(10));
    }
    view.update(cx, |view, _| view.record_paint(true));
    draw(cx);
    view
}

/// The suggestion the last frame painted, with its colour.
fn suggestion(
    view: &Entity<TerminalView>,
    cx: &mut VisualTestContext,
) -> Option<(String, gpui::Hsla)> {
    let report = view.read_with(cx, |view, _| view.painted().clone());
    report.suggestion.map(|(text, color)| (text.text, color))
}

/// The screen's last row with text: the live prompt.
fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    let text = view.read_with(cx, |view, _| view.terminal().unwrap().snapshot().text());
    text.trim_end()
        .lines()
        .last()
        .unwrap_or_default()
        .to_owned()
}

fn faint(cx: &mut VisualTestContext) -> gpui::Hsla {
    cx.update(|_, cx| cx.theme().colors.text.faint)
}

/// Waits, drawing, until the frame paints (`want`) a suggestion or not.
fn settle(view: &Entity<TerminalView>, want: bool, cx: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        if suggestion(view, cx).is_some() == want {
            return;
        }
        assert!(Instant::now() < deadline, "no suggestion change: {want}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// An earlier command, then a live prompt with `typed` after it.
fn typing(typed: &str) -> String {
    format!(
        "{}{}\x1b]133;A\x07$ \x1b]133;B\x07{typed}",
        command("git status"),
        command("ls")
    )
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn the_remainder_shows_faint_after_the_cursor(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let view = typed(&workbench, "git s", cx);
    let (text, color) = suggestion(&view, cx).expect("a suggestion");
    assert_eq!(text, "tatus");
    assert_eq!(color, faint(cx));

    // Drawn at the cursor, not as cells on the grid.
    let report = view.read_with(cx, |view, _| view.painted().clone());
    let origin = report.suggestion.unwrap().0.origin;
    let fe_terminal_view::PaintedCursor::Block(cursor) = report.cursor.expect("a cursor") else {
        panic!("a block cursor");
    };
    assert_eq!(origin, cursor.origin);
    assert!(!screen(&view, cx).contains("tatus"));
    assert!(report.text.iter().all(|text| !text.text.contains("tatus")));

    // Nothing that matches, or nothing longer, or nothing typed: none.
    let view = typed(&workbench, "zzz", cx);
    assert_eq!(suggestion(&view, cx), None);
    let view = typed(&workbench, "ls", cx);
    assert_eq!(suggestion(&view, cx), None);
    let view = typed(&workbench, "", cx);
    assert_eq!(suggestion(&view, cx), None);
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn right_and_end_at_the_end_of_the_line_send_the_remainder(cx: &mut TestAppContext) {
    for key in ["right", "end"] {
        let (workbench, cx, _dir) = opened_again(cx);
        let view = typed(&workbench, "git s", cx);
        assert!(suggestion(&view, cx).is_some());
        cx.update(|window, cx| window.focus(&view.read(cx).focus_handle(cx), cx));
        assert!(!screen(&view, cx).contains("tatus"), "{key}");
        cx.simulate_keystrokes(key);
        let deadline = Instant::now() + Duration::from_secs(5);
        while screen(&view, cx).trim_end() != "$ git status" {
            assert!(Instant::now() < deadline, "{key}: {}", screen(&view, cx));
            thread::sleep(Duration::from_millis(10));
            draw(cx);
        }
    }
}

fn opened_again(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    opened(cx)
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn nothing_shows_in_the_alternate_screen_in_vi_mode_or_while_a_program_runs(
    cx: &mut TestAppContext,
) {
    // A program running: `C` came, `D` didn't.
    let (workbench, cx, _dir) = opened(cx);
    let running = format!("{}{PROMPT}git s\r\n\x1b]133;C\x07", command("git status"));
    let view = session(
        &workbench,
        &running,
        |t| t.command_status() == fe_terminal_view::CommandStatus::Running,
        cx,
    );
    assert_eq!(suggestion(&view, cx), None);

    // The alternate screen.
    let alt = format!("{}\x1b[?1049h", typing("git s"));
    let view = session(&workbench, &alt, |t| t.snapshot().modes.alt_screen, cx);
    assert_eq!(suggestion(&view, cx), None);

    // Vi mode, which keeps the cursor elsewhere.
    let view = typed(&workbench, "git s", cx);
    assert!(suggestion(&view, cx).is_some());
    view.update(cx, |view, cx| view.toggle_vi_mode(cx));
    settle(&view, false, cx);
    view.update(cx, |view, cx| view.toggle_vi_mode(cx));
    settle(&view, true, cx);

    // A shell that doesn't mark prompts.
    let view = session(
        &workbench,
        "git s",
        |t| t.snapshot().text().contains("git s"),
        cx,
    );
    assert_eq!(suggestion(&view, cx), None);
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn the_setting_off_hides_it(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let view = typed(&workbench, "git s", cx);
    assert!(suggestion(&view, cx).is_some());
    for on in [false, true] {
        let settings = TerminalSettings {
            history_suggestions: on,
            ..TerminalSettings::default()
        };
        workbench.update(cx, |workbench, cx| {
            workbench.set_terminal_settings(&settings, cx)
        });
        settle(&view, on, cx);
    }
}
