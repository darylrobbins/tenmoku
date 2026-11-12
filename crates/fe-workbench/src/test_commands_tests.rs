//! The test commands: the command lines they build, from a project read
//! through a fake, and the commands on a real project in a temporary folder,
//! whose sessions are fake, so no process starts.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_terminal_view::TerminalViewEvent;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Launch, TerminalPanel, Workbench,
    task_commands::TestFiles,
    task_defs::{Group, ProjectFiles, Source, Task, TaskCommand},
    task_problems::{Matcher, match_output},
    task_run::untrusted_message,
    test_commands::{no_test_here, no_test_run},
    test_runnables::{Runnable, RunnableKind, test_command},
    tests::open,
    workbench::toast::Level,
};

/// A project's files, in memory.
#[derive(Default)]
struct Files(HashMap<PathBuf, String>);

impl Files {
    fn with(mut self, path: &str, text: &str) -> Self {
        self.0.insert(PathBuf::from(path), text.to_owned());
        self
    }
}

impl ProjectFiles for Files {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        (self.0.get(path).cloned()).ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn exists(&self, path: &Path) -> bool {
        self.0.contains_key(path)
    }
}

fn runnable(kind: RunnableKind, name: &str, selector: &str) -> Runnable {
    Runnable {
        name: name.to_owned(),
        kind,
        rows: 0..1,
        selector: selector.to_owned(),
        exact: kind == RunnableKind::Test,
    }
}

fn test(name: &str, selector: &str) -> Runnable {
    runnable(RunnableKind::Test, name, selector)
}

/// The command line of `language`'s task for `runnable` in `path`, under
/// `root`, and where it runs.
fn command(
    files: &Files,
    language: &str,
    runnable: &Runnable,
    path: &str,
    root: &str,
) -> (String, PathBuf) {
    let task = test_command(files, language, runnable, Path::new(path), Path::new(root))
        .unwrap_or_else(|error| panic!("a task for {path}: {error}"));
    let TaskCommand::Argv(argv) = &task.command else {
        panic!("an argument list: {task:?}");
    };
    (argv.join(" "), task.cwd)
}

fn crate_files() -> Files {
    (Files::default())
        .with("/p/Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n")
        .with(
            "/p/crates/adder/Cargo.toml",
            "[package]\nname = \"adder\"\n",
        )
}

#[test]
fn cargo_command_for_a_test_in_a_nested_module() {
    let files = crate_files();
    let adds = test("adds", "tests::nested::adds");
    let ops = "/p/crates/adder/src/math/ops.rs";
    // It runs in the workspace's directory, which its output's paths are from.
    let workspace = PathBuf::from("/p");

    let task = test_command(&files, "rust", &adds, Path::new(ops), Path::new("/p")).unwrap();
    assert_eq!(
        task,
        Task {
            label: "Test: adds".to_owned(),
            command: TaskCommand::Argv(
                [
                    "cargo",
                    "test",
                    "-p",
                    "adder",
                    "math::ops::tests::nested::adds",
                    "--",
                    "--exact",
                    "--nocapture",
                ]
                .map(str::to_owned)
                .into()
            ),
            cwd: workspace.clone(),
            env: Default::default(),
            group: Some(Group::Test),
            default: false,
            source: Source::Cargo,
            problem_matcher: None,
        }
    );

    // A `mod.rs` is its directory's module; the crate's root has none.
    let mod_rs = "/p/crates/adder/src/math/mod.rs";
    assert_eq!(
        command(&files, "rust", &adds, mod_rs, "/p").0,
        "cargo test -p adder math::tests::nested::adds -- --exact --nocapture"
    );
    let lib = "/p/crates/adder/src/lib.rs";
    assert_eq!(
        command(&files, "rust", &adds, lib, "/p").0,
        "cargo test -p adder tests::nested::adds -- --exact --nocapture"
    );

    // A module runs what's under it; a test of cases, its cases.
    let module = runnable(RunnableKind::Module, "tests", "tests");
    assert_eq!(
        command(&files, "rust", &module, ops, "/p").0,
        "cargo test -p adder math::ops::tests:: -- --nocapture"
    );
    let cases = Runnable {
        exact: false,
        ..test("doubles", "tests::doubles")
    };
    assert_eq!(
        command(&files, "rust", &cases, ops, "/p").0,
        "cargo test -p adder math::ops::tests::doubles -- --nocapture"
    );

    // A file runs its module's tests; the crate's root file, its target's.
    let file = |path: &str| Runnable::file(Path::new(path), 10);
    assert_eq!(
        command(&files, "rust", &file(ops), ops, "/p"),
        (
            "cargo test -p adder math::ops:: -- --nocapture".to_owned(),
            workspace
        )
    );
    assert_eq!(
        command(&files, "rust", &file(lib), lib, "/p").0,
        "cargo test -p adder --lib -- --nocapture"
    );
    let main = "/p/crates/adder/src/main.rs";
    assert_eq!(
        command(&files, "rust", &file(main), main, "/p").0,
        "cargo test -p adder --bin adder -- --nocapture"
    );
    let bin = "/p/crates/adder/src/bin/tool.rs";
    assert_eq!(
        command(&files, "rust", &adds, bin, "/p").0,
        "cargo test -p adder --bin tool tests::nested::adds -- --exact --nocapture"
    );
    assert_eq!(file(ops).name, "ops.rs");

    // A package of no workspace runs in its own directory.
    let alone = Files::default().with("/a/Cargo.toml", "[package]\nname = \"alone\"\n");
    assert_eq!(
        command(&alone, "rust", &test("one", "one"), "/a/src/lib.rs", "/a").1,
        PathBuf::from("/a")
    );

    // No package holds a file outside one, nor above the root.
    let loose = "/p/scripts/x.rs";
    let error = test_command(&files, "rust", &adds, Path::new(loose), Path::new("/p"));
    assert_eq!(error, Err("No Cargo package holds scripts/x.rs".to_owned()));
    let inner = "/p/crates/adder/src/lib.rs";
    let root = "/p/crates/adder/src";
    let error = test_command(&files, "rust", &adds, Path::new(inner), Path::new(root));
    assert_eq!(error, Err("No Cargo package holds lib.rs".to_owned()));
    let error = test_command(&files, "rust", &adds, Path::new("/q/a.rs"), Path::new("/p"));
    assert_eq!(error, Err("/q/a.rs isn't in the folder".to_owned()));
}

#[test]
fn cargo_command_for_an_integration_test_file() {
    let files = crate_files();
    let api = "/p/crates/adder/tests/api.rs";
    assert_eq!(
        command(&files, "rust", &test("lists", "lists"), api, "/p").0,
        "cargo test -p adder --test api lists -- --exact --nocapture"
    );
    assert_eq!(
        command(
            &files,
            "rust",
            &Runnable::file(Path::new(api), 3),
            api,
            "/p"
        )
        .0,
        "cargo test -p adder --test api -- --nocapture"
    );
    // A test of several files: its `main.rs`, and the modules beside it.
    let main = "/p/crates/adder/tests/suite/main.rs";
    assert_eq!(
        command(&files, "rust", &test("one", "one"), main, "/p").0,
        "cargo test -p adder --test suite one -- --exact --nocapture"
    );
    let part = "/p/crates/adder/tests/suite/part.rs";
    assert_eq!(
        command(&files, "rust", &test("one", "one"), part, "/p").0,
        "cargo test -p adder --test suite part::one -- --exact --nocapture"
    );
    let bench = "/p/crates/adder/benches/speed.rs";
    assert_eq!(
        command(
            &files,
            "rust",
            &Runnable::file(Path::new(bench), 3),
            bench,
            "/p"
        )
        .0,
        "cargo test -p adder --bench speed -- --nocapture"
    );
    // Nor a build script, which no target holds.
    let build = "/p/crates/adder/build.rs";
    let error = test_command(
        &files,
        "rust",
        &test("t", "t"),
        Path::new(build),
        Path::new("/p"),
    );
    assert_eq!(
        error,
        Err("Can't tell which crate crates/adder/build.rs is in".to_owned())
    );
}

/// The targets a manifest declares, by name or by path, over the ones Cargo
/// finds by their place.
#[test]
fn cargo_command_for_targets_the_manifest_names() {
    let manifest = "\
[package]
name = \"fastedit\"

[[bin]]
name = \"tenmoku\"
path = \"src/main.rs\"

[[test]]
name = \"integration\"
path = \"tests/it/main.rs\"

[[bench]]
name = \"speed\"
harness = false

[[example]]
name = \"demo\"
path = \"samples/demo.rs\"
";
    let files = (Files::default())
        .with("/f/Cargo.toml", manifest)
        .with("/f/src/main.rs", "")
        .with("/f/benches/speed.rs", "");
    let one = test("one", "tests::one");
    let file = |path: &str| Runnable::file(Path::new(path), 3);
    let run = |runnable: &Runnable, path: &str| command(&files, "rust", runnable, path, "/f").0;

    // The binary's name isn't the package's.
    assert_eq!(
        run(&file("/f/src/main.rs"), "/f/src/main.rs"),
        "cargo test -p fastedit --bin tenmoku -- --nocapture"
    );
    assert_eq!(
        run(&one, "/f/src/main.rs"),
        "cargo test -p fastedit --bin tenmoku tests::one -- --exact --nocapture"
    );
    // With no library, a module in `src` is the binary's.
    assert_eq!(
        run(&one, "/f/src/app/keys.rs"),
        "cargo test -p fastedit --bin tenmoku app::keys::tests::one -- --exact --nocapture"
    );
    // A test whose root is where the manifest says, and its modules.
    assert_eq!(
        run(&file("/f/tests/it/main.rs"), "/f/tests/it/main.rs"),
        "cargo test -p fastedit --test integration -- --nocapture"
    );
    assert_eq!(
        run(&one, "/f/tests/it/part.rs"),
        "cargo test -p fastedit --test integration part::tests::one -- --exact --nocapture"
    );
    assert_eq!(
        run(&file("/f/benches/speed.rs"), "/f/benches/speed.rs"),
        "cargo test -p fastedit --bench speed -- --nocapture"
    );
    // A root outside Cargo's places.
    assert_eq!(
        run(&file("/f/samples/demo.rs"), "/f/samples/demo.rs"),
        "cargo test -p fastedit --example demo -- --nocapture"
    );

    // A kind Cargo isn't to find by place has only the targets declared: a
    // file in `tests` is a module of the one test there.
    let declared = (Files::default())
        .with(
            "/k/Cargo.toml",
            "[package]\nname = \"shell\"\nautotests = false\n\n\
             [[test]]\nname = \"it\"\npath = \"tests/it.rs\"\n",
        )
        .with("/k/tests/it.rs", "");
    assert_eq!(
        command(&declared, "rust", &one, "/k/tests/status.rs", "/k").0,
        "cargo test -p shell --test it status::tests::one -- --exact --nocapture"
    );
    assert_eq!(
        command(
            &declared,
            "rust",
            &file("/k/tests/status.rs"),
            "/k/tests/status.rs",
            "/k"
        )
        .0,
        "cargo test -p shell --test it status:: -- --nocapture"
    );
    let no_bins = (Files::default())
        .with(
            "/n/Cargo.toml",
            "[package]\nname = \"nobin\"\nautobins = false\n",
        )
        .with("/n/src/lib.rs", "")
        .with("/n/src/main.rs", "");
    assert_eq!(
        command(&no_bins, "rust", &one, "/n/src/app.rs", "/n").0,
        "cargo test -p nobin app::tests::one -- --exact --nocapture"
    );

    // With a library too, a module in `src` can be either's, so it's named
    // by its path alone; and a library can have a root of its own.
    let both = (Files::default())
        .with("/b/Cargo.toml", "[package]\nname = \"both\"\n")
        .with("/b/src/lib.rs", "")
        .with("/b/src/main.rs", "");
    assert_eq!(
        command(&both, "rust", &one, "/b/src/app.rs", "/b").0,
        "cargo test -p both app::tests::one -- --exact --nocapture"
    );
    let custom = (Files::default()).with(
        "/c/Cargo.toml",
        "[package]\nname = \"custom\"\n\n[lib]\npath = \"lib/core.rs\"\n",
    );
    assert_eq!(
        command(
            &custom,
            "rust",
            &file("/c/lib/core.rs"),
            "/c/lib/core.rs",
            "/c"
        )
        .0,
        "cargo test -p custom --lib -- --nocapture"
    );
    assert_eq!(
        command(&custom, "rust", &one, "/c/lib/util.rs", "/c").0,
        "cargo test -p custom util::tests::one -- --exact --nocapture"
    );
}

#[test]
fn go_command_runs_one_test_by_anchored_name() {
    let files = Files::default().with("/g/go.mod", "module example.com/g\n");
    let adds = test("TestAdds", "TestAdds");
    let nested = "/g/pkg/adder/adder_test.go";
    // It runs in the package's directory, which a failure's lines name its
    // files from.
    assert_eq!(
        command(&files, "go", &adds, nested, "/g"),
        (
            "go test -run ^TestAdds$ .".to_owned(),
            PathBuf::from("/g/pkg/adder")
        )
    );
    assert_eq!(
        command(
            &files,
            "go",
            &Runnable::file(Path::new(nested), 4),
            nested,
            "/g"
        )
        .0,
        "go test ."
    );
    assert_eq!(
        command(&files, "go", &adds, "/g/adder_test.go", "/g").0,
        "go test -run ^TestAdds$ ."
    );
    let task = test_command(&files, "go", &adds, Path::new(nested), Path::new("/g")).unwrap();
    assert_eq!(task.label, "Test: TestAdds");
    assert_eq!(task.problem_matcher.as_deref(), Some("go"));
    assert_eq!(task.source, Source::File);

    let error = test_command(
        &Files::default(),
        "go",
        &adds,
        Path::new(nested),
        Path::new("/g"),
    );
    assert_eq!(
        error,
        Err("No go.mod holds pkg/adder/adder_test.go".to_owned())
    );
}

#[test]
fn pytest_command_for_a_class_method() {
    let files = Files::default();
    let path = "/py/tests/test_maths.py";
    let method = test("test_one", "TestMaths::test_one");
    assert_eq!(
        command(&files, "python", &method, path, "/py"),
        (
            "python -m pytest --tb=native tests/test_maths.py::TestMaths::test_one".to_owned(),
            PathBuf::from("/py")
        )
    );
    let class = runnable(RunnableKind::Module, "TestMaths", "TestMaths");
    assert_eq!(
        command(&files, "python", &class, path, "/py").0,
        "python -m pytest --tb=native tests/test_maths.py::TestMaths"
    );
    assert_eq!(
        command(
            &files,
            "python",
            &Runnable::file(Path::new(path), 9),
            path,
            "/py"
        )
        .0,
        "python -m pytest --tb=native tests/test_maths.py"
    );
    let task = test_command(&files, "python", &method, Path::new(path), Path::new("/py"));
    assert_eq!(task.unwrap().problem_matcher.as_deref(), Some("python"));

    let error = test_command(
        &files,
        "javascript",
        &method,
        Path::new("/py/a.test.js"),
        Path::new("/py"),
    );
    assert_eq!(error, Err("No test runner for javascript".to_owned()));
}

/// A failing test of each runner's, read from its output by the matcher its
/// task names, as the Problems panel lists them: (path, line, message).
#[test]
fn failing_tests_are_problems() {
    let problems = |task: &Task, output: &str, root: &str| {
        let matcher = match &task.problem_matcher {
            Some(name) => Matcher::from_name(name).unwrap(),
            None => Matcher::default_for(task.source).unwrap(),
        };
        let exists = |_: &Path| true;
        let found = match_output(matcher, output, &task.cwd, Path::new(root), &exists);
        (found.problems.into_iter())
            .map(|problem| (problem.path, problem.line, problem.message))
            .collect::<Vec<_>>()
    };

    // Cargo's paths are from the workspace's directory.
    let lib = Path::new("/p/crates/adder/src/lib.rs");
    let adds = test("adds", "tests::adds");
    let task = test_command(&crate_files(), "rust", &adds, lib, Path::new("/p")).unwrap();
    let output = "\
running 1 test
test tests::adds ... FAILED

failures:

---- tests::adds stdout ----

thread 'tests::adds' (3550665) panicked at crates/adder/src/lib.rs:12:9:
assertion `left == right` failed
  left: 3
 right: 4

failures:
    tests::adds

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
";
    assert_eq!(
        problems(&task, output, "/p"),
        [(
            lib.to_owned(),
            12,
            "tests::adds: assertion `left == right` failed".to_owned()
        )]
    );

    // Go's from the package's directory.
    let go = Files::default().with("/g/go.mod", "module example.com/g\n");
    let path = Path::new("/g/pkg/adder/adder_test.go");
    let task = test_command(
        &go,
        "go",
        &test("TestAdds", "TestAdds"),
        path,
        Path::new("/g"),
    );
    let output = "\
--- FAIL: TestAdds (0.00s)
    adder_test.go:12: expected 3, got 4
FAIL
FAIL\texample.com/g/pkg/adder\t0.002s
FAIL
";
    assert_eq!(
        problems(&task.unwrap(), output, "/g"),
        [(path.to_owned(), 12, "expected 3, got 4".to_owned())]
    );

    // pytest's, asked for as Python prints them.
    let path = Path::new("/py/tests/test_maths.py");
    let method = test("test_one", "TestMaths::test_one");
    let task = test_command(&Files::default(), "python", &method, path, Path::new("/py"));
    let output = "\
============================= test session starts ==============================
collected 1 item

tests/test_maths.py F                                                    [100%]

=================================== FAILURES ===================================
______________________________ TestMaths.test_one ______________________________
Traceback (most recent call last):
  File \"/py/tests/test_maths.py\", line 7, in test_one
    assert add(1, 2) == 4
AssertionError: assert 3 == 4
=========================== short test summary info ============================
FAILED tests/test_maths.py::TestMaths::test_one - assert 3 == 4
";
    assert_eq!(
        problems(&task.unwrap(), output, "/py"),
        [(
            path.to_owned(),
            7,
            "AssertionError: assert 3 == 4".to_owned()
        )]
    );
}

const LIB: &str = "\
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        assert_eq!(add(1, 2), 3);
    }
}
";

struct Project<'a> {
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    dir: tempfile::TempDir,
}

/// The disk.
struct Disk;

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// Turns rust-analyzer off in the folder `root`, whose Rust files would
/// start it.
fn no_rust_analyzer(root: &Path) {
    let settings = fe_config::project_settings_path(root);
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.rust-analyzer]\nenabled = false\n").unwrap();
}

/// A workbench showing [`LIB`] as a package's `src/lib.rs`, in a temporary
/// folder, focused as the app focuses it, whose terminal panel starts no
/// process. The root isn't trusted.
fn untrusted(cx: &mut TestAppContext) -> Project<'_> {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    let path = dir.path().join("src/lib.rs");
    fs::write(&path, LIB).unwrap();
    no_rust_analyzer(dir.path());
    cx.update(|cx| {
        cx.set_global(TestFiles(Arc::new(Disk)));
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new(LIB).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    Project { workbench, cx, dir }
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
    fn run(&mut self, id: &str) {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run = RunCommand(Invocation::of(id).unwrap().context(context));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        draw(self.cx);
    }

    /// Puts the editor's caret at the start of `needle` in [`LIB`].
    fn caret_at(&mut self, needle: &str) {
        let offset = LIB.find(needle).expect("the text has it");
        let editor = (self.workbench)
            .read_with(self.cx, |workbench, cx| {
                workbench.active_editor(cx).cloned()
            })
            .expect("an editor shows");
        editor.update(self.cx, |view, _| view.model_mut().set_cursor(offset));
        draw(self.cx);
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

    /// What each session started ran, and where.
    fn started(&mut self) -> Vec<(String, PathBuf)> {
        terminal(&self.workbench, self.cx).read_with(self.cx, |terminal, _| {
            (terminal.started_options.iter())
                .map(|options| {
                    let (program, args) = options.shell.clone().expect("a task's command");
                    let line = [vec![program], args].concat().join(" ");
                    (line, options.cwd.clone())
                })
                .collect()
        })
    }

    fn last_test(&mut self) -> Option<String> {
        (self.workbench).update(self.cx, |workbench, cx| {
            workbench.last_test(cx).map(|task| task.label)
        })
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

const ADDS: &str = "cargo test -p p tests::adds -- --exact --nocapture";

#[gpui::test]
fn run_at_cursor_starts_the_task_in_a_terminal_tab(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    let root = project.dir.path().to_owned();

    project.caret_at("assert_eq!");
    project.run("tests.run_at_cursor");
    assert_eq!(project.tabs(), ["Test: adds"]);
    assert_eq!(project.started(), [(ADDS.to_owned(), root.clone())]);
    assert_eq!(project.last_test().as_deref(), Some("Test: adds"));
    let dock = project
        .workbench
        .read_with(project.cx, |workbench, _| workbench.terminal_dock().clone());
    assert!(dock.read_with(project.cx, |dock, _| dock.is_visible()));
    assert_eq!(project.toasts(), []);

    // Between the module's tests, the module.
    project.caret_at("use super");
    project.run("tests.run_at_cursor");
    assert_eq!(project.tabs(), ["Test: adds", "Test: tests"]);
    let module = "cargo test -p p tests:: -- --nocapture";
    assert_eq!(project.started()[1], (module.to_owned(), root.clone()));

    // The file, wherever the caret is.
    project.caret_at("a + b");
    project.run("tests.run_file");
    assert_eq!(
        project.tabs(),
        ["Test: adds", "Test: tests", "Test: lib.rs"]
    );
    let file = "cargo test -p p --lib -- --nocapture";
    assert_eq!(project.started()[2], (file.to_owned(), root.clone()));
    assert_eq!(project.last_test().as_deref(), Some("Test: lib.rs"));

    // The palette has them, focus going back to the editor to run there.
    let editor = (project.workbench)
        .read_with(project.cx, |workbench, cx| {
            workbench.active_editor(cx).cloned()
        })
        .expect("an editor shows");
    (project.cx).update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    project.caret_at("fn adds");
    project.cx.simulate_keystrokes("cmd-shift-p");
    draw(project.cx);
    project.cx.simulate_input("Run Test at Cursor");
    draw(project.cx);
    project.cx.simulate_keystrokes("enter");
    draw(project.cx);
    assert_eq!(project.started().len(), 4);
    assert_eq!(project.started()[3], (ADDS.to_owned(), root));
}

#[gpui::test]
fn rerun_last_repeats_it(cx: &mut TestAppContext) {
    let mut project = trusted(cx);

    // None yet.
    project.run("tests.rerun_last");
    assert_eq!(project.toasts(), [(Level::Info, no_test_run())]);
    assert_eq!(project.tabs(), Vec::<String>::new());

    project.caret_at("fn adds");
    project.run("tests.run_at_cursor");
    let before = project.views();

    // The last test again, wherever the caret is now: its finished tab, with a
    // new session.
    project.exit_active();
    project.caret_at("pub fn add");
    project.run("tests.rerun_last");
    assert_eq!(project.tabs(), ["Test: adds"]);
    let after = project.views();
    assert_eq!(after.len(), 1);
    assert_ne!(after, before);
    let started: Vec<String> = (project.started().into_iter())
        .map(|(line, _)| line)
        .collect();
    assert_eq!(started, [ADDS, ADDS]);

    // A task run since isn't a test: the test is still the one to rerun.
    fs::write(project.dir.path().join("Makefile"), "all:\n\ttrue\n").unwrap();
    project.run("tasks.run");
    project.cx.simulate_input("make all");
    draw(project.cx);
    project.cx.simulate_keystrokes("enter");
    draw(project.cx);
    assert_eq!(project.tabs(), ["Test: adds", "make all"]);
    project.run("tests.rerun_last");
    assert_eq!(project.tabs(), ["Test: adds", "make all", "Test: adds (2)"]);
    assert_eq!(project.started()[3].0, ADDS);

    // A new root has its own. The Rust file closes first, as outside the
    // root it would start rust-analyzer on its own.
    project.run("workbench.close_tab");
    let other = tempfile::tempdir().unwrap();
    let (workbench, root) = (project.workbench.clone(), other.path().to_owned());
    project.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx));
    });
    draw(project.cx);
    assert_eq!(project.last_test(), None);
}

#[gpui::test]
fn no_test_at_the_cursor_toasts(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    project.caret_at("a + b");
    project.run("tests.run_at_cursor");
    assert_eq!(project.toasts(), [(Level::Info, no_test_here())]);
    assert_eq!(project.tabs(), Vec::<String>::new());
    assert_eq!(project.last_test(), None);

    // A test in no package says so, and runs nothing.
    fs::remove_file(project.dir.path().join("Cargo.toml")).unwrap();
    project.caret_at("fn adds");
    project.run("tests.run_at_cursor");
    let toasts = project.toasts();
    assert_eq!(
        toasts.last(),
        Some(&(Level::Info, "No Cargo package holds src/lib.rs".to_owned()))
    );
    assert_eq!(project.tabs(), Vec::<String>::new());
}

#[gpui::test]
fn untrusted_folder_gets_the_trust_toast(cx: &mut TestAppContext) {
    let mut project = untrusted(cx);
    project.caret_at("fn adds");
    project.run("tests.run_at_cursor");
    assert_eq!(project.toasts(), [(Level::Warning, untrusted_message())]);
    assert_eq!(project.tabs(), Vec::<String>::new());
    // Refused, it isn't the test to rerun.
    assert_eq!(project.last_test(), None);

    // Trusted, it runs.
    (project.workbench).update(project.cx, |workbench, cx| {
        workbench.set_root_trust(true, cx)
    });
    draw(project.cx);
    project.run("tests.run_at_cursor");
    assert_eq!(project.tabs(), ["Test: adds"]);
}

/// A test runs what's on disk, so the window's files are saved first: a test
/// added and not saved yet runs, and so does a rerun's edit.
#[gpui::test]
fn unsaved_edits_are_saved_before_the_run(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    let lib = project.dir.path().join("src/lib.rs");
    let editor = (project.workbench)
        .read_with(project.cx, |workbench, cx| {
            workbench.active_editor(cx).cloned()
        })
        .expect("an editor shows");
    let added = "\n#[test]\nfn subtracts() {}\n";
    editor.update(project.cx, |view, _| {
        let model = view.model_mut();
        model.set_cursor(LIB.len());
        model.insert(added);
        // Inside its body.
        model.set_cursor(LIB.len() + added.len() - 2);
    });
    draw(project.cx);
    project.run("tests.run_at_cursor");
    assert_eq!(fs::read_to_string(&lib).unwrap(), format!("{LIB}{added}"));
    assert_eq!(project.tabs(), ["Test: subtracts"]);
    let subtracts = "cargo test -p p subtracts -- --exact --nocapture";
    assert_eq!(project.started()[0].0, subtracts);
    assert_eq!(project.toasts(), []);

    project.exit_active();
    editor.update(project.cx, |view, _| {
        let model = view.model_mut();
        model.set_cursor(0);
        model.insert("// Adds.\n");
    });
    draw(project.cx);
    project.run("tests.rerun_last");
    let saved = fs::read_to_string(&lib).unwrap();
    assert!(saved.starts_with("// Adds.\npub fn add"), "{saved}");
    assert_eq!(project.started().len(), 2);
    assert_eq!(project.started()[1].0, subtracts);
}
