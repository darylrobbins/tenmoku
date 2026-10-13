//! Shell integration in the terminal panel: `terminal.new` starting where
//! the active shell said it was (OSC 7), and `terminal.previous_prompt` and
//! `terminal.next_prompt` scrolling between the prompts it marked (OSC
//! 133).

use std::{
    fs,
    path::{Path, PathBuf},
    thread,
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
        file: Some((path, Some(Buffer::new("fn a() {}\n")))),
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

/// Runs command `id` with the contexts `frames` focused, as a key bound
/// there or the palette would.
fn run_in(id: &str, frames: &[&str], cx: &mut VisualTestContext) {
    let context: ContextStack = frames
        .iter()
        .map(|&frame| ContextFrame::new(frame))
        .collect();
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Runs `id` from the focused terminal.
fn run_in_terminal(id: &str, cx: &mut VisualTestContext) {
    run_in(id, &["workbench", fe_terminal_view::KEY_CONTEXT], cx);
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

fn active_view(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalView> {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, _| terminal.view().unwrap().clone())
}

/// Shows the terminal and waits for its first shell to print `text`.
fn shown(workbench: &Entity<Workbench>, text: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("ctrl-`");
    printed(workbench, text, cx);
}

/// Waits for the active session's shell to print `text`.
fn printed(workbench: &Entity<Workbench>, text: &str, cx: &mut VisualTestContext) {
    let view = active_view(workbench, cx);
    until(cx, text, |cx| {
        view.read_with(cx, |view, _| {
            (view.terminal()).is_some_and(|t| t.snapshot().text().contains(text))
        })
    });
}

/// What the active session shows: its top row and how far back it's
/// scrolled.
fn view_top(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (String, usize) {
    let view = active_view(workbench, cx);
    view.read_with(cx, |view, _| {
        let snapshot = view.terminal().unwrap().snapshot();
        (snapshot.row_text(0), snapshot.display_offset)
    })
}

/// A script that appends its directory to `log`, then reports `reported`
/// with OSC 7, if any, and waits.
fn reporting(log: &Path, reported: Option<&Path>) -> String {
    let log = log.display();
    let osc = reported.map_or_else(String::new, |dir| {
        // Percent-encoded, as shells do.
        let url = format!("file://host{}", dir.display()).replace(' ', "%20");
        format!(r"printf '\033]7;%s\007' '{url}'; ")
    });
    format!(r"pwd -P >> '{log}'; {osc}printf 'reported\n'; read x")
}

/// The directories the sessions started in, as they logged them.
fn logged(log: &Path, sessions: usize, cx: &mut VisualTestContext) -> Vec<PathBuf> {
    let read = || fs::read_to_string(log).unwrap_or_default();
    until(cx, "the sessions to log", |_| {
        read().lines().count() >= sessions
    });
    read().lines().map(PathBuf::from).collect()
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap()
}

#[gpui::test]
fn new_starts_in_the_active_shells_directory(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().unwrap();
    let deep = scratch.path().join("src/deep dir");
    fs::create_dir_all(&deep).unwrap();
    let log = scratch.path().join("log");
    let script = reporting(&log, Some(&deep));
    let (workbench, cx, dir) = opened(cx, &script);
    shown(&workbench, "reported", cx);

    run_in("terminal.new", &["workbench"], cx);
    printed(&workbench, "reported", cx);
    let started = logged(&log, 2, cx);
    assert_eq!(started, [canonical(dir.path()), canonical(&deep)]);
}

#[gpui::test]
fn new_starts_in_the_panels_directory_without_a_report(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().unwrap();
    let log = scratch.path().join("log");
    let (workbench, cx, dir) = opened(cx, &reporting(&log, None));
    shown(&workbench, "reported", cx);

    run_in("terminal.new", &["workbench"], cx);
    let started = logged(&log, 2, cx);
    assert_eq!(started, [canonical(dir.path()), canonical(dir.path())]);
}

#[gpui::test]
fn new_starts_in_the_panels_directory_when_the_reported_one_is_gone(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().unwrap();
    let gone = scratch.path().join("gone");
    fs::create_dir(&gone).unwrap();
    let log = scratch.path().join("log");
    let (workbench, cx, dir) = opened(cx, &reporting(&log, Some(&gone)));
    shown(&workbench, "reported", cx);
    fs::remove_dir(&gone).unwrap();

    run_in("terminal.new", &["workbench"], cx);
    let started = logged(&log, 2, cx);
    assert_eq!(started, [canonical(dir.path()), canonical(dir.path())]);
}

#[test]
fn prompt_commands_are_in_the_palette_in_the_terminal() {
    let list = crate::command_list();
    for (id, title) in [
        ("terminal.previous_prompt", "Terminal: Previous Prompt"),
        ("terminal.next_prompt", "Terminal: Next Prompt"),
    ] {
        let command = list.iter().find(|command| command.id.as_str() == id);
        let command = command.unwrap_or_else(|| panic!("{id} is listed"));
        assert_eq!(command.title, title);
        let terminal = ContextPredicate::identifier(fe_terminal_view::KEY_CONTEXT);
        assert_eq!(command.context, Some(terminal), "{id}");
    }
}

/// Three marked prompts, each followed by 100 lines, then a marked prompt
/// waiting for input.
const PROMPTS: &str = r"for i in 1 2 3; do
        printf '\033]133;A\007$ prompt %d\n' $i
        j=1; while [ $j -le 100 ]; do echo out $i $j; j=$((j+1)); done
    done; printf '\033]133;A\007$ '; read x";

#[gpui::test]
fn prompt_commands_scroll_between_the_prompts(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx, PROMPTS);
    shown(&workbench, "out 3 100", cx);
    assert_eq!(view_top(&workbench, cx).1, 0);

    // Back from the current prompt, each to the top of the view.
    run_in_terminal("terminal.previous_prompt", cx);
    assert_eq!(view_top(&workbench, cx).0, "$ prompt 3");
    run_in_terminal("terminal.previous_prompt", cx);
    assert_eq!(view_top(&workbench, cx).0, "$ prompt 2");
    run_in_terminal("terminal.previous_prompt", cx);
    assert_eq!(view_top(&workbench, cx).0, "$ prompt 1");
    run_in_terminal("terminal.previous_prompt", cx);
    assert_eq!(view_top(&workbench, cx).0, "$ prompt 1");

    // Forward again, and past the current prompt to the bottom.
    run_in_terminal("terminal.next_prompt", cx);
    assert_eq!(view_top(&workbench, cx).0, "$ prompt 2");
    run_in_terminal("terminal.next_prompt", cx);
    assert_eq!(view_top(&workbench, cx).0, "$ prompt 3");
    run_in_terminal("terminal.next_prompt", cx);
    run_in_terminal("terminal.next_prompt", cx);
    assert_eq!(view_top(&workbench, cx).1, 0);
}

#[gpui::test]
fn prompt_commands_do_nothing_without_marks(cx: &mut TestAppContext) {
    let script = "i=1; while [ $i -le 300 ]; do echo line $i; i=$((i+1)); done; read x";
    let (workbench, cx, _dir) = opened(cx, script);
    shown(&workbench, "line 300", cx);
    let view = active_view(&workbench, cx);
    view.read_with(cx, |view, _| view.terminal().unwrap().scroll(-5));
    let before = view_top(&workbench, cx);
    assert_eq!(before.1, 5);

    run_in_terminal("terminal.previous_prompt", cx);
    assert_eq!(view_top(&workbench, cx), before);
    run_in_terminal("terminal.next_prompt", cx);
    assert_eq!(view_top(&workbench, cx), before);
}
