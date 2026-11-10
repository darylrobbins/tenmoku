//! The task commands on a real project in a temporary folder, read through a
//! seam that counts. Sessions are fake, so no process starts.

use std::{
    fs, io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_terminal_view::TerminalViewEvent;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::Picker;

use crate::{
    Launch, TerminalPanel, Workbench,
    task_commands::{ShownRow, TaskPicker, TestFiles},
    task_defs::{ProjectFiles, TASKS_FILE},
    task_run::UNTRUSTED,
    tests::open,
    workbench::toast::Level,
};

const NO_TASKS: &str = "No tasks: add .tenmoku/tasks.toml";

/// The disk, counting what's read from it.
#[derive(Default)]
struct CountingDisk {
    reads: AtomicUsize,
}

impl ProjectFiles for CountingDisk {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
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
    disk: Arc<CountingDisk>,
}

/// A workbench showing a text file in a temporary project, focused as the app
/// focuses it, whose terminal panel starts no process. The root isn't trusted.
fn untrusted(cx: &mut TestAppContext) -> Project<'_> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "a\n").unwrap();
    let disk = Arc::new(CountingDisk::default());
    cx.update({
        let disk = disk.clone();
        |cx| cx.set_global(TestFiles(disk))
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    Project {
        workbench,
        cx,
        dir,
        disk,
    }
}

/// As [`untrusted`], with the root trusted.
fn trusted(cx: &mut TestAppContext) -> Project<'_> {
    let project = untrusted(cx);
    (project.workbench).update(project.cx, |workbench, cx| {
        workbench.set_root_trust(true, cx)
    });
    draw(project.cx);
    project
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

impl Project<'_> {
    fn write(&self, file: &str, text: &str) {
        let path = self.dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Asks for command `id` in the workbench's context, doing no more than
    /// that: nothing the background has been given runs yet.
    fn ask(&mut self, id: &str) {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run = RunCommand(Invocation::of(id).unwrap().context(context));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
    }

    fn run(&mut self, id: &str) {
        self.ask(id);
        draw(self.cx);
    }

    fn picker(&mut self) -> Option<Entity<Picker<TaskPicker>>> {
        (self.workbench).read_with(self.cx, |workbench, cx| workbench.task_picker(cx))
    }

    fn rows(&mut self) -> Vec<ShownRow> {
        let picker = self.picker().expect("the task picker shows");
        picker.read_with(self.cx, |picker, _| picker.delegate.shown())
    }

    /// The labels of the tabs of the tasks run, in the order they're listed.
    fn tabs(&mut self) -> Vec<String> {
        draw(self.cx);
        terminal(&self.workbench, self.cx).read_with(self.cx, |terminal, _| {
            (terminal.drawn_tabs().iter())
                .map(|(label, _)| label.to_string())
                .collect()
        })
    }

    fn last_task(&mut self) -> Option<String> {
        (self.workbench).read_with(self.cx, |workbench, cx| {
            workbench.last_task(cx).map(|task| task.label)
        })
    }

    fn editor_focused(&mut self) -> bool {
        let editor = (self.workbench)
            .read_with(self.cx, |workbench, cx| {
                workbench.active_editor(cx).cloned()
            })
            .expect("an editor shows");
        self.cx
            .update(|window, cx| editor.focus_handle(cx).is_focused(window))
    }

    fn toasts(&mut self) -> Vec<(Level, String)> {
        let layer = (self.workbench).read_with(self.cx, |workbench, _| workbench.toasts().clone());
        layer.read_with(self.cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| (entry.toast.level, entry.toast.message.clone()))
                .collect()
        })
    }

    /// Ends the session of the task tab showing, so a rerun reuses its tab.
    fn exit_active(&mut self) {
        let terminal = terminal(&self.workbench, self.cx);
        let view = terminal.read_with(self.cx, |terminal, _| {
            terminal.views().nth(terminal.active()).cloned().unwrap()
        });
        view.update(self.cx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(0))));
        draw(self.cx);
    }

    fn views(&mut self) -> Vec<gpui::EntityId> {
        let terminal = terminal(&self.workbench, self.cx);
        terminal.read_with(self.cx, |terminal, _| {
            terminal.views().map(|view| view.entity_id()).collect()
        })
    }
}

fn task(label: &str, detail: &str) -> ShownRow {
    ShownRow::Task(label.to_owned(), detail.to_owned())
}

/// A tasks file of `(label, command, group, default)` entries.
fn tasks_file(entries: &[(&str, &str, &str, bool)]) -> String {
    (entries.iter())
        .map(|(label, command, group, default)| {
            format!(
                "[[task]]\nlabel = \"{label}\"\ncommand = \"{command}\"\n\
                 group = \"{group}\"\ndefault = {default}\n"
            )
        })
        .collect()
}

#[gpui::test]
fn run_task_lists_file_then_detected_tasks(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    project.write(
        TASKS_FILE,
        "[[task]]\nlabel = \"serve\"\ncommand = \"cargo run -p server\"\n\n\
         [[task]]\nlabel = \"say\"\ncommand = [\"echo\", \"two words\"]\n\n\
         [[task]]\nlabel = \"both\"\ncommand = \"a &&\\nb\"\n",
    );
    project.write("Cargo.toml", "[package]\nname = \"p\"\n");
    project.write("package.json", r#"{"scripts": {"lint": "eslint ."}}"#);
    project.write("Makefile", "all:\n\ttrue\n");
    project.write("justfile", "fmt:\n    true\n");

    project.run("tasks.run");
    assert_eq!(
        project.rows(),
        [
            task("serve", "tasks.toml · cargo run -p server"),
            task("say", "tasks.toml · echo \"two words\""),
            task("both", "tasks.toml · a && b"),
            task("cargo build", "cargo · cargo build"),
            task("cargo test", "cargo · cargo test"),
            task("cargo run", "cargo · cargo run"),
            task("cargo clippy", "cargo · cargo clippy"),
            task("npm run lint", "npm · npm run lint"),
            task("make all", "make · make all"),
            task("just fmt", "just · just fmt"),
        ]
    );
    assert_eq!(project.last_task(), None);

    // Filtered, fuzzily: by the label, highlighting it...
    project.cx.simulate_input("mkal");
    draw(project.cx);
    assert_eq!(project.rows(), [task("make all", "make · make all")]);
    let picker = project.picker().unwrap();
    let highlights = picker.read_with(project.cx, |picker, _| picker.delegate.highlights(0));
    assert_eq!(highlights, [0..1, 2..3, 5..7]);
    picker.update_in(project.cx, |picker, window, cx| picker.show(window, cx));

    // ...or by its source and command, highlighting nothing.
    project.cx.simulate_input("tasks.toml");
    draw(project.cx);
    assert_eq!(project.rows().len(), 3);
    let picker = project.picker().unwrap();
    let highlights = picker.read_with(project.cx, |picker, _| picker.delegate.highlights(0));
    assert!(highlights.is_empty(), "{highlights:?}");

    // Escape runs nothing, and leaves focus where it was.
    project.cx.simulate_keystrokes("escape");
    draw(project.cx);
    assert!(project.picker().is_none());
    assert_eq!(project.tabs(), Vec::<String>::new());
    assert_eq!(project.last_task(), None);
    assert!(project.editor_focused());

    // Enter runs the row selected, in the terminal, which takes the focus.
    project.run("tasks.run");
    project.cx.simulate_input("just");
    draw(project.cx);
    project.cx.simulate_keystrokes("enter");
    draw(project.cx);
    assert!(project.picker().is_none());
    assert_eq!(project.tabs(), ["just fmt"]);
    assert_eq!(project.last_task().as_deref(), Some("just fmt"));
    let terminal = terminal(&project.workbench, project.cx);
    let view = terminal.read_with(project.cx, |terminal, _| {
        terminal.views().next().cloned().unwrap()
    });
    let focused = (project.cx).update(|window, cx| view.focus_handle(cx).is_focused(window));
    assert!(focused);
    let dock = project
        .workbench
        .read_with(project.cx, |workbench, _| workbench.terminal_dock().clone());
    assert!(dock.read_with(project.cx, |dock, _| dock.is_visible()));
}

#[gpui::test]
fn picking_in_a_folder_not_trusted_is_refused(cx: &mut TestAppContext) {
    let mut project = untrusted(cx);
    project.write("Makefile", "all:\n\ttrue\n");
    project.run("tasks.run");
    assert_eq!(project.rows(), [task("make all", "make · make all")]);

    project.cx.simulate_keystrokes("enter");
    draw(project.cx);
    assert!(project.picker().is_none());
    assert_eq!(project.tabs(), Vec::<String>::new());
    assert_eq!(project.toasts(), [(Level::Warning, UNTRUSTED.to_owned())]);
    assert!(project.editor_focused());
}

#[gpui::test]
fn a_pick_from_a_root_since_left_runs_nothing(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    project.write("Makefile", "all:\n\ttrue\n");
    project.run("tasks.run");

    let other = tempfile::tempdir().unwrap();
    let picker = project.picker().unwrap();
    let (workbench, root) = (project.workbench.clone(), other.path().to_owned());
    project.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx));
    });
    draw(project.cx);
    picker.update_in(project.cx, |picker, window, cx| {
        picker.confirm(0, window, cx);
    });
    draw(project.cx);
    assert_eq!(project.tabs(), Vec::<String>::new());
    assert_eq!(project.last_task(), None);
    // Not asked to run and refused, the new root not being trusted.
    assert_eq!(project.toasts(), []);
}

#[gpui::test]
fn empty_project_says_so(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    project.run("tasks.run");
    assert_eq!(project.rows(), [ShownRow::Empty(NO_TASKS.to_owned())]);

    // It's there whatever's typed, and Enter on it does nothing.
    project.cx.simulate_input("zz");
    draw(project.cx);
    assert_eq!(project.rows(), [ShownRow::Empty(NO_TASKS.to_owned())]);
    project.cx.simulate_keystrokes("enter");
    draw(project.cx);
    assert!(project.picker().is_some());
    assert_eq!(project.last_task(), None);
    project.cx.simulate_keystrokes("escape");
    draw(project.cx);

    // A problem with an entry shows below, naming it, and can't be picked.
    project.write(TASKS_FILE, "[[task]]\nlabel = \"broken\"\n");
    project.run("tasks.run");
    let rows = project.rows();
    assert_eq!(rows[0], ShownRow::Empty(NO_TASKS.to_owned()));
    assert_eq!(rows.len(), 2);
    let ShownRow::Warning(warning) = &rows[1] else {
        panic!("a warning row: {rows:?}");
    };
    assert!(warning.starts_with("task broken: "), "{warning}");
    let picker = project.picker().unwrap();
    picker.update_in(project.cx, |picker, window, cx| {
        picker.confirm(1, window, cx);
    });
    draw(project.cx);
    assert!(project.picker().is_some());
    assert_eq!(project.last_task(), None);
    project.cx.simulate_keystrokes("escape");
    draw(project.cx);

    // A file that doesn't parse is one row too, on one line, below the tasks.
    project.write(TASKS_FILE, "[[task]\n");
    project.write("Makefile", "all:\n\ttrue\n");
    project.run("tasks.run");
    let rows = project.rows();
    assert_eq!(rows[0], task("make all", "make · make all"));
    let ShownRow::Warning(warning) = &rows[1] else {
        panic!("a warning row: {rows:?}");
    };
    assert!(warning.starts_with(TASKS_FILE), "{warning}");
    assert!(!warning.contains('\n'), "{warning}");
}

#[gpui::test]
fn no_folder_has_no_tasks(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(TestFiles(Arc::new(CountingDisk::default()))));
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of("tasks.run").unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let picker = workbench
        .read_with(cx, |workbench, cx| workbench.task_picker(cx))
        .expect("the task picker shows");
    assert_eq!(
        picker.read_with(cx, |picker, _| picker.delegate.shown()),
        [ShownRow::Empty(NO_TASKS.to_owned())]
    );
}

#[gpui::test]
fn rerun_last_task(cx: &mut TestAppContext) {
    let mut project = trusted(cx);

    // None yet.
    project.run("tasks.rerun_last");
    assert_eq!(
        project.toasts(),
        [(Level::Info, "No task has run yet".to_owned())]
    );
    assert_eq!(project.tabs(), Vec::<String>::new());

    project.write("Makefile", "all:\n\ttrue\n\nclean:\n\ttrue\n");
    for query in ["all", "clean"] {
        project.run("tasks.run");
        project.cx.simulate_input(query);
        draw(project.cx);
        project.cx.simulate_keystrokes("enter");
        draw(project.cx);
    }
    assert_eq!(project.tabs(), ["make all", "make clean"]);
    assert_eq!(project.last_task().as_deref(), Some("make clean"));
    let before = project.views();

    // The last run again: its finished tab, with a new session.
    project.exit_active();
    project.run("tasks.rerun_last");
    assert_eq!(project.tabs(), ["make all", "make clean"]);
    let after = project.views();
    assert_eq!(after[0], before[0]);
    assert_ne!(after[1], before[1]);
    assert_eq!(project.last_task().as_deref(), Some("make clean"));
    assert_eq!(
        project.toasts(),
        [(Level::Info, "No task has run yet".to_owned())]
    );
}

#[gpui::test]
fn run_build_task_uses_the_default(cx: &mut TestAppContext) {
    let mut project = trusted(cx);

    // A default of the file's: run with no picker, over Cargo's.
    project.write("Cargo.toml", "[package]\nname = \"p\"\n");
    project.write(
        TASKS_FILE,
        &tasks_file(&[
            ("check", "true", "test", true),
            ("all", "true", "build", true),
        ]),
    );
    project.run("tasks.run_build");
    assert!(project.picker().is_none());
    assert_eq!(project.tabs(), ["all"]);

    // Cargo's, with none in the file.
    project.write(TASKS_FILE, "");
    project.run("tasks.run_build");
    assert!(project.picker().is_none());
    assert_eq!(project.tabs(), ["all", "cargo build"]);

    // With none, the picker on the build group, whose pick runs.
    fs::remove_file(project.dir.path().join("Cargo.toml")).unwrap();
    project.write(
        TASKS_FILE,
        &tasks_file(&[
            ("one", "true", "build", false),
            ("spec", "true", "test", false),
            ("two", "true", "build", false),
        ]),
    );
    project.run("tasks.run_build");
    assert_eq!(
        project.rows(),
        [
            task("one", "tasks.toml · true"),
            task("two", "tasks.toml · true"),
        ]
    );
    project.cx.simulate_keystrokes("down");
    draw(project.cx);
    project.cx.simulate_keystrokes("enter");
    draw(project.cx);
    assert!(project.picker().is_none());
    assert_eq!(project.last_task().as_deref(), Some("two"));

    // And with no build task at all, it says that.
    project.write(TASKS_FILE, &tasks_file(&[("spec", "true", "test", false)]));
    project.run("tasks.run_build");
    assert_eq!(
        project.rows(),
        [ShownRow::Empty(
            "No build tasks: add .tenmoku/tasks.toml".to_owned()
        )]
    );
}

#[gpui::test]
fn a_default_build_read_from_a_root_since_left_runs_nothing(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    project.write("Cargo.toml", "[package]\nname = \"p\"\n");
    project.ask("tasks.run_build");

    let other = tempfile::tempdir().unwrap();
    let (workbench, root) = (project.workbench.clone(), other.path().to_owned());
    project.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx));
    });
    draw(project.cx);
    assert_eq!(project.tabs(), Vec::<String>::new());
    assert_eq!(project.last_task(), None);
    // Not asked to run and refused, the new root not being trusted.
    assert_eq!(project.toasts(), []);
}

#[gpui::test]
fn tasks_are_rescanned_on_open(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    project.write(TASKS_FILE, &tasks_file(&[("old", "true", "run", false)]));

    // The picker shows at once, and nothing's read until the background runs.
    let reads = project.disk.reads.load(Ordering::SeqCst);
    project.ask("tasks.run");
    let picker = project.picker().expect("the task picker shows");
    picker.read_with(project.cx, |picker, _| {
        assert!(!picker.delegate.loaded());
        assert_eq!(
            crate::picker::PickerDelegate::status(&picker.delegate).as_deref(),
            Some("Reading tasks…")
        );
        assert_eq!(picker.delegate.shown(), []);
    });
    assert_eq!(project.disk.reads.load(Ordering::SeqCst), reads);
    draw(project.cx);
    picker.read_with(project.cx, |picker, _| {
        assert!(picker.delegate.loaded());
        assert_eq!(
            crate::picker::PickerDelegate::status(&picker.delegate),
            None
        );
    });
    assert_eq!(project.rows(), [task("old", "tasks.toml · true")]);
    project.cx.simulate_keystrokes("escape");
    draw(project.cx);

    project.write(TASKS_FILE, &tasks_file(&[("new", "true", "run", false)]));
    project.run("tasks.run");
    assert_eq!(project.rows(), [task("new", "tasks.toml · true")]);
}

/// The way in: the palette's row and the key, each leaving the picker focused,
/// and focus back in the editor once it's dismissed.
#[gpui::test]
fn the_palette_and_the_key_reach_the_commands(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    let editor = (project.workbench)
        .read_with(project.cx, |workbench, cx| {
            workbench.active_editor(cx).cloned()
        })
        .expect("an editor shows");
    project
        .cx
        .update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(project.cx);
    project.write(TASKS_FILE, &tasks_file(&[("one", "true", "build", false)]));

    project.cx.simulate_keystrokes("cmd-shift-p");
    draw(project.cx);
    project.cx.simulate_input("Tasks: Run Task…");
    draw(project.cx);
    project.cx.simulate_keystrokes("enter");
    draw(project.cx);
    assert_eq!(project.rows(), [task("one", "tasks.toml · true")]);
    let picker = project.picker().unwrap();
    let focused = (project.cx).update(|window, cx| picker.focus_handle(cx).is_focused(window));
    assert!(focused);
    project.cx.simulate_keystrokes("escape");
    draw(project.cx);
    assert!(project.picker().is_none());
    assert!(project.editor_focused());

    project.cx.simulate_keystrokes("cmd-shift-b");
    draw(project.cx);
    assert_eq!(project.rows(), [task("one", "tasks.toml · true")]);
    project.cx.simulate_keystrokes("escape");
    draw(project.cx);
    assert!(project.editor_focused());
}
