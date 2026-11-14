//! Rust tests run as a run in the workbench's [`Runs`](crate::runs::Runs)
//! (`docs/design/design-system.md`, "Runs: builds, tests and agent tasks in
//! one list"): a test run in progress with live counts, its results kept per
//! test.
//!
//! - `runs.test_file`: the active Rust file's tests, the command
//!   [`test_command`] builds for `tests.run_file`.
//! - `runs.test_project`: `cargo test --workspace` in the root.
//!
//! Each saves the window's files first, as `tests.*` do
//! ([`crate::test_commands`]), and runs only in a trusted root. The command
//! runs through [`RunProcess`], not in a terminal tab, its stdout and stderr
//! one pipe ([`RunSpec::merged`]), as cargo's `Running` lines are on stderr
//! and the results they head on stdout. [`LibtestParser`] reads it, its
//! tests' output captured (no `--nocapture`), so a failure's output is its
//! own. When `cargo nextest` is on the `PATH` it runs with, as a program
//! run in its folder finds it, and that folder configures it (a
//! `.config/nextest.toml`, or `[workspace.metadata.nextest]` in its
//! `Cargo.toml`), it runs `cargo nextest run --message-format libtest-json`
//! and [`NextestParser`] reads it, a line longer than the runner reads as one
//! put back together, up to [`MAX_EVENT_LINE`]. Either way it doesn't stop
//! at the first failure.
//!
//! The run's summary is [`RunSummary::Tests`], updated once a batch of
//! lines as results arrive, its `total` the tests each binary said it would
//! run; once the output ends, what the parser held back counts too, such as
//! a failure whose binary aborted before its report. Its results, each
//! test's name, outcome and output, are kept for the run while
//! [`Runs`](crate::runs::Runs) keeps it ([`Workbench::test_results`]),
//! dropped as it drops the run. Once it passes or fails, its outcomes are
//! recorded in the root's [`History`] under the state directory, off the
//! main thread, one run after another in the order they ended; a cancelled
//! run records nothing.
//!
//! The existing `tests.*` commands, the Run Test lens and their terminal
//! tabs are [`crate::test_commands`]'s and are unchanged. Its text is the
//! catalogue's (`l10n/en-US/test_runs.ftl`).

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use fe_runlog::{
    History, LibtestParser, NextestParser, Outcome, TestEvent, TestEventKind, TestId,
    TestOutputParser,
};
use gpui::{App, AppContext as _, Context, Entity, EntityId, Global, Subscription, Window};
use kiln::Cx;
use kiln_l10n::{tr, verbatim};

use crate::{
    Workbench,
    run_process::{LineSink, RunProcess, RunSpec, RunUpdate, Stream},
    runs::{RunEnd, RunId, RunKind, RunSummary, RunsEvent},
    status_bar::language_label,
    task_commands::{Files, project_files},
    task_defs::TaskCommand,
    task_run::untrusted_message,
    test_commands::{Target, TestFile, no_folder, on_workbench},
    test_runnables::{Runnable, test_command},
    workbench::toast::{self, Toast},
};

/// The commands' declaration: see the module's docs. They're handled by
/// [`init`]'s listeners, as [`crate::test_commands`]' are.
#[allow(dead_code)]
struct TestRunCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl TestRunCommands {
    /// Runs: Test File
    #[command(id = "runs.test_file")]
    fn runs_test_file(&mut self, _: &mut Cx<Self>) {}

    /// Runs: Test Project
    #[command(id = "runs.test_project")]
    fn runs_test_project(&mut self, _: &mut Cx<Self>) {}
}

/// How one test ended in a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestOutcome {
    Passed,
    Failed,
    /// It didn't run.
    Ignored,
}

/// One test's result in a run, for its detail view.
#[derive(Clone, Debug, PartialEq)]
pub struct TestResult {
    pub id: TestId,
    pub outcome: TestOutcome,
    /// What a failed test printed, its panic included: empty for one that
    /// passed, whose output is captured and not shown.
    pub output: String,
    /// How long it took, if the runner said.
    pub secs: Option<f64>,
}

/// The results a run's sink has read so far, shared with the main thread.
type Results = Arc<Mutex<Vec<TestResult>>>;

fn lock(results: &Results) -> MutexGuard<'_, Vec<TestResult>> {
    results.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// The tests counted so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    passed: usize,
    failed: usize,
    ignored: usize,
    /// What the binaries said they'd run, once one has.
    total: Option<usize>,
}

impl Counts {
    fn summary(self) -> RunSummary {
        RunSummary::Tests {
            passed: self.passed,
            failed: self.failed,
            total: self.total,
        }
    }

    /// How far along the tests planned so far are.
    fn progress(self) -> Option<f32> {
        let total = self.total.filter(|total| *total > 0)?;
        let done = self.passed + self.failed + self.ignored;
        Some(done as f32 / total as f32)
    }
}

/// The longest line a test run reassembles from its parts: a nextest event
/// with more output than this is dropped, said once in the log.
const MAX_EVENT_LINE: usize = 32 * 1024 * 1024;

/// Reads a test run's lines into its counts and results, on the runner's
/// thread.
struct TestSink {
    parser: Box<dyn TestOutputParser + Send>,
    results: Results,
    counts: Counts,
    /// Whether the counts changed in this batch.
    changed: bool,
    /// The parts read so far of a line longer than the runner reads as one.
    partial: String,
    /// Whether that line has passed [`MAX_EVENT_LINE`]: it's dropped.
    overlong: bool,
}

impl TestSink {
    fn new(nextest: bool, results: Results) -> Self {
        let parser: Box<dyn TestOutputParser + Send> = match nextest {
            true => Box::new(NextestParser::new()),
            false => Box::new(LibtestParser::new()),
        };
        Self {
            parser,
            results,
            counts: Counts::default(),
            changed: false,
            partial: String::new(),
            overlong: false,
        }
    }

    /// Counts `events` and keeps their results.
    fn take(&mut self, events: Vec<TestEvent>) {
        if events.is_empty() {
            return;
        }
        let mut results = lock(&self.results);
        for event in events {
            let (outcome, output, secs) = match event.kind {
                TestEventKind::Planned(count) => {
                    *self.counts.total.get_or_insert(0) += count;
                    self.changed = true;
                    continue;
                }
                TestEventKind::Started => continue,
                TestEventKind::Passed { secs } => {
                    self.counts.passed += 1;
                    (TestOutcome::Passed, String::new(), secs)
                }
                TestEventKind::Failed { output } => {
                    self.counts.failed += 1;
                    (TestOutcome::Failed, output, None)
                }
                TestEventKind::Ignored => {
                    self.counts.ignored += 1;
                    (TestOutcome::Ignored, String::new(), None)
                }
            };
            self.changed = true;
            results.push(TestResult {
                id: event.id,
                outcome,
                output,
                secs,
            });
        }
    }

    /// Feeds the parser one whole line: `text`, after the parts before it.
    fn feed(&mut self, text: &str) {
        if std::mem::take(&mut self.overlong) {
            self.partial.clear();
            log::warn!("a test run's line passed {MAX_EVENT_LINE} bytes: it's dropped");
            return;
        }
        let events = match self.partial.is_empty() {
            true => self.parser.feed_line(text),
            false => {
                let mut line = std::mem::take(&mut self.partial);
                line.push_str(text);
                self.parser.feed_line(&line)
            }
        };
        self.take(events);
    }

    /// The counts, if they've changed since they were last said.
    fn updates(&mut self) -> Vec<RunUpdate> {
        if !std::mem::take(&mut self.changed) {
            return Vec::new();
        }
        vec![
            RunUpdate::Progress(self.counts.progress()),
            RunUpdate::Summary(self.counts.summary()),
        ]
    }
}

impl LineSink for TestSink {
    fn line(&mut self, _: Stream, text: &str, _: Duration) -> Vec<RunUpdate> {
        self.feed(text);
        Vec::new()
    }

    fn fragment(&mut self, _: Stream, text: &str, _: Duration) -> Vec<RunUpdate> {
        if self.partial.len() + text.len() > MAX_EVENT_LINE {
            self.overlong = true;
            self.partial.clear();
        } else if !self.overlong {
            self.partial.push_str(text);
        }
        Vec::new()
    }

    /// What the parser held back, such as a failure whose output never came
    /// because its binary aborted.
    fn end(&mut self) -> Vec<RunUpdate> {
        if !self.partial.is_empty() || self.overlong {
            self.feed("");
        }
        let held = self.parser.finish();
        self.take(held);
        self.updates()
    }

    fn batch_end(&mut self) -> Vec<RunUpdate> {
        self.updates()
    }
}

/// What's tested.
#[derive(Clone)]
pub(crate) enum Scope {
    /// The tests of the Rust file at `path`.
    File { path: PathBuf, runnable: Runnable },
    /// Every package's.
    Project,
}

/// A test run, as it will start.
pub(crate) struct Plan {
    pub(crate) spec: RunSpec,
    pub(crate) nextest: bool,
}

/// A Rust package's manifest's file name.
const CARGO_TOML: &str = verbatim!("Cargo.toml");

/// What `runs.test_project` says in a folder with no `Cargo.toml`.
pub(crate) fn no_cargo_project() -> String {
    tr!("workbench-test-runs-no-cargo").into()
}

/// What `runs.test_file` says of a file in `language`, which isn't Rust.
pub(crate) fn rust_only(language: &str) -> String {
    let command = fe_command::l10n::command_label("tests.run_file", "");
    tr!(
        "workbench-test-runs-rust-only",
        language = language,
        command = command
    )
    .into()
}

/// The run of `scope` in `root`, its programs on `path`, else, when the
/// login shell's couldn't be read, on the workbench's own, `inherited`, as
/// [`RunProcess`] then runs them: why not, in words, if it can't be told. It
/// reads the project's files, so it's for a background thread.
pub(crate) fn plan(
    files: &dyn crate::task_defs::ProjectFiles,
    scope: &Scope,
    root: &Path,
    path: Option<OsString>,
    inherited: Option<OsString>,
) -> Result<Plan, String> {
    let (title, argv, cwd) = match scope {
        Scope::File { path, runnable } => {
            let task = test_command(files, "rust", runnable, path, root)?;
            let TaskCommand::Argv(argv) = task.command else {
                return Err(no_cargo_project());
            };
            (task.label, argv, task.cwd)
        }
        Scope::Project => {
            if !files.exists(&root.join(CARGO_TOML)) {
                return Err(no_cargo_project());
            }
            let folder = root.file_name().unwrap_or(root.as_os_str());
            let folder = folder.to_string_lossy().into_owned();
            let title = tr!("workbench-test-runs-project-title", folder = folder).into();
            let argv = ["cargo", "test", "--workspace"].map(str::to_owned).into();
            (title, argv, root.to_owned())
        }
    };
    let lookup = path.as_deref().or(inherited.as_deref());
    let nextest = lookup.is_some_and(|lookup| nextest_on_path(lookup, &cwd))
        && configures_nextest(files, &cwd);
    let mut args = run_args(&argv, nextest).into_iter();
    let program = args.next().unwrap_or_else(|| verbatim!("cargo").to_owned());
    let mut env: Vec<(OsString, OsString)> = Vec::new();
    if let Some(path) = path {
        env.push((verbatim!("PATH").into(), path));
    }
    if nextest {
        env.push((
            verbatim!("NEXTEST_EXPERIMENTAL_LIBTEST_JSON").into(),
            verbatim!("1").into(),
        ));
    }
    Ok(Plan {
        spec: RunSpec {
            kind: RunKind::Test,
            title,
            program: program.into(),
            args: args.map(OsString::from).collect(),
            cwd,
            env,
            // Cargo's `Running` lines are on stderr, the results they head on
            // stdout: the parser reads them in the order they were written.
            merged: true,
        },
        nextest,
    })
}

/// `argv`, a `cargo test` command line, as the run's: its tests' output
/// captured, so a failure's is its own; not stopping at the first failing
/// binary, or, under nextest, test; and as `cargo nextest run` with
/// libtest-json if `nextest`.
fn run_args(argv: &[String], nextest: bool) -> Vec<String> {
    let dash = argv
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(argv.len());
    let (cargo, tests) = argv.split_at(dash);
    // `cargo test`, then what it's given.
    let given = cargo.get(2..).unwrap_or_default();
    let tests: Vec<&String> = (tests.iter().skip(1))
        .filter(|arg| *arg != "--nocapture")
        .collect();
    let mut args: Vec<String> = vec![verbatim!("cargo").to_owned()];
    match nextest {
        true => args.extend(["nextest", "run"].map(str::to_owned)),
        false => args.push(verbatim!("test").to_owned()),
    }
    args.extend(given.iter().cloned());
    args.push(verbatim!("--no-fail-fast").to_owned());
    if nextest {
        args.extend(["--message-format", "libtest-json"].map(str::to_owned));
    }
    if !tests.is_empty() {
        args.push(verbatim!("--").to_owned());
        args.extend(tests.into_iter().cloned());
    }
    args
}

/// Whether `cargo nextest` is on `path`, as a program run in `cwd` looks it
/// up: a `cargo-nextest` there that can run, a folder on the `PATH` that
/// isn't absolute being from `cwd`.
fn nextest_on_path(path: &OsStr, cwd: &Path) -> bool {
    let name = [verbatim!("cargo-nextest"), std::env::consts::EXE_SUFFIX].concat();
    std::env::split_paths(path).any(|dir| runnable(&cwd.join(dir).join(&name)))
}

/// Whether `file` is a program that can run: a file, with an execute bit on
/// Unix.
fn runnable(file: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(file) else {
        return false;
    };
    #[cfg(unix)]
    let can_run = {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let can_run = true;
    metadata.is_file() && can_run
}

/// Whether the workspace in `dir` configures nextest: a
/// `.config/nextest.toml`, or `[workspace.metadata.nextest]` in its
/// `Cargo.toml`.
fn configures_nextest(files: &dyn crate::task_defs::ProjectFiles, dir: &Path) -> bool {
    if files.exists(&dir.join(".config").join("nextest.toml")) {
        return true;
    }
    let Ok(manifest) = files.read_to_string(&dir.join(CARGO_TOML)) else {
        return false;
    };
    let Ok(manifest) = manifest.parse::<toml::Table>() else {
        return false;
    };
    let metadata = manifest.get("workspace").and_then(|w| w.get("metadata"));
    metadata.and_then(|m| m.get("nextest")).is_some()
}

/// Where test histories are kept, in place of the user's state directory.
struct HistoryDir(PathBuf);

impl Global for HistoryDir {}

/// Keeps test histories under `dir` from now on.
#[cfg(test)]
pub(crate) fn set_history_dir(cx: &mut App, dir: PathBuf) {
    cx.set_global(HistoryDir(dir));
}

/// The state directory histories go under: the one set, or the user's.
/// Under test, none unless one is set, so no test touches the user's.
fn history_dir(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<HistoryDir>() {
        Some(HistoryDir(dir)) => Some(dir.clone()),
        None if cfg!(test) => None,
        None => fe_config::paths::state_dir(),
    }
}

/// The `PATH` the tests' programs are looked up on, in place of the login
/// shell's.
#[cfg(test)]
pub(crate) struct RunnerPath(pub(crate) OsString);

#[cfg(test)]
impl Global for RunnerPath {}

/// The `PATH` set, under test.
fn runner_path(cx: &App) -> Option<OsString> {
    #[cfg(test)]
    if let Some(RunnerPath(path)) = cx.try_global::<RunnerPath>() {
        return Some(path.clone());
    }
    let _ = cx;
    None
}

/// One run's results and its program, kept while [`Runs`](crate::runs::Runs) keeps the run.
struct TestRun {
    run: RunId,
    process: Entity<RunProcess>,
    results: Results,
    /// The project root and state directory its history is kept in.
    root: PathBuf,
    state_dir: Option<PathBuf>,
    /// Its end has been seen.
    ended: bool,
    _process: Subscription,
}

impl TestRun {
    fn new(
        process: Entity<RunProcess>,
        results: Results,
        root: PathBuf,
        state_dir: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let observed = cx.observe(&process, |this, process, cx| {
            let end = process.read(cx).end();
            this.ended(end, cx);
        });
        let mut this = Self {
            run: process.read(cx).run(),
            process: process.clone(),
            results,
            root,
            state_dir,
            ended: false,
            _process: observed,
        };
        // It may have ended before it was observed.
        this.ended(process.read(cx).end(), cx);
        this
    }

    /// Records the run's outcomes in its history, once it's passed or failed.
    fn ended(&mut self, end: Option<RunEnd>, cx: &mut Context<Self>) {
        let Some(end) = end.filter(|_| !self.ended) else {
            return;
        };
        self.ended = true;
        if end == RunEnd::Cancelled {
            return;
        }
        let Some(state_dir) = self.state_dir.clone() else {
            return;
        };
        let outcomes: Vec<(TestId, Outcome)> = (lock(&self.results).iter())
            .filter_map(|result| {
                let outcome = match result.outcome {
                    TestOutcome::Passed => Outcome::Passed,
                    TestOutcome::Failed => Outcome::Failed,
                    TestOutcome::Ignored => return None,
                };
                Some((result.id.clone(), outcome))
            })
            .collect();
        if outcomes.is_empty() {
            return;
        }
        let root = self.root.clone();
        write_history(
            Box::new(move || {
                let mut history = History::load(&state_dir, &root);
                if let Err(error) = history.record(&outcomes) {
                    log::warn!(
                        "couldn't save the test history of {}: {error}",
                        root.display()
                    );
                }
            }),
            cx,
        );
    }
}

/// A history's read, record and write, done off the main thread.
type HistoryJob = Box<dyn FnOnce() + Send>;

/// The queue the histories' writes go through, one at a time, in the order
/// the runs ended: a later run's results come after an earlier one's.
struct HistoryQueue(async_channel::Sender<HistoryJob>);

impl Global for HistoryQueue {}

/// How long each of the next history writes waits before it starts, so a
/// test can hold one up.
#[cfg(test)]
pub(crate) struct DelayHistoryWrites(pub(crate) Vec<Duration>);

#[cfg(test)]
impl Global for DelayHistoryWrites {}

/// Queues `job` after the history writes queued before it.
fn write_history(job: HistoryJob, cx: &mut App) {
    #[cfg(test)]
    let job: HistoryJob = {
        let delays = cx.has_global::<DelayHistoryWrites>().then(|| {
            let delays = &mut cx.global_mut::<DelayHistoryWrites>().0;
            (!delays.is_empty()).then(|| delays.remove(0))
        });
        match delays.flatten() {
            Some(delay) => Box::new(move || {
                std::thread::sleep(delay);
                job();
            }),
            None => job,
        }
    };
    let jobs = match cx.try_global::<HistoryQueue>() {
        Some(HistoryQueue(jobs)) => jobs.clone(),
        None => {
            let (jobs, queued) = async_channel::unbounded::<HistoryJob>();
            cx.background_spawn(async move {
                while let Ok(job) = queued.recv().await {
                    job();
                }
            })
            .detach();
            cx.set_global(HistoryQueue(jobs.clone()));
            jobs
        }
    };
    if jobs.try_send(job).is_err() {
        log::warn!("couldn't queue a test history's write");
    }
}

/// The test runs of each workbench's [`Runs`](crate::runs::Runs), oldest
/// first.
#[derive(Default)]
struct TestRuns(HashMap<EntityId, Kept>);

impl Global for TestRuns {}

struct Kept {
    runs: Vec<Entity<TestRun>>,
    /// Drops those whose runs [`Runs`](crate::runs::Runs) no longer keeps,
    /// as it changes.
    _changed: Subscription,
    /// Drops them all, and so stops their programs, once the runs go with
    /// their workbench.
    _released: Subscription,
}

/// Drops the test runs of the [`Runs`](crate::runs::Runs) `runs`, keyed by
/// `key`, whose runs it no longer keeps: their results, and their programs'
/// output.
fn prune(key: EntityId, runs: &Entity<crate::runs::Runs>, cx: &mut App) {
    let Some(kept) = cx
        .try_global::<TestRuns>()
        .and_then(|kept| kept.0.get(&key))
    else {
        return;
    };
    let listed = runs.read(cx);
    let gone: Vec<usize> = (kept.runs.iter().enumerate())
        .filter(|(_, test_run)| listed.get(test_run.read(cx).run).is_none())
        .map(|(ix, _)| ix)
        .collect();
    if gone.is_empty() {
        return;
    }
    let dropped: Vec<Entity<TestRun>> = match cx.global_mut::<TestRuns>().0.get_mut(&key) {
        Some(kept) => gone
            .into_iter()
            .rev()
            .map(|ix| kept.runs.remove(ix))
            .collect(),
        None => Vec::new(),
    };
    drop(dropped);
}

impl Workbench {
    /// `runs.test_file`: the active Rust file's tests.
    fn run_test_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let TestFile {
            path,
            language,
            runnable,
        } = match self.test_file(Target::File, cx) {
            Ok(found) => found,
            Err(message) => return toast::show(cx, Toast::info(message)),
        };
        if language != "rust" {
            return toast::show(cx, Toast::info(rust_only(&language_label(language))));
        }
        self.start_test_run(Scope::File { path, runnable }, window, cx);
    }

    /// Saves the window's files, works out the run of `scope` off the main
    /// thread, and starts it if the root is still the one it was for. A root
    /// that isn't trusted is refused with a toast offering to trust it.
    fn start_test_run(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root().map(Path::to_owned) else {
            return toast::show(cx, Toast::info(no_folder()));
        };
        if !self.root_trusted() {
            let trust = fe_command::CommandId::parse("workbench.trust_folder")
                .expect("the command's id is valid");
            let toast = Toast::warning(untrusted_message())
                .action(tr!("workbench-lsp-tasks-trust-folder"), trust);
            return toast::show(cx, toast);
        }
        let saving = self.save_all(window, cx);
        let files: Files = project_files(cx);
        let path = runner_path(cx);
        cx.spawn_in(window, async move |workbench, cx| {
            saving.await;
            let saved = workbench.update(cx, |workbench, cx| {
                workbench.root() == Some(root.as_path()) && workbench.all_saved(cx)
            });
            if !saved.unwrap_or(false) {
                return;
            }
            let planning = cx.background_spawn({
                let root = root.clone();
                async move {
                    let path = path.or_else(|| {
                        let login = fe_lsp::LoginPath::system().path();
                        login.map(ToOwned::to_owned)
                    });
                    plan(
                        &*files,
                        &scope,
                        &root,
                        path,
                        std::env::var_os(verbatim!("PATH")),
                    )
                }
            });
            let planned = planning.await;
            let started = workbench.update(cx, |workbench, cx| {
                if workbench.root() != Some(root.as_path()) {
                    return;
                }
                match planned {
                    Ok(plan) => workbench.begin_test_run(plan, root, cx),
                    Err(message) => toast::show(cx, Toast::info(message)),
                }
            });
            started.ok();
        })
        .detach();
    }

    /// Starts `plan`'s program for a new run, its results kept here.
    fn begin_test_run(&mut self, plan: Plan, root: PathBuf, cx: &mut Context<Self>) {
        let results = Results::default();
        let sink = TestSink::new(plan.nextest, results.clone());
        let starting = RunProcess::start(plan.spec, sink, self.runs().clone(), cx);
        let state_dir = history_dir(cx);
        cx.spawn(async move |workbench, cx| {
            let started = starting.await;
            let kept = workbench.update(cx, |workbench, cx| match started {
                Ok(process) => workbench.keep_test_run(process, results, root, state_dir, cx),
                Err(refused) => toast::show(cx, Toast::warning(refused.message().to_owned())),
            });
            kept.ok();
        })
        .detach();
    }

    /// Keeps a started run's results and its program, dropping those of the
    /// runs [`Runs`](crate::runs::Runs) no longer keeps.
    fn keep_test_run(
        &mut self,
        process: Entity<RunProcess>,
        results: Results,
        root: PathBuf,
        state_dir: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let test_run = cx.new(|cx| TestRun::new(process, results, root, state_dir, cx));
        let runs = self.runs().clone();
        let key = runs.entity_id();
        let app: &mut App = cx;
        let mut kept = match app.default_global::<TestRuns>().0.remove(&key) {
            Some(kept) => kept,
            None => Kept {
                runs: Vec::new(),
                _changed: app.subscribe(&runs, move |runs, _: &RunsEvent, cx| {
                    prune(key, &runs, cx);
                }),
                _released: app.observe_release(&runs, move |_, cx| {
                    if cx.has_global::<TestRuns>() {
                        cx.global_mut::<TestRuns>().0.remove(&key);
                    }
                }),
            },
        };
        kept.runs.push(test_run);
        app.global_mut::<TestRuns>().0.insert(key, kept);
    }

    /// The test run that made `run`, while [`Runs`](crate::runs::Runs) keeps it.
    fn test_run(&self, run: RunId, cx: &App) -> Option<Entity<TestRun>> {
        self.runs().read(cx).get(run)?;
        let kept = cx
            .try_global::<TestRuns>()?
            .0
            .get(&self.runs().entity_id())?;
        (kept.runs.iter())
            .find(|test_run| test_run.read(cx).run == run)
            .cloned()
    }

    /// The results of the test run that made `run`, so far, in the order
    /// they came, while [`Runs`](crate::runs::Runs) keeps it: none if it isn't one.
    pub fn test_results(&self, run: RunId, cx: &App) -> Option<Vec<TestResult>> {
        let test_run = self.test_run(run, cx)?;
        let results = lock(&test_run.read(cx).results).clone();
        Some(results)
    }

    /// The program of the test run that made `run`, and the output it's
    /// kept, while [`Runs`](crate::runs::Runs) keeps it: none if it isn't one.
    pub fn test_process(&self, run: RunId, cx: &App) -> Option<Entity<RunProcess>> {
        Some(self.test_run(run, cx)?.read(cx).process.clone())
    }
}

/// Handles the commands.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &RunsTestFile, cx| {
        on_workbench(cx, |workbench, window, cx| {
            workbench.run_test_file(window, cx)
        });
    });
    cx.on_action(|_: &RunsTestProject, cx| {
        on_workbench(cx, |workbench, window, cx| {
            workbench.start_test_run(Scope::Project, window, cx)
        });
    });
}
