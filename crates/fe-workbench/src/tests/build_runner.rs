//! A Rust project's build as a run ([`crate::build_runner`]): `runs.build` on
//! a real project in a temporary folder, its program a fake `cargo`
//! (`tests/fixtures/fake_cargo_build.sh`) on the `PATH` it's given, which
//! prints recorded `cargo build --message-format=json-render-diagnostics`
//! output and records its arguments.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{CommandId, ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_l10n::tr;
use serde_json::Value;

use crate::{
    Launch, Workbench,
    build_runner::no_cargo_project,
    dock::problems_panel::ShownRow,
    runs::{Run, RunKind, RunStatus, RunSummary},
    task_commands::TestFiles,
    task_defs::ProjectFiles,
    task_run::untrusted_message,
    test_runner::RunnerPath,
    tests::{open, test_runner::install_fake_cargo},
    workbench::toast::Level,
};

/// Package `p`'s library, as the fake's diagnostics have it: a warning at
/// 2:9 and an error at 6:5.
const LIB: &str = "\
pub fn one() -> u32 {
    let x = 1;
    1
}
pub fn two() -> u32 {
    \"two\"
}
";

/// The fake's arguments for a build.
const BUILD: &str = "build --workspace --message-format=json-render-diagnostics";

/// The disk, as far as the project's folder: not a `Cargo.toml` in a folder
/// above the temporary one.
struct Disk(PathBuf);

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.starts_with(&self.0) && path.is_file()
    }
}

struct Project<'a> {
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    dir: tempfile::TempDir,
    /// The fake `cargo`'s folder, first on the runs' `PATH`.
    bin: tempfile::TempDir,
}

/// A workbench showing [`LIB`] as package `p`'s `src/lib.rs`, in a temporary
/// folder, with the fake `cargo` first on its runs' `PATH`. The root isn't
/// trusted.
fn untrusted(cx: &mut TestAppContext) -> Project<'_> {
    // The runs' programs are real processes, read on threads of their own.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    let path = dir.path().join("src/lib.rs");
    fs::write(&path, LIB).unwrap();
    // Its Rust file would start rust-analyzer.
    let settings = fe_config::project_settings_path(dir.path());
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.rust-analyzer]\nenabled = false\n").unwrap();

    let bin = tempfile::tempdir().unwrap();
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_cargo_build.sh");
    let runner_path = install_fake_cargo(bin.path(), &fs::read_to_string(fake).unwrap());
    let files = Disk(dir.path().to_owned());
    cx.update(|cx| {
        cx.set_global(TestFiles(Arc::new(files)));
        cx.set_global(RunnerPath(runner_path));
    });

    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new(LIB).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    Project {
        workbench,
        cx,
        dir,
        bin,
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

impl Project<'_> {
    fn run(&mut self, id: &str) {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run = RunCommand(Invocation::of(id).unwrap().context(context));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        draw(self.cx);
    }

    /// The build's title in this project's folder.
    fn title(&self) -> String {
        let folder = self.dir.path().file_name().unwrap().to_string_lossy();
        tr!("workbench-build-runs-title", folder = folder).into()
    }

    /// The workbench's runs, newest first.
    fn runs(&mut self) -> Vec<Run> {
        let runs = (self.workbench).read_with(self.cx, |workbench, _| workbench.runs().clone());
        runs.read_with(self.cx, |runs, _| runs.list().cloned().collect())
    }

    /// Runs the app until `done`, which real threads bring about, for up to
    /// 60 s.
    fn wait_until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let give_up = Instant::now() + Duration::from_secs(60);
        loop {
            draw(self.cx);
            if done(self) {
                return;
            }
            assert!(Instant::now() < give_up, "gave up waiting: {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits for the newest run to end, and gives it.
    fn finished(&mut self) -> Run {
        self.wait_until("the run to end", |project| {
            (project.runs().first()).is_some_and(|run| !run.status.is_running())
        });
        self.runs().remove(0)
    }

    /// The fake `cargo`'s arguments, a run a line.
    fn args(&self) -> Vec<String> {
        let args = fs::read_to_string(self.bin.path().join("args")).unwrap_or_default();
        args.lines().map(str::to_owned).collect()
    }

    fn toasts(&mut self) -> Vec<(Level, String)> {
        let layer = (self.workbench).read_with(self.cx, |workbench, _| workbench.toasts().clone());
        layer.read_with(self.cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| (entry.toast.level, entry.toast.message.clone()))
                .collect()
        })
    }

    /// The Problems panel's rows.
    fn problems(&mut self) -> Vec<ShownRow> {
        let panel = (self.workbench).read_with(self.cx, |workbench, cx| {
            workbench.dock().read(cx).problems().clone()
        });
        panel.read_with(self.cx, |panel, _| panel.shown_rows())
    }

    /// The lines of the output the build `run` kept.
    fn output(&mut self, run: &Run) -> Vec<String> {
        let process = (self.workbench)
            .read_with(self.cx, |workbench, cx| workbench.build_process(run.id, cx))
            .expect("its program");
        process.read_with(self.cx, |process, _| {
            (process.output().lines())
                .map(|line| line.text.clone())
                .collect()
        })
    }
}

fn build(crates: usize, warnings: usize, errors: usize) -> RunSummary {
    RunSummary::Build {
        crates,
        warnings,
        errors,
    }
}

fn file(path: &str, problems: usize) -> ShownRow {
    ShownRow::File(path.to_owned(), problems)
}

fn problem(mark: &'static str, message: &str, source: &str, position: &str) -> ShownRow {
    let source = Some(source.to_owned());
    ShownRow::Problem(mark, message.to_owned(), source, position.to_owned())
}

/// The warning the fake's `p` has, as the panel lists it from `source`.
fn unused_x(source: &str) -> ShownRow {
    problem("W", "unused variable: `x`", source, "2:9")
}

/// The error the fake's `p` has unless it's `fixed`.
fn mismatched(source: &str) -> ShownRow {
    let message = tr!(
        "workbench-lsp-tasks-problem-code",
        message = "mismatched types",
        code = "E0308"
    );
    problem("E", &message, source, "6:5")
}

#[gpui::test]
fn the_summary_follows_artifacts(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    // The fake stops after `ryu`'s artifact until `go` is there.
    fs::write(project.bin.path().join("gate"), "").unwrap();

    project.run("runs.build");
    project.wait_until("the first artifacts", |project| {
        let summary = project.runs().first().map(|run| run.summary);
        summary == Some(build(2, 0, 0))
    });
    let run = project.runs().remove(0);
    assert_eq!(run.kind, RunKind::Build);
    assert_eq!(run.title, project.title());
    // How far along a build is isn't known.
    assert_eq!(run.status, RunStatus::Running { progress: None });
    assert_eq!(project.args(), [BUILD]);

    fs::write(project.bin.path().join("go"), "").unwrap();
    let run = project.finished();
    assert_eq!(run.status, RunStatus::Failed { code: Some(101) });
    // `p` had no artifact: its error stopped it.
    assert_eq!(run.summary, build(2, 1, 1));

    // Its rendered diagnostics are kept for the Output view.
    let output = project.output(&run);
    for line in [
        "warning: unused variable: `x`",
        "error[E0308]: mismatched types",
        " --> src/lib.rs:6:5",
    ] {
        assert!(output.contains(&line.to_owned()), "{line}: {output:?}");
    }
    // And the build, as the parser read it.
    let timeline = (project.workbench)
        .read_with(project.cx, |workbench, cx| {
            workbench.build_timeline(run.id, cx)
        })
        .expect("its timeline");
    // In the order the two pipes' lines were read in, which may be either.
    let mut crates: Vec<(&str, bool)> = (timeline.spans.iter())
        .map(|span| (span.name.as_str(), span.fresh))
        .collect();
    crates.sort();
    assert_eq!(crates, [("itoa", true), ("p", false), ("ryu", false)]);
    assert!(!timeline.success);

    // It builds once fixed.
    fs::write(project.bin.path().join("fixed"), "").unwrap();
    project.run("runs.build");
    let run = project.finished();
    assert_eq!(run.status, RunStatus::Passed);
    assert_eq!(run.summary, build(3, 1, 0));
    assert_eq!(project.args(), [BUILD, BUILD]);

    // It isn't a task: no terminal tab shows it.
    let terminal =
        (project.workbench).read_with(project.cx, |workbench, _| workbench.terminal().clone());
    assert_eq!(terminal.read_with(project.cx, |t, _| t.views().count()), 0);
    assert_eq!(project.toasts(), []);
}

#[gpui::test]
fn compiler_messages_become_problems_and_replace_the_last_builds(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    let title = project.title();
    assert_eq!(project.problems(), []);

    project.run("runs.build");
    project.finished();
    let both = vec![file("src/lib.rs", 2), unused_x(&title), mismatched(&title)];
    project.wait_until("the build's problems", |project| project.problems() == both);

    // The next build's replace them.
    fs::write(project.bin.path().join("fixed"), "").unwrap();
    project.run("runs.build");
    project.finished();
    let one = vec![file("src/lib.rs", 1), unused_x(&title)];
    project.wait_until("the next build's problems", |project| {
        project.problems() == one
    });

    // And so on, back again.
    fs::remove_file(project.bin.path().join("fixed")).unwrap();
    project.run("runs.build");
    project.finished();
    project.wait_until("the third build's problems", |project| {
        project.problems() == both
    });
    assert_eq!(project.toasts(), []);
}

#[gpui::test]
fn a_root_without_cargo_toml_says_so(cx: &mut TestAppContext) {
    let catalog = crate::command_catalog();
    let command = CommandId::parse("runs.build").unwrap();
    assert_eq!(catalog.check(&command, &Value::Null), Ok(()));
    let title = fe_command::l10n::command_label("runs.build", "");
    assert_eq!(title, tr!("command-runs-build").to_string());
    assert_eq!(title, "Runs: Build Project");

    let mut project = trusted(cx);
    fs::remove_file(project.dir.path().join("Cargo.toml")).unwrap();
    project.run("runs.build");
    project.wait_until("the toast", |project| !project.toasts().is_empty());
    assert_eq!(project.toasts(), [(Level::Info, no_cargo_project())]);
    assert_eq!(project.runs(), []);
    assert_eq!(project.args(), Vec::<String>::new());

    // One in a folder above the root will do: the root is a package in it.
    let member = project.dir.path().join("src");
    fs::write(project.dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let plan = crate::build_runner::plan(&Disk(project.dir.path().to_owned()), &member, None)
        .expect("a plan");
    assert_eq!(plan.spec.cwd, member);
    assert_eq!(plan.manifest_dirs, [project.dir.path().to_owned()]);
}

#[gpui::test]
fn files_are_saved_first(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    let lib = project.dir.path().join("src/lib.rs");
    let editor = (project.workbench)
        .read_with(project.cx, |workbench, cx| {
            workbench.active_editor(cx).cloned()
        })
        .expect("an editor shows");
    editor.update(project.cx, |view, _| {
        let model = view.model_mut();
        model.set_cursor(0);
        model.insert("// One.\n");
    });
    draw(project.cx);

    project.run("runs.build");
    // Saved by the time it started.
    project.wait_until("the run to start", |project| !project.args().is_empty());
    assert_eq!(fs::read_to_string(&lib).unwrap(), format!("// One.\n{LIB}"));
    project.finished();
    assert_eq!(project.args(), [BUILD]);
    assert_eq!(project.toasts(), []);
}

#[gpui::test]
fn an_untrusted_root_refuses(cx: &mut TestAppContext) {
    let mut project = untrusted(cx);
    project.run("runs.build");
    assert_eq!(project.toasts(), [(Level::Warning, untrusted_message())]);
    // Nothing ran, nor began a run.
    assert_eq!(project.runs(), []);
    assert_eq!(project.args(), Vec::<String>::new());

    // Trusted, it runs.
    (project.workbench).update(project.cx, |workbench, cx| {
        workbench.set_root_trust(true, cx)
    });
    draw(project.cx);
    project.run("runs.build");
    let run = project.finished();
    assert_eq!(run.kind, RunKind::Build);
    assert_eq!(run.status, RunStatus::Failed { code: Some(101) });
    assert_eq!(project.args(), [BUILD]);
}
