//! The run widget in the title bar (T1539): the chosen task's label (kept
//! per project), play to run it, stop while it runs, the error glyph after
//! a failed run, and nothing with no tasks.

use std::{fs, io, path::Path, sync::Arc};

use fe_terminal_view::{TerminalView, TerminalViewEvent};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_project::FsChange;

use crate::{
    Launch, Workbench, task_commands::TestFiles, task_defs::ProjectFiles, tests::open,
    title_bar::DRAWN,
};

struct Disk;

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }
}

const TASKS: &str = r#"
[[task]]
label = "alpha"
command = "true"
group = "build"
default = true

[[task]]
label = "beta"
command = "true"
"#;

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// A trusted window on a temporary project with `tasks` as its tasks file
/// (none if empty), its title bar shown, its sessions fake.
fn window<'a>(
    cx: &'a mut TestAppContext,
    tasks: &str,
) -> (
    Entity<Workbench>,
    &'a mut VisualTestContext,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().unwrap();
    if !tasks.is_empty() {
        fs::create_dir(dir.path().join(".tenmoku")).unwrap();
        fs::write(dir.path().join(".tenmoku/tasks.toml"), tasks).unwrap();
    }
    cx.update(|cx| cx.set_global(TestFiles(Arc::new(Disk))));
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    workbench.update(cx, |workbench, cx| {
        workbench.set_root_trust(true, cx);
        workbench.set_title_bar(true, cx);
    });
    draw(cx);
    (workbench, cx, dir)
}

/// The run widget's buttons the last frame drew: id and accessible name.
fn run_buttons() -> Vec<(&'static str, String)> {
    DRAWN.with_borrow(|drawn| {
        (drawn.iter())
            .filter(|(id, _)| id.starts_with("title-run"))
            .map(|(id, name)| (*id, name.to_string()))
            .collect()
    })
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let at = cx.debug_bounds(selector).expect(selector).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

fn redraw(cx: &mut VisualTestContext) {
    DRAWN.with_borrow_mut(Vec::clear);
    draw(cx);
}

fn session_count(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.read_with(cx, |terminal, _| terminal.views().count())
}

#[gpui::test]
fn with_no_tasks_nothing_is_drawn(cx: &mut TestAppContext) {
    let (_workbench, cx, _dir) = window(cx, "");
    redraw(cx);
    assert_eq!(run_buttons(), []);
    assert!(cx.debug_bounds("title-run").is_none());
}

#[gpui::test]
fn the_widget_shows_a_task_with_play(cx: &mut TestAppContext) {
    let (_workbench, cx, _dir) = window(cx, TASKS);
    redraw(cx);
    assert_eq!(
        run_buttons(),
        [
            ("title-run", "Task: alpha".to_owned()),
            ("title-run-play", "Run Task".to_owned()),
        ]
    );
}

#[gpui::test]
fn the_choice_is_kept(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = window(cx, TASKS);
    click("title-run", cx);
    let shown = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        layer.modal().map(|id| id.0)
    });
    assert_eq!(shown, Some("run-widget-tasks"));
    cx.simulate_keystrokes("down enter");
    draw(cx);
    // Choosing doesn't run.
    assert_eq!(session_count(&workbench, cx), 0);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: beta");
    // It stays after another run of the widget.
    click("icon-button:title-run-play", cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: beta, running");
}

#[gpui::test]
fn play_runs_and_stop_stops(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = window(cx, TASKS);
    click("icon-button:title-run-play", cx);
    assert_eq!(session_count(&workbench, cx), 1);
    redraw(cx);
    assert_eq!(
        run_buttons(),
        [
            ("title-run", "Task: alpha, running".to_owned()),
            ("title-run-stop", "Stop Task".to_owned()),
        ]
    );
    click("icon-button:title-run-stop", cx);
    assert_eq!(session_count(&workbench, cx), 0);
    redraw(cx);
    assert_eq!(run_buttons()[1].0, "title-run-play");
}

#[gpui::test]
fn a_failed_run_shows_the_error_glyph(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = window(cx, TASKS);
    click("icon-button:title-run-play", cx);
    let view = workbench
        .read_with(cx, |workbench, cx| {
            workbench.terminal().read(cx).views().next().cloned()
        })
        .unwrap();
    view.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(2))));
    draw(cx);
    redraw(cx);
    assert_eq!(
        run_buttons(),
        [
            ("title-run", "Task: alpha, last run failed".to_owned()),
            ("title-run-play", "Run Task".to_owned()),
        ]
    );
}

/// Asks for command `id` in the workbench's context.
fn command(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn tasks_file(dir: &Path, text: &str) {
    fs::create_dir_all(dir.join(".tenmoku")).unwrap();
    fs::write(dir.join(".tenmoku/tasks.toml"), text).unwrap();
}

/// Tells the workbench the watcher saw `dir`'s tasks file change.
fn tasks_file_changed(workbench: &Entity<Workbench>, dir: &Path, cx: &mut VisualTestContext) {
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    let path = dir.join(".tenmoku/tasks.toml");
    panel.update(cx, |panel, cx| {
        panel.apply_fs_changes(&[FsChange::Path(path)], cx)
    });
    draw(cx);
    redraw(cx);
}

fn last_task(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> crate::task_defs::Task {
    workbench
        .read_with(cx, |workbench, cx| workbench.last_task(cx))
        .expect("a task ran")
}

/// F1: after the root changes the widget lists and runs the new root's
/// tasks, in its directory.
#[gpui::test]
fn a_new_root_has_its_own_tasks(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = window(cx, TASKS);
    let other = tempfile::tempdir().unwrap();
    tasks_file(
        other.path(),
        "[[task]]\nlabel = \"delta\"\ncommand = \"true\"\n",
    );
    let root = other.path().to_owned();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx))
    });
    // The new root isn't trusted yet.
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: delta");
    click("icon-button:title-run-play", cx);
    let task = last_task(&workbench, cx);
    assert_eq!(task.label, "delta");
    assert_eq!(task.cwd, other.path());
}

/// F2: the choice survives a title bar reinstall, and a layout save and
/// restore.
#[gpui::test]
fn the_choice_survives_reinstall_and_restore(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = window(cx, TASKS);
    click("title-run", cx);
    cx.simulate_keystrokes("down enter");
    draw(cx);
    workbench.update(cx, |workbench, cx| workbench.set_title_bar(true, cx));
    draw(cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: beta");
    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    // Chosen otherwise since.
    click("title-run", cx);
    cx.simulate_keystrokes("up enter");
    draw(cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha");
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.restore_layout(&entry, window, cx)
        })
    });
    draw(cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: beta");
}

/// F3: adding, editing and removing a task definition shows, with no run in
/// between.
#[gpui::test]
fn task_definitions_changing_show(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = window(cx, TASKS);
    // Added: it can be chosen.
    tasks_file(
        dir.path(),
        &format!("{TASKS}\n[[task]]\nlabel = \"gamma\"\ncommand = \"true\"\n"),
    );
    tasks_file_changed(&workbench, dir.path(), cx);
    click("title-run", cx);
    cx.simulate_keystrokes("g a m m a enter");
    draw(cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: gamma");
    // Edited: play runs the edit, though no batch has been heard.
    tasks_file(
        dir.path(),
        &format!("{TASKS}\n[[task]]\nlabel = \"gamma\"\ncommand = \"echo edited\"\n"),
    );
    click("icon-button:title-run-play", cx);
    let task = last_task(&workbench, cx);
    assert_eq!(
        task.command,
        crate::task_defs::TaskCommand::Shell("echo edited".to_owned())
    );
    // Removed: the widget falls back to what's left.
    tasks_file(dir.path(), TASKS);
    tasks_file_changed(&workbench, dir.path(), cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha");
}

/// F4: runs started by the task commands show, stop from the widget, and a
/// rerun stays tracked.
#[gpui::test]
fn runs_from_commands_are_tracked(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = window(cx, TASKS);
    // run_build: alpha is the default build task.
    command("tasks.run_build", cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha, running");
    assert_eq!(run_buttons()[1].0, "title-run-stop");
    click("icon-button:title-run-stop", cx);
    assert_eq!(session_count(&workbench, cx), 0);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha");
    // tasks.run: the first row is alpha.
    command("tasks.run", cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha, running");
    // It fails; rerunning it by the command replaces its session.
    let view = workbench
        .read_with(cx, |workbench, cx| {
            workbench.terminal().read(cx).views().next().cloned()
        })
        .unwrap();
    view.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(1))));
    draw(cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha, last run failed");
    command("tasks.rerun_last", cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha, running");
    click("icon-button:title-run-stop", cx);
    assert_eq!(session_count(&workbench, cx), 0);
}

fn set_root(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) {
    let root = root.to_owned();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx))
    });
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    redraw(cx);
}

fn exit(view: &Entity<TerminalView>, code: i32, cx: &mut VisualTestContext) {
    view.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(code))));
    draw(cx);
    redraw(cx);
}

fn views(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<TerminalView>> {
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.read_with(cx, |terminal, _| terminal.views().cloned().collect())
}

/// F5: with two instances finished, a rerun that reuses the first tab has the
/// last word, whatever the other one's tab says.
#[gpui::test]
fn the_latest_run_has_the_last_word(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = window(cx, TASKS);
    command("tasks.run_build", cx);
    // A second instance, while the first runs.
    command("tasks.rerun_last", cx);
    let running = views(&workbench, cx);
    assert_eq!(running.len(), 2);
    exit(&running[0], 0, cx);
    exit(&running[1], 1, cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha, last run failed");
    // The rerun takes the first finished tab, and passes.
    command("tasks.rerun_last", cx);
    let rerun = views(&workbench, cx);
    assert_eq!(rerun.len(), 2);
    exit(&rerun[0], 0, cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha");
    // And the other way round.
    command("tasks.rerun_last", cx);
    let rerun = views(&workbench, cx);
    exit(&rerun[0], 3, cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha, last run failed");
}

/// F1: a task of another project with the same label and directory isn't
/// this one's run.
#[gpui::test]
fn another_project_does_not_share_a_run(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    tasks_file(
        dir.path(),
        "[[task]]\nlabel = \"alpha\"\ncommand = \"true\"\ncwd = \"sub\"\n",
    );
    let sub = dir.path().join("sub");
    tasks_file(&sub, "[[task]]\nlabel = \"alpha\"\ncommand = \"true\"\n");
    let (workbench, cx, _own) = window(cx, TASKS);
    set_root(&workbench, dir.path(), cx);
    click("icon-button:title-run-play", cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha, running");
    // The same label, in the same directory, run from another root.
    set_root(&workbench, &sub, cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha");
    assert_eq!(run_buttons()[1].0, "title-run-play");
}

/// F3: a folder made or removed shows the tasks it holds, or no widget.
#[gpui::test]
fn the_tasks_folder_coming_and_going_shows(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = window(cx, "");
    redraw(cx);
    assert_eq!(run_buttons(), []);
    tasks_file(dir.path(), TASKS);
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    let folder = dir.path().join(".tenmoku");
    let event = |path: std::path::PathBuf, cx: &mut VisualTestContext| {
        panel.update(cx, |panel, cx| {
            panel.apply_fs_changes(&[FsChange::Path(path)], cx)
        });
        draw(cx);
        redraw(cx);
    };
    // Only the folder is named.
    event(folder.clone(), cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha");
    fs::remove_dir_all(&folder).unwrap();
    event(folder, cx);
    assert_eq!(run_buttons(), []);
}

/// F6: a click waits for the tasks to be read, and acts on nothing if the
/// root changed meanwhile.
#[gpui::test]
fn a_pending_play_dies_with_its_root(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = window(cx, TASKS);
    cx.update(Workbench::press_run_play);
    // Nothing has run: the read is pending.
    assert_eq!(session_count(&workbench, cx), 0);
    let other = tempfile::tempdir().unwrap();
    set_root(&workbench, other.path(), cx);
    assert_eq!(session_count(&workbench, cx), 0);
}

#[gpui::test]
fn play_waits_for_the_read_then_runs(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = window(cx, TASKS);
    cx.update(Workbench::press_run_play);
    assert_eq!(session_count(&workbench, cx), 0);
    draw(cx);
    assert_eq!(session_count(&workbench, cx), 1);
}

/// F1: a task's directory edited while it runs is still the same task: it
/// shows running, stops, and runs in the edited directory next.
#[gpui::test]
fn editing_the_directory_keeps_the_run(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = window(cx, TASKS);
    fs::create_dir(dir.path().join("sub")).unwrap();
    click("icon-button:title-run-play", cx);
    redraw(cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha, running");
    tasks_file(
        dir.path(),
        &TASKS.replace(
            "command = \"true\"\ngroup",
            "command = \"true\"\ncwd = \"sub\"\ngroup",
        ),
    );
    tasks_file_changed(&workbench, dir.path(), cx);
    assert_eq!(run_buttons()[0].1, "Task: alpha, running");
    assert_eq!(run_buttons()[1].0, "title-run-stop");
    click("icon-button:title-run-stop", cx);
    assert_eq!(session_count(&workbench, cx), 0);
    redraw(cx);
    click("icon-button:title-run-play", cx);
    assert_eq!(last_task(&workbench, cx).cwd, dir.path().join("sub"));
}
