//! The task picker's row for a typed Gradle task or Maven goal, on a real
//! project in a temporary folder. Sessions are fake, so no process starts.

use std::{fs, io, path::Path, sync::Arc};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_l10n::tr;

use crate::{
    Launch, Workbench,
    task_commands::{ShownRow, SplitError, TaskPicker, TestFiles, split_args},
    task_defs::{ProjectFiles, Source, Task, TaskCommand},
    task_run::untrusted_message,
    tests::open,
    workbench::toast::Level,
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

struct Project<'a> {
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    dir: tempfile::TempDir,
}

/// A workbench on a temporary project holding `files`, trusted or not.
fn project<'a>(cx: &'a mut TestAppContext, files: &[&str], trust: bool) -> Project<'a> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "a\n").unwrap();
    for file in files {
        fs::write(dir.path().join(file), "").unwrap();
    }
    cx.update(|cx| cx.set_global(TestFiles(Arc::new(Disk))));
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    if trust {
        workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
        draw(cx);
    }
    Project { workbench, cx, dir }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

impl Project<'_> {
    fn run(&mut self, id: &str) {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run = RunCommand(Invocation::of(id).unwrap().context(context));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        draw(self.cx);
    }

    /// Opens the picker and types `query`.
    fn type_query(&mut self, query: &str) {
        self.run("tasks.run");
        if !query.is_empty() {
            self.cx.simulate_input(query);
        }
        draw(self.cx);
    }

    fn rows(&mut self) -> Vec<ShownRow> {
        let picker: Entity<kiln_shell::Picker<TaskPicker>> = (self.workbench)
            .read_with(self.cx, |workbench, cx| workbench.task_picker(cx))
            .expect("the task picker shows");
        picker.read_with(self.cx, |picker, _| picker.delegate.shown())
    }

    fn enter(&mut self) {
        self.cx.simulate_keystrokes("enter");
        draw(self.cx);
    }

    fn last_task(&mut self) -> Option<Task> {
        (self.workbench).read_with(self.cx, |workbench, cx| workbench.last_task(cx))
    }

    fn toasts(&mut self) -> Vec<(Level, String)> {
        let layer = (self.workbench).read_with(self.cx, |workbench, _| workbench.toasts().clone());
        layer.read_with(self.cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| (entry.toast.level, entry.toast.message.clone()))
                .collect()
        })
    }
}

fn argv(words: &[&str]) -> TaskCommand {
    TaskCommand::Argv(words.iter().map(|word| (*word).to_owned()).collect())
}

fn run_row(command: &str) -> String {
    tr!("workbench-task-custom-run", command = command).to_string()
}

fn custom(rows: Vec<ShownRow>) -> Option<(String, Option<String>)> {
    rows.into_iter().find_map(|row| match row {
        ShownRow::Custom(text, why) => Some((text, why)),
        _ => None,
    })
}

#[test]
fn split_args_handles_quotes_and_flags() {
    let words = |args: &[&str]| Ok(args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>());
    assert_eq!(split_args(":app:bootRun"), words(&[":app:bootRun"]));
    assert_eq!(
        split_args("  test   --tests  Foo "),
        words(&["test", "--tests", "Foo"])
    );
    assert_eq!(split_args("-q test"), words(&["-q", "test"]));
    assert_eq!(
        split_args(r#"run --args="a b" 'c d' e"f g"h"#),
        words(&["run", "--args=a b", "c d", "ef gh"])
    );
    assert_eq!(split_args(r#"x "it's""#), words(&["x", "it's"]));
    assert_eq!(split_args(r#"x """#), words(&["x", ""]));
    assert_eq!(split_args("; rm -rf /"), words(&[";", "rm", "-rf", "/"]));
}

#[gpui::test]
fn picker_offers_custom_row_only_with_gradle_or_maven_root(cx: &mut TestAppContext) {
    let mut bare = project(cx, &["Makefile"], true);
    bare.type_query("test");
    assert_eq!(custom(bare.rows()), None);
    drop(bare);

    let mut gradle = project(cx, &["build.gradle"], true);
    gradle.run("tasks.run");
    // Nothing typed, nothing to run.
    assert_eq!(custom(gradle.rows()), None);
    gradle.cx.simulate_input("zzz");
    draw(gradle.cx);
    // It's there whatever the query matches, and last.
    let rows = gradle.rows();
    assert_eq!(
        rows,
        [ShownRow::Custom(run_row("gradle zzz"), None)],
        "{rows:?}"
    );
    drop(gradle);

    let mut maven = project(cx, &["pom.xml", "mvnw"], true);
    maven.type_query("verify -DskipTests");
    assert_eq!(
        custom(maven.rows()),
        Some((run_row("./mvnw verify -DskipTests"), None))
    );
}

#[gpui::test]
fn custom_row_runs_wrapper_argv_without_shell(cx: &mut TestAppContext) {
    let mut project = project(cx, &["build.gradle.kts", "gradlew"], true);
    project.type_query(r#"test --tests "a b" ; echo hi"#);
    project.enter();
    let task = project.last_task().expect("a task ran");
    assert_eq!(
        task.command,
        argv(&["./gradlew", "test", "--tests", "a b", ";", "echo", "hi"])
    );
    assert_eq!(task.label, "./gradlew test --tests \"a b\" ; echo hi");
    assert_eq!(task.source, Source::Gradle);
    assert_eq!(task.cwd, project.dir.path());
    assert_eq!(task.group, None);
    assert!(!task.default);
}

#[gpui::test]
fn custom_row_prefers_gradle_over_maven(cx: &mut TestAppContext) {
    let mut project = project(cx, &["pom.xml", "mvnw", "settings.gradle"], true);
    project.type_query("--version");
    assert_eq!(
        custom(project.rows()),
        Some((run_row("gradle --version"), None))
    );
    // A tasks entry named like the query leaves the custom row alone, last.
    project.enter();
    let task = project.last_task();
    // Enter takes the first match, a detected task or the row: either way a
    // Gradle or Maven task.
    assert!(task.is_some_and(|task| matches!(task.source, Source::Gradle | Source::Maven)));
}

#[gpui::test]
fn empty_or_unterminated_query_is_not_runnable(cx: &mut TestAppContext) {
    assert_eq!(split_args("   "), Err(SplitError::Empty));
    assert_eq!(split_args("a 'b"), Err(SplitError::Unterminated));

    let mut project = project(cx, &["build.gradle"], true);
    project.type_query("zz \"b");
    let (_, why) = custom(project.rows()).expect("the row shows");
    assert_eq!(
        why,
        Some(tr!("workbench-task-custom-unterminated").to_string())
    );
    project.enter();
    assert_eq!(project.last_task(), None);
    let still_open = (project.workbench).read_with(project.cx, |workbench, cx| {
        workbench.task_picker(cx).is_some()
    });
    assert!(still_open);
}

#[gpui::test]
fn rerun_last_repeats_custom_task(cx: &mut TestAppContext) {
    let mut project = project(cx, &["build.gradle", "gradlew"], true);
    project.type_query(":app:bootRun --args=x");
    // The row is the only match, so Enter runs it.
    project.enter();
    let first = project.last_task().expect("a task ran");
    assert_eq!(
        first.command,
        argv(&["./gradlew", ":app:bootRun", "--args=x"])
    );
    let terminal =
        (project.workbench).read_with(project.cx, |workbench, _| workbench.terminal().clone());
    let started = terminal.read_with(project.cx, |terminal, _| terminal.views().count());
    project.run("tasks.rerun_last");
    assert_eq!(project.last_task(), Some(first));
    let views = terminal.read_with(project.cx, |terminal, _| terminal.views().count());
    assert_eq!(views, started + 1);
}

#[gpui::test]
fn untrusted_root_refuses_custom_task(cx: &mut TestAppContext) {
    let mut project = project(cx, &["build.gradle"], false);
    project.type_query(":app:bootRun");
    project.enter();
    assert_eq!(project.toasts(), [(Level::Warning, untrusted_message())]);
    let tabs = (project.workbench).read_with(project.cx, |workbench, cx| {
        workbench.terminal().read(cx).views().count()
    });
    assert_eq!(tabs, 0);
}
