//! Rust tests run as runs ([`crate::test_runner`]): `runs.test_file` and
//! `runs.test_project` on a real project in a temporary folder, their
//! program a fake `cargo` (`tests/fixtures/fake_cargo_test.sh`) on the
//! `PATH` they're given, which prints recorded `cargo test` or `cargo nextest`
//! output and records its arguments.

use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{CommandId, ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_runlog::{History, Outcome, TestId};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_l10n::tr;
use serde_json::Value;

use crate::{
    Launch, Workbench,
    run_process::MAX_LINE,
    runs::{MAX_FINISHED, RunEnd},
    runs::{Run, RunKind, RunStatus, RunSummary},
    task_commands::TestFiles,
    task_defs::ProjectFiles,
    task_run::untrusted_message,
    test_runner::{
        DelayHistoryWrites, RunnerPath, Scope, TestOutcome, TestResult, no_cargo_project, plan,
        rust_only, set_history_dir,
    },
    tests::open,
    workbench::toast::Level,
};

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

    #[test]
    fn fails() {
        assert_eq!(add(1, 2), 4);
    }
}
";

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

struct Project<'a> {
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    dir: tempfile::TempDir,
    /// The fake `cargo`'s folder, first on the runs' `PATH`.
    bin: tempfile::TempDir,
    /// The state directory the test histories go under.
    state: tempfile::TempDir,
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
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_cargo_test.sh");
    let cargo = bin.path().join("cargo");
    fs::copy(fake, &cargo).unwrap();
    executable(&cargo);
    // The fake, then the system's tools it runs: not the `cargo-nextest` a
    // developer has installed.
    let path_dirs = [bin.path(), Path::new("/usr/bin"), Path::new("/bin")];
    let runner_path = std::env::join_paths(path_dirs).unwrap();
    let state = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        cx.set_global(TestFiles(Arc::new(Disk)));
        cx.set_global(RunnerPath(runner_path));
        set_history_dir(cx, state.path().to_owned());
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
        state,
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

#[cfg(unix)]
fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn executable(_: &Path) {}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

const ADDS: &str = "tests::adds";
const FAILS: &str = "tests::fails";

/// What the fake `cargo` printed for `tests::fails`, as libtest captured it.
const FAILS_OUTPUT: &str = "\
thread 'tests::fails' panicked at src/lib.rs:16:9:
assertion `left == right` failed
  left: 3
 right: 4
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
";

impl Project<'_> {
    fn run(&mut self, id: &str) {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run = RunCommand(Invocation::of(id).unwrap().context(context));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        draw(self.cx);
    }

    fn root(&mut self) -> PathBuf {
        (self.workbench).read_with(self.cx, |workbench, _| {
            workbench.root().expect("a root").to_owned()
        })
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

    fn results(&mut self, run: &Run) -> Vec<TestResult> {
        (self.workbench)
            .read_with(self.cx, |workbench, cx| workbench.test_results(run.id, cx))
            .expect("a test run's results")
    }

    /// The fake `cargo`'s arguments, a run a line.
    fn args(&self) -> Vec<String> {
        let args = fs::read_to_string(self.bin.path().join("args")).unwrap_or_default();
        args.lines().map(str::to_owned).collect()
    }

    fn history(&mut self) -> History {
        let root = self.root();
        History::load(self.state.path(), &root)
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

fn id(name: &str) -> TestId {
    TestId::new("p", name)
}

const TESTS: &str = "test -p p --lib --no-fail-fast --message-format json-render-diagnostics";

#[gpui::test]
fn a_file_run_reports_live_counts_and_finishes(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    // The fake stops after its first result until `go` is there.
    fs::write(project.bin.path().join("gate"), "").unwrap();

    project.run("runs.test_file");
    project.wait_until("the first result", |project| {
        let runs = project.runs();
        let summary = runs.first().map(|run| run.summary);
        summary
            == Some(RunSummary::Tests {
                passed: 1,
                failed: 0,
                total: Some(2),
            })
    });
    let run = project.runs().remove(0);
    assert_eq!(run.kind, RunKind::Test);
    assert_eq!(run.title, "Test: lib.rs");
    assert_eq!(
        run.status,
        RunStatus::Running {
            progress: Some(0.5)
        }
    );
    assert_eq!(project.args(), [TESTS]);
    let so_far = project.results(&run);
    assert_eq!(so_far.len(), 1);
    assert_eq!(
        (so_far[0].id.clone(), so_far[0].outcome),
        (id(ADDS), TestOutcome::Passed)
    );

    fs::write(project.bin.path().join("go"), "").unwrap();
    let run = project.finished();
    assert_eq!(run.status, RunStatus::Failed { code: Some(101) });
    assert_eq!(
        run.summary,
        RunSummary::Tests {
            passed: 1,
            failed: 1,
            total: Some(2),
        }
    );
    let results: Vec<(TestId, TestOutcome, String)> = (project.results(&run).into_iter())
        .map(|result| (result.id, result.outcome, result.output))
        .collect();
    assert_eq!(
        results,
        [
            (id(ADDS), TestOutcome::Passed, String::new()),
            (id(FAILS), TestOutcome::Failed, FAILS_OUTPUT.to_owned()),
        ]
    );
    // Its output is kept for the Output view.
    let process = (project.workbench)
        .read_with(project.cx, |workbench, cx| {
            workbench.test_process(run.id, cx)
        })
        .expect("its program");
    let output: Vec<String> = process.read_with(project.cx, |process, _| {
        (process.output().lines())
            .map(|line| line.text.clone())
            .collect()
    });
    assert!(
        output.contains(&"test tests::adds ... ok".to_owned()),
        "{output:?}"
    );
    // It isn't a task: no terminal tab shows it.
    let terminal =
        (project.workbench).read_with(project.cx, |workbench, _| workbench.terminal().clone());
    assert_eq!(terminal.read_with(project.cx, |t, _| t.views().count()), 0);
    assert_eq!(project.toasts(), []);
}

#[gpui::test]
fn results_are_recorded_in_the_history(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    assert!(project.history().is_empty());

    project.run("runs.test_file");
    project.finished();
    project.wait_until("the history to be saved", |project| {
        project.history().len() == 2
    });
    let history = project.history();
    assert_eq!(history.last8(&id(ADDS)), [Outcome::Passed]);
    assert_eq!(history.last8(&id(FAILS)), [Outcome::Failed]);

    // The project's tests, all of them: the same tests, a second result each,
    // and the integration test's first.
    project.run("runs.test_project");
    let run = project.finished();
    let folder = project
        .root()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        run.title,
        tr!("workbench-test-runs-project-title", folder = folder).to_string()
    );
    assert_eq!(
        project.args()[1],
        "test --workspace --no-fail-fast --message-format json-render-diagnostics"
    );
    project.wait_until("the second run to be saved", |project| {
        project.history().last8(&id(ADDS)).len() == 2
    });
    let history = project.history();
    assert_eq!(history.last8(&id(ADDS)), [Outcome::Passed, Outcome::Passed]);
    assert_eq!(
        history.last8(&id(FAILS)),
        [Outcome::Failed, Outcome::Failed]
    );
    assert_eq!(
        history.last8(&TestId::new("more", "subtracts")),
        [Outcome::Passed]
    );
    assert_eq!(history.len(), 3);
    // Two runs, each its own results.
    let runs = project.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(project.results(&runs[1]).len(), 2);
}

#[gpui::test]
fn nextest_is_used_when_the_root_configures_it(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    let nextest = project.bin.path().join("cargo-nextest");

    // Configured, but not on the PATH: cargo test.
    let config = project.dir.path().join(".config");
    fs::create_dir(&config).unwrap();
    fs::write(config.join("nextest.toml"), "[profile.default]\n").unwrap();
    project.run("runs.test_file");
    project.finished();
    assert_eq!(project.args(), [TESTS]);

    // On the PATH and configured: nextest, read as libtest-json.
    fs::write(&nextest, "#!/bin/sh\n").unwrap();
    executable(&nextest);
    project.run("runs.test_file");
    let run = project.finished();
    let nextest_run = "nextest run -p p --lib --no-fail-fast --message-format libtest-json";
    assert_eq!(project.args()[1], nextest_run);
    assert_eq!(run.status, RunStatus::Failed { code: Some(100) });
    assert_eq!(
        run.summary,
        RunSummary::Tests {
            passed: 1,
            failed: 1,
            total: Some(2),
        }
    );
    let results: Vec<(TestId, TestOutcome, String)> = (project.results(&run).into_iter())
        .map(|result| (result.id, result.outcome, result.output))
        .collect();
    assert_eq!(
        results,
        [
            (id(ADDS), TestOutcome::Passed, String::new()),
            (id(FAILS), TestOutcome::Failed, FAILS_OUTPUT.to_owned()),
        ]
    );

    // On the PATH, not configured: cargo test.
    fs::remove_dir_all(&config).unwrap();
    project.run("runs.test_project");
    project.finished();
    assert_eq!(
        project.args()[2],
        "test --workspace --no-fail-fast --message-format json-render-diagnostics"
    );

    // Configured in the workspace's metadata: nextest.
    let manifest = "[package]\nname = \"p\"\n\n[workspace.metadata.nextest]\n";
    fs::write(project.dir.path().join("Cargo.toml"), manifest).unwrap();
    project.run("runs.test_project");
    project.finished();
    assert_eq!(
        project.args()[3],
        "nextest run --workspace --no-fail-fast --message-format libtest-json"
    );
}

/// A test runs what's on disk, so the window's files are saved first.
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
        model.insert("// Adds.\n");
    });
    draw(project.cx);

    project.run("runs.test_file");
    // Saved by the time it started.
    project.wait_until("the run to start", |project| !project.args().is_empty());
    assert_eq!(
        fs::read_to_string(&lib).unwrap(),
        format!("// Adds.\n{LIB}")
    );
    project.finished();
    assert_eq!(project.args(), [TESTS]);
    assert_eq!(project.toasts(), []);

    // So with the project's.
    editor.update(project.cx, |view, _| {
        let model = view.model_mut();
        model.set_cursor(0);
        model.insert("// Sums.\n");
    });
    draw(project.cx);
    project.run("runs.test_project");
    project.wait_until("the run to start", |project| project.args().len() == 2);
    assert!(
        (fs::read_to_string(&lib).unwrap()).starts_with("// Sums.\n// Adds.\n"),
        "saved first"
    );
    project.finished();
}

#[gpui::test]
fn an_untrusted_root_refuses(cx: &mut TestAppContext) {
    let mut project = untrusted(cx);
    project.run("runs.test_file");
    assert_eq!(project.toasts(), [(Level::Warning, untrusted_message())]);
    // The same toast again, shown once.
    project.run("runs.test_project");
    assert_eq!(project.toasts(), [(Level::Warning, untrusted_message())]);
    // Nothing ran, nor began a run.
    assert_eq!(project.runs(), []);
    assert_eq!(project.args(), Vec::<String>::new());

    // Trusted, it runs.
    (project.workbench).update(project.cx, |workbench, cx| {
        workbench.set_root_trust(true, cx)
    });
    draw(project.cx);
    project.run("runs.test_file");
    let run = project.finished();
    assert_eq!(run.status, RunStatus::Failed { code: Some(101) });
    assert_eq!(project.args(), [TESTS]);
}

#[gpui::test]
fn the_commands_have_titles(cx: &mut TestAppContext) {
    let catalog = crate::command_catalog();
    for (id, key) in [
        ("runs.test_file", "command-runs-test_file"),
        ("runs.test_project", "command-runs-test_project"),
    ] {
        let command = CommandId::parse(id).unwrap();
        assert_eq!(catalog.check(&command, &Value::Null), Ok(()), "{id}");
        let title = fe_command::l10n::command_label(id, "");
        assert_eq!(title, tr!(key).to_string(), "{id}");
        assert!(title.starts_with("Runs: "), "{title}");
    }

    // What they say when they can't run.
    let mut project = trusted(cx);
    fs::remove_file(project.dir.path().join("Cargo.toml")).unwrap();
    project.run("runs.test_project");
    assert_eq!(
        project.toasts().last(),
        Some(&(Level::Info, no_cargo_project()))
    );
    let notes = project.dir.path().join("notes.py");
    fs::write(&notes, "def test_one():\n    pass\n").unwrap();
    let notes = dunce::canonicalize(notes).unwrap();
    let workbench = project.workbench.clone();
    project.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_range(notes, 0..0, window, cx)
        })
    });
    project.wait_until("the Python file's language", |project| {
        project.run("runs.test_file");
        let said = project.toasts().last().map(|(_, message)| message.clone());
        said == Some(rust_only("Python"))
    });
    assert_eq!(project.runs(), []);
    assert_eq!(project.args(), Vec::<String>::new());
}

/// Cargo writes a binary's `Running` line to stderr and its tests' results
/// to stdout: read in the order they were written, each result is its own
/// binary's.
#[gpui::test]
fn results_keep_their_binaries_across_streams(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    for _ in 0..3 {
        project.run("runs.test_project");
        let run = project.finished();
        assert_eq!(
            run.summary,
            RunSummary::Tests {
                passed: 2,
                failed: 1,
                total: Some(3),
            }
        );
        let results: Vec<(TestId, TestOutcome)> = (project.results(&run).into_iter())
            .map(|result| (result.id, result.outcome))
            .collect();
        assert_eq!(
            results,
            [
                (id(ADDS), TestOutcome::Passed),
                (id(FAILS), TestOutcome::Failed),
                (TestId::new("more", "subtracts"), TestOutcome::Passed),
            ]
        );
    }
}

/// A binary that aborts after a test's `FAILED` writes neither its output
/// nor its report: the failure still counts once the output ends, and is
/// recorded.
#[gpui::test]
fn an_aborted_binary_still_counts_its_failure(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    fs::write(project.bin.path().join("abort"), "").unwrap();
    project.run("runs.test_file");
    let run = project.finished();
    assert_eq!(run.status, RunStatus::Failed { code: Some(101) });
    assert_eq!(
        run.summary,
        RunSummary::Tests {
            passed: 1,
            failed: 1,
            total: Some(2),
        }
    );
    let results: Vec<(TestId, TestOutcome, String)> = (project.results(&run).into_iter())
        .map(|result| (result.id, result.outcome, result.output))
        .collect();
    assert_eq!(
        results,
        [
            (id(ADDS), TestOutcome::Passed, String::new()),
            (id(FAILS), TestOutcome::Failed, String::new()),
        ]
    );
    project.wait_until("the history to be saved", |project| {
        project.history().len() == 2
    });
    assert_eq!(project.history().last8(&id(FAILS)), [Outcome::Failed]);
}

/// nextest writes a test's event on one line however much it printed: one
/// longer than the runner reads as one is put back together.
#[gpui::test]
fn a_failure_longer_than_a_line_is_read_whole(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    let nextest = project.bin.path().join("cargo-nextest");
    fs::write(&nextest, "#!/bin/sh\n").unwrap();
    executable(&nextest);
    let config = project.dir.path().join(".config");
    fs::create_dir(&config).unwrap();
    fs::write(config.join("nextest.toml"), "").unwrap();
    fs::write(project.bin.path().join("big"), "").unwrap();

    project.run("runs.test_file");
    let run = project.finished();
    assert_eq!(
        run.summary,
        RunSummary::Tests {
            passed: 1,
            failed: 1,
            total: Some(2),
        }
    );
    let results = project.results(&run);
    assert_eq!(results.len(), 2);
    assert_eq!(results[1].id, id(FAILS));
    assert_eq!(results[1].outcome, TestOutcome::Failed);
    let big = format!("{}\n", "x".repeat(70_000));
    assert!(big.len() > MAX_LINE);
    assert_eq!(results[1].output, big);
}

/// The histories are written in the order the runs ended, however long a
/// write takes.
#[gpui::test]
fn history_keeps_the_order_runs_ended_in(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    // The first run's write is held up.
    let delays = vec![Duration::from_millis(500)];
    project
        .cx
        .update(|_, cx| cx.set_global(DelayHistoryWrites(delays)));

    project.run("runs.test_file");
    project.finished();
    fs::write(project.bin.path().join("pass"), "").unwrap();
    project.run("runs.test_file");
    let run = project.finished();
    assert_eq!(run.status, RunStatus::Passed);
    project.wait_until("both runs to be saved", |project| {
        project.history().last8(&id(FAILS)).len() == 2
    });
    let history = project.history();
    assert_eq!(
        history.last8(&id(FAILS)),
        [Outcome::Failed, Outcome::Passed]
    );
    assert_eq!(history.last8(&id(ADDS)), [Outcome::Passed, Outcome::Passed]);
}

/// A test run's results, and its program's output, go once the Runs window
/// no longer keeps its run.
#[gpui::test]
fn evicted_runs_drop_their_results(cx: &mut TestAppContext) {
    let mut project = trusted(cx);
    project.run("runs.test_file");
    let run = project.finished();
    let process = (project.workbench)
        .read_with(project.cx, |workbench, cx| {
            workbench.test_process(run.id, cx)
        })
        .expect("its program")
        .downgrade();

    // Others finish after it, as many as are kept: it's gone.
    let runs = (project.workbench).read_with(project.cx, |workbench, _| workbench.runs().clone());
    runs.update(project.cx, |runs, cx| {
        for _ in 0..MAX_FINISHED {
            let other = runs.begin(RunKind::Script, "other", cx);
            runs.finish(other, RunEnd::Passed, None, cx);
        }
    });
    draw(project.cx);
    assert!(runs.read_with(project.cx, |runs, _| runs.get(run.id).is_none()));
    let (results, kept) = (project.workbench).read_with(project.cx, |workbench, cx| {
        (
            workbench.test_results(run.id, cx),
            workbench.test_process(run.id, cx),
        )
    });
    assert_eq!(results, None);
    assert!(kept.is_none());
    assert!(process.upgrade().is_none(), "its program is dropped");
}

/// nextest is looked for as a program run in the command's folder is: one
/// that can run, a relative `PATH` entry from that folder, and on the
/// workbench's own `PATH` when the login shell's couldn't be read.
#[test]
fn nextest_is_found_as_programs_are_run() {
    let root = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(root.path()).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
    fs::create_dir(root.join(".config")).unwrap();
    fs::write(root.join(".config/nextest.toml"), "").unwrap();
    let tools = root.join("tools");
    fs::create_dir(&tools).unwrap();
    let nextest = tools.join("cargo-nextest");
    let uses = |path: Option<&str>, inherited: Option<&Path>| {
        let path = path.map(OsString::from);
        let inherited = inherited.map(|dir| dir.as_os_str().to_owned());
        let plan = plan(&Disk, &Scope::Project, &root, path, inherited).unwrap();
        plan.nextest
    };
    let absolute = tools.to_str().unwrap();

    // A file that can't run isn't nextest.
    fs::write(&nextest, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        assert!(!uses(Some(absolute), None));
    }
    executable(&nextest);
    assert!(uses(Some(absolute), None));

    // A relative entry is from the command's folder.
    assert!(uses(Some("tools"), None));
    assert!(!uses(Some("elsewhere"), None));

    // No login shell `PATH`: the workbench's own, which the program then
    // runs with.
    assert!(uses(None, Some(&tools)));
    assert!(!uses(None, None));
    let spec = plan(
        &Disk,
        &Scope::Project,
        &root,
        None,
        Some(tools.clone().into()),
    )
    .unwrap();
    assert!(spec.spec.env.iter().all(|(name, _)| name != "PATH"));
    assert!(spec.spec.merged);
}
