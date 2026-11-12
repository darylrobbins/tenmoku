//! Running a project's tasks on the terminal panel. Sessions here are fake:
//! they stay out of the frame, so no process starts, and each test emits the
//! exit it wants.

use std::{collections::BTreeMap, fs, path::Path};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_terminal_view::{TerminalView, TerminalViewEvent};
use gpui::{Entity, Focusable as _, SharedString, TestAppContext, VisualTestContext};
use kiln_shell::DockView as _;

use crate::{
    Launch, TerminalPanel, Workbench,
    task_defs::{Source, Task, TaskCommand},
    task_run::untrusted_message,
    terminal_panel::task_options,
    tests::open,
    workbench::toast::{Choices, Level},
};

const LABEL: &str = "cargo test";

/// A workbench showing a text file from a temporary project (no language
/// server for it, trusted or not), focused as the app focuses it, whose
/// terminal panel starts no process.
fn untrusted(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "a\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

/// As [`untrusted`], with the root trusted.
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let (workbench, cx, dir) = untrusted(cx);
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    assert!(workbench.read_with(cx, |workbench, _| workbench.root_trusted()));
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn task(label: &str, command: TaskCommand, cwd: &Path) -> Task {
    Task {
        label: label.to_owned(),
        command,
        cwd: cwd.to_owned(),
        env: BTreeMap::new(),
        group: None,
        default: false,
        source: Source::File,
        problem_matcher: None,
    }
}

fn shell_task(label: &str, cwd: &Path) -> Task {
    task(label, TaskCommand::Shell("true".to_owned()), cwd)
}

fn run_task(workbench: &Entity<Workbench>, task: Task, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.run_task(task, window, cx));
    });
    draw(cx);
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

fn views(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<TerminalView>> {
    terminal(workbench, cx).read_with(cx, |terminal, _| terminal.views().cloned().collect())
}

/// The tabs the last frame listed.
fn labels(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<SharedString> {
    draw(cx);
    terminal(workbench, cx).read_with(cx, |terminal, _| {
        (terminal.drawn_tabs().iter())
            .map(|(label, _)| label.clone())
            .collect()
    })
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    terminal(workbench, cx).read_with(cx, |terminal, _| terminal.active())
}

fn dock_shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.read_with(cx, |dock, _| dock.is_visible())
}

fn focused(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.focus_handle(cx).is_focused(window))
}

fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows");
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// Ends `view`'s fake session.
fn exit(view: &Entity<TerminalView>, code: Option<i32>, cx: &mut VisualTestContext) {
    view.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(code)));
    draw(cx);
}

/// Each toast showing: level, message and its choices' titles.
fn toasts(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(Level, String, Vec<String>)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let toast = &entry.toast;
                let titles = toast.choices.as_ref().map(Choices::titles);
                let titles = titles.unwrap_or_default().to_vec();
                (toast.level, toast.message.clone(), titles)
            })
            .collect()
    })
}

/// The choices of the toast showing `message`.
fn choices(workbench: &Entity<Workbench>, message: &str, cx: &mut VisualTestContext) -> Choices {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .find(|entry| entry.toast.message == message)
            .and_then(|entry| entry.toast.choices.clone())
            .expect("a toast with choices")
    })
}

fn show_of(message: &str) -> (Level, String, Vec<String>) {
    (Level::Error, message.to_owned(), vec!["Show".to_owned()])
}

#[gpui::test]
fn shell_task_becomes_sh_c(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = task(
        LABEL,
        TaskCommand::Shell("cargo test -q".to_owned()),
        dir.path(),
    );
    shell.env.insert("RUST_LOG".to_owned(), "debug".to_owned());
    let options = task_options(&shell, 7).unwrap();
    #[cfg(not(windows))]
    let expected = (
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), "cargo test -q".to_owned()],
    );
    #[cfg(windows)]
    let expected = (
        format!(
            r"{}\System32\WindowsPowerShell\v1.0\powershell.exe",
            std::env::var("SystemRoot").unwrap()
        ),
        ["-NoLogo", "-NoProfile", "-Command", "cargo test -q"]
            .map(str::to_owned)
            .to_vec(),
    );
    assert_eq!(options.shell, Some(expected), "not the login shell");
    assert_eq!(options.cwd, dir.path());
    assert_eq!(options.scrollback, 7);
    assert_eq!(options.env, shell.env);

    let argv = ["cargo", "test", "-q"].map(str::to_owned).to_vec();
    let direct = task(LABEL, TaskCommand::Argv(argv), dir.path());
    let options = task_options(&direct, 7).unwrap();
    assert_eq!(
        options.shell,
        Some(("cargo".to_owned(), vec!["test".to_owned(), "-q".to_owned()])),
        "no shell"
    );
    assert!(options.env.is_empty());

    // The panel starts what these build.
    let (workbench, cx, root) = opened(cx);
    let mut asked = shell_task(LABEL, root.path());
    asked.env.insert("A".to_owned(), "b".to_owned());
    let expected = task_options(&asked, fe_config::TerminalSettings::default().scrollback);
    run_task(&workbench, asked, cx);
    let started =
        terminal(&workbench, cx).read_with(cx, |terminal, _| terminal.started_options[0].clone());
    assert_eq!(started, expected.unwrap());
}

#[gpui::test]
fn run_opens_a_tab_named_for_the_task(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));

    let task = shell_task(LABEL, dir.path());
    run_task(&workbench, task.clone(), cx);

    assert_eq!(views(&workbench, cx).len(), 1);
    assert_eq!(labels(&workbench, cx), [LABEL]);
    assert!(dock_shown(&workbench, cx));
    assert!(!editor_focused(&workbench, cx));
    let view = views(&workbench, cx).remove(0);
    assert!(focused(&view, cx), "as a new terminal is");
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    let title = dock.read_with(cx, |dock, cx| dock.dock().title(cx));
    assert_eq!(title.as_deref(), Some(LABEL));
    let last = workbench.read_with(cx, |workbench, cx| workbench.last_task(cx));
    assert_eq!(last, Some(task));
    assert_eq!(toasts(&workbench, cx), []);
}

#[gpui::test]
fn rerun_reuses_the_finished_tab(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    let task = shell_task(LABEL, dir.path());
    run_task(&workbench, task.clone(), cx);
    let first = views(&workbench, cx).remove(0);
    exit(&first, Some(0), cx);

    // Finished: the same tab, with a new session and no check mark yet.
    run_task(&workbench, task.clone(), cx);
    let again = views(&workbench, cx);
    assert_eq!(again.len(), 1);
    assert_ne!(again[0].entity_id(), first.entity_id());
    assert_eq!(labels(&workbench, cx), [LABEL]);
    assert!(focused(&again[0], cx));

    // The old session's end is no longer heard.
    exit(&first, Some(2), cx);
    assert_eq!(toasts(&workbench, cx), []);

    // Still running: new tabs, without asking.
    run_task(&workbench, task.clone(), cx);
    run_task(&workbench, task.clone(), cx);
    assert_eq!(
        labels(&workbench, cx),
        [LABEL, "cargo test (2)", "cargo test (3)"]
    );
    assert_eq!(active(&workbench, cx), 2);
    assert_eq!(toasts(&workbench, cx), []);

    // The middle one finishes: it's the one reused, under its own name.
    let second = views(&workbench, cx).remove(1);
    exit(&second, Some(0), cx);
    run_task(&workbench, task.clone(), cx);
    assert_eq!(views(&workbench, cx).len(), 3);
    assert_eq!(
        labels(&workbench, cx),
        [LABEL, "cargo test (2)", "cargo test (3)"]
    );
    assert_eq!(active(&workbench, cx), 1);

    // Another label is another tab.
    run_task(&workbench, shell_task("lint", dir.path()), cx);
    assert_eq!(views(&workbench, cx).len(), 4);

    // A closed tab's name is free again.
    terminal(&workbench, cx).update_in(cx, |terminal, window, cx| {
        terminal.activate(0, window, cx);
        terminal.close_active(window, cx);
    });
    draw(cx);
    run_task(&workbench, task, cx);
    assert_eq!(
        labels(&workbench, cx),
        ["cargo test (2)", "cargo test (3)", "lint", LABEL]
    );
}

#[gpui::test]
fn exit_code_is_reported(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);

    // Passed: a check mark, and nothing to dismiss.
    run_task(&workbench, shell_task(LABEL, dir.path()), cx);
    let passed = views(&workbench, cx).remove(0);
    exit(&passed, Some(0), cx);
    assert_eq!(labels(&workbench, cx), ["cargo test \u{2713}"]);
    assert_eq!(toasts(&workbench, cx), []);
    assert_eq!(views(&workbench, cx).len(), 1, "its output stays");

    // Failed: no check mark, and an error that can show the tab.
    run_task(&workbench, shell_task("lint", dir.path()), cx);
    let failed = views(&workbench, cx).remove(1);
    exit(&failed, Some(2), cx);
    assert_eq!(labels(&workbench, cx), ["cargo test \u{2713}", "lint"]);
    assert_eq!(toasts(&workbench, cx), [show_of("lint failed (exit 2)")]);
    exit(&failed, Some(2), cx);
    assert_eq!(toasts(&workbench, cx).len(), 1, "told once");

    // Killed.
    run_task(&workbench, shell_task("serve", dir.path()), cx);
    let killed = views(&workbench, cx).remove(2);
    exit(&killed, None, cx);
    assert_eq!(
        toasts(&workbench, cx),
        [show_of("lint failed (exit 2)"), show_of("serve was killed")]
    );
    assert_eq!(views(&workbench, cx).len(), 3);

    // Show, from a hidden dock with another tab active, shows and focuses it.
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.update(cx, |dock, cx| {
        dock.dock_mut().set_visible(false);
        cx.notify();
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(active(&workbench, cx), 2);
    choices(&workbench, "lint failed (exit 2)", cx).choose(0);
    draw(cx);
    assert!(dock_shown(&workbench, cx));
    assert_eq!(active(&workbench, cx), 1);
    assert!(focused(&failed, cx));
    assert!(!editor_focused(&workbench, cx));

    // Even after a rerun replaced the session it reported on.
    terminal(&workbench, cx).update_in(cx, |terminal, window, cx| {
        terminal.activate(0, window, cx);
    });
    run_task(&workbench, shell_task("serve", dir.path()), cx);
    assert_eq!(active(&workbench, cx), 2);
    choices(&workbench, "serve was killed", cx).choose(0);
    draw(cx);
    assert_eq!(active(&workbench, cx), 2);
    let rerun = views(&workbench, cx).remove(2);
    assert_ne!(rerun.entity_id(), killed.entity_id());
    assert!(focused(&rerun, cx));
}

#[gpui::test]
fn terminal_without_task_is_unchanged(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    let command = |id: &str| {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        RunCommand(Invocation::of(id).unwrap().context(context))
    };
    cx.update(|window, cx| window.dispatch_action(Box::new(command("terminal.new")), cx));
    draw(cx);

    let options = terminal(&workbench, cx).read_with(cx, |t, _| t.started_options[0].clone());
    assert_eq!(options.shell, None, "the login shell");
    assert_eq!(options.cwd, dir.path());
    assert!(options.env.is_empty());
    assert_eq!(labels(&workbench, cx), ["Terminal 1"]);

    // A task doesn't take a terminal's number, or change how its shell ends.
    run_task(&workbench, shell_task(LABEL, dir.path()), cx);
    cx.update(|window, cx| window.dispatch_action(Box::new(command("terminal.new")), cx));
    draw(cx);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", LABEL, "Terminal 2"]);
    let shell = views(&workbench, cx).remove(2);
    exit(&shell, Some(0), cx);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", LABEL]);
    assert_eq!(toasts(&workbench, cx), []);
}

#[gpui::test]
fn an_untrusted_root_runs_nothing(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = untrusted(cx);
    assert!(!workbench.read_with(cx, |workbench, _| workbench.root_trusted()));
    run_task(&workbench, shell_task(LABEL, dir.path()), cx);
    assert!(views(&workbench, cx).is_empty());
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));
    assert_eq!(workbench.read_with(cx, |w, cx| w.last_task(cx)), None);
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    let action = layer.read_with(cx, |layer, _| {
        let entry = &layer.queue().shown()[0];
        assert_eq!(entry.toast.message, untrusted_message());
        entry.toast.action.clone()
    });
    let (title, command) = action.expect("an offer to trust it");
    assert_eq!(title, "Trust Folder");
    assert_eq!(command.as_str(), "workbench.trust_folder");
}

#[gpui::test]
fn a_task_never_runs_anywhere_else(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    let gone = dir.path().join("gone");
    let missing = shell_task(LABEL, &gone);
    assert!(task_options(&missing, 7).is_err());
    run_task(&workbench, missing.clone(), cx);
    assert!(views(&workbench, cx).is_empty(), "not in home or /");
    assert!(!dock_shown(&workbench, cx));
    assert_eq!(toasts(&workbench, cx)[0].1, format!("Can't run {LABEL}"));
    let last = workbench.read_with(cx, |workbench, cx| workbench.last_task(cx));
    assert_eq!(last, Some(missing), "a rerun tries it again");

    // An empty argument list isn't the login shell.
    let empty = task(LABEL, TaskCommand::Argv(Vec::new()), dir.path());
    assert!(task_options(&empty, 7).is_err());
    run_task(&workbench, empty, cx);
    assert!(views(&workbench, cx).is_empty());
}

/// No process starts: the program isn't there.
#[gpui::test]
fn a_program_that_wont_start_leaves_no_tab(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.fake_sessions = false);
    let argv = vec!["/nonexistent/program".to_owned()];
    run_task(
        &workbench,
        task(LABEL, TaskCommand::Argv(argv), dir.path()),
        cx,
    );
    draw(cx);
    assert!(views(&workbench, cx).is_empty());
    assert_eq!(
        toasts(&workbench, cx),
        [(Level::Error, format!("Couldn't start {LABEL}"), Vec::new())]
    );
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));
}

#[gpui::test]
fn a_tabs_given_name_outlives_its_runs(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    run_task(&workbench, shell_task(LABEL, dir.path()), cx);
    let view = views(&workbench, cx).remove(0);
    let id = view.entity_id();
    let panel = terminal(&workbench, cx);
    panel.update(cx, |terminal, cx| terminal.set_name(id, "tests", cx));
    exit(&view, Some(0), cx);
    assert_eq!(labels(&workbench, cx), ["tests \u{2713}"]);
    let editing = panel.read_with(cx, |terminal, cx| terminal.label_of(id, cx));
    assert_eq!(editing.as_deref(), Some("tests"), "no check mark to edit");

    run_task(&workbench, shell_task(LABEL, dir.path()), cx);
    assert_eq!(labels(&workbench, cx), ["tests"]);
}

#[gpui::test]
fn a_new_root_forgets_the_last_task(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    run_task(&workbench, shell_task(LABEL, dir.path()), cx);
    let panel = terminal(&workbench, cx);
    panel.update(cx, |terminal, _| terminal.set_cwd(dir.path().to_owned()));
    assert!(workbench.read_with(cx, |w, cx| w.last_task(cx)).is_some());
    let other = dir.path().join("other");
    panel.update(cx, |terminal, _| terminal.set_cwd(other));
    assert_eq!(workbench.read_with(cx, |w, cx| w.last_task(cx)), None);
}

/// Nothing runs: neither program is there, so each says it couldn't start,
/// which it only does once its session tries to. The one behind goes first, in
/// the frame that lays it out beneath the active one. Were it left to start on
/// becoming active, it would only follow once the active one had gone.
#[gpui::test]
fn every_task_starts_though_only_one_is_active(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.fake_sessions = false);
    let missing = |label: &str| {
        let argv = vec!["/nonexistent/program".to_owned()];
        task(label, TaskCommand::Argv(argv), dir.path())
    };
    // In one update, so no frame comes between them.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_task(missing("one"), window, cx);
            workbench.run_task(missing("two"), window, cx);
        });
    });
    draw(cx);
    draw(cx);
    assert!(views(&workbench, cx).is_empty());
    let said: Vec<_> = (toasts(&workbench, cx).into_iter())
        .map(|(_, message, _)| message)
        .collect();
    assert_eq!(said, ["Couldn't start one", "Couldn't start two"]);
}

#[gpui::test]
fn show_does_nothing_once_its_tab_has_closed(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    run_task(&workbench, shell_task(LABEL, dir.path()), cx);
    let failed = views(&workbench, cx).remove(0);
    exit(&failed, Some(2), cx);
    let message = format!("{LABEL} failed (exit 2)");
    let show = choices(&workbench, &message, cx);

    // Its name goes to a new tab, which isn't the one the toast is about.
    let panel = terminal(&workbench, cx);
    panel.update_in(cx, |terminal, window, cx| terminal.close_active(window, cx));
    draw(cx);
    assert!(!dock_shown(&workbench, cx));
    run_task(&workbench, shell_task("lint", dir.path()), cx);
    run_task(&workbench, shell_task(LABEL, dir.path()), cx);
    assert_eq!(labels(&workbench, cx), ["lint", LABEL]);
    panel.update_in(cx, |terminal, window, cx| terminal.activate(0, window, cx));
    draw(cx);

    show.choose(0);
    draw(cx);
    assert_eq!(active(&workbench, cx), 0, "the lint tab stays");
}

#[gpui::test]
fn show_brings_the_tasks_window_forward(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let (workbench, vcx, dir) = opened(cx);
    run_task(&workbench, shell_task(LABEL, dir.path()), vcx);
    let failed = views(&workbench, vcx).remove(0);
    let dock = workbench.read_with(vcx, |workbench, _| workbench.terminal_dock().clone());
    dock.update(vcx, |dock, cx| {
        dock.dock_mut().set_visible(false);
        cx.notify();
    });
    draw(vcx);

    // It fails with another window in front, whose toasts say so.
    let other = app.add_window(|window, cx| {
        let theme = fe_gpui::EditorTheme::default_dark(cx);
        let launch = Launch {
            root: None,
            file: None,
        };
        Workbench::new(
            launch,
            theme,
            fe_gpui::EditorSettings::default(),
            window,
            cx,
        )
    });
    let elsewhere = other.root(&mut app).unwrap();
    let mut other = VisualTestContext::from_window(other.into(), &app);
    other.update(|window, _| window.activate_window());
    draw(&mut other);
    assert!(other.update(|window, _| window.is_window_active()));
    assert!(!vcx.update(|window, _| window.is_window_active()));
    failed.update(vcx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(2))));
    draw(vcx);
    draw(&mut other);
    let message = format!("{LABEL} failed (exit 2)");
    assert_eq!(toasts(&elsewhere, &mut other), [show_of(&message)]);

    choices(&elsewhere, &message, &mut other).choose(0);
    draw(&mut other);
    draw(vcx);
    assert!(vcx.update(|window, _| window.is_window_active()));
    assert!(!other.update(|window, _| window.is_window_active()));
    assert!(dock_shown(&workbench, vcx));
    assert!(focused(&failed, vcx));
}
