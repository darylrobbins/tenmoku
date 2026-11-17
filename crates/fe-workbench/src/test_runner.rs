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
//! A run keeps the command it was planned from, so `runs.rerun_failed`
//! ([`crate::test_run_view`]) can run its failed tests again, each by its
//! exact name ([`failed_plan`]). Reads of a root's history for the test
//! run's detail ([`test_tracks`]) go through the same queue as the writes.
//! [`Workbench::record_test_run`] adds a finished run that ran elsewhere,
//! for the screenshots.
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
    Flaky, History, LibtestParser, NextestParser, Outcome, TestEvent, TestEventKind, TestId,
    TestOutputParser,
};
use gpui::{App, AppContext as _, Context, Entity, EntityId, Global, Subscription, Task, Window};
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

/// What a `cargo test` run's output has said of its test binaries so far,
/// shared with the main thread.
type Binaries = Arc<Mutex<CargoBinaries>>;

fn lock<T>(shared: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    shared.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// The cargo target a test binary was built from, as cargo's
/// `compiler-artifact` message names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CargoTarget {
    /// Its package's manifest.
    pub(crate) manifest: PathBuf,
    /// The options that pick it alone in its package: `--lib`, `--bin`,
    /// `--test`, `--bench` or `--example` and its name as its manifest
    /// spells it, or `--doc` for a library's doc tests.
    pub(crate) options: Vec<String>,
    /// Its tests' binary's name, as nextest names it (`p`, `p::bin/p`,
    /// `p::more`), when cargo said its package: what its results'
    /// [`TestId`]s name, in place of cargo's file name without its hash.
    /// None for doc tests.
    pub(crate) id: Option<String>,
}

/// What a `cargo test` run's output said of its test binaries: which binary
/// each result ran in, and the target each was built from. Cargo names a
/// binary by its file without its hash, so a library and a binary of one
/// name, or two packages' integration tests of one name, are one there:
/// these keep them apart, and give their results' [`TestId`]s the names
/// nextest gives them ([`CargoTarget::id`]).
#[derive(Debug, Default)]
pub(crate) struct CargoBinaries {
    /// The binary each result ran in, by the result's index: its
    /// executable's file name, cargo's hash kept, or `Doc-tests <crate>`;
    /// none before cargo named one.
    pub(crate) of_result: Vec<Option<String>>,
    /// The targets of the test executables cargo built, by file name.
    pub(crate) executables: HashMap<String, CargoTarget>,
    /// Each library's package's manifest, by its crate's name, for its doc
    /// tests: none if two packages' libraries have that name, as cargo's
    /// `Doc-tests` line then can't say whose.
    pub(crate) libraries: HashMap<String, Option<PathBuf>>,
}

impl CargoBinaries {
    /// The target `binary`, as [`Self::of_result`] names it, was built from.
    pub(crate) fn target(&self, binary: &str) -> Option<CargoTarget> {
        match binary.strip_prefix("Doc-tests ") {
            Some(krate) => Some(CargoTarget {
                manifest: self.libraries.get(krate.trim())?.clone()?,
                options: vec![verbatim!("--doc").to_owned()],
                id: None,
            }),
            None => self.executables.get(binary).cloned(),
        }
    }

    /// Keeps the target of the test executable, or the library, a
    /// `compiler-artifact` message `line` says cargo built.
    pub(crate) fn read_artifact(&mut self, line: &str) {
        if !line.starts_with(r#"{"reason":"compiler-artifact""#) {
            return;
        }
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        let target = &message["target"];
        let (Some(manifest), Some(name), Some(kinds)) = (
            message["manifest_path"].as_str(),
            target["name"].as_str(),
            target["kind"].as_array(),
        ) else {
            return;
        };
        let manifest = PathBuf::from(manifest);
        let kinds: Vec<&str> = kinds.iter().filter_map(|kind| kind.as_str()).collect();
        let library = (kinds.iter()).any(|kind| {
            matches!(
                *kind,
                "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
            )
        });
        if library {
            let known = (self.libraries)
                .entry(name.replace('-', "_"))
                .or_insert_with(|| Some(manifest.clone()));
            if known.as_ref() != Some(&manifest) {
                *known = None;
            }
        }
        let tested = message["profile"]["test"].as_bool() == Some(true);
        let Some(executable) = message["executable"].as_str().filter(|_| tested) else {
            return;
        };
        let option = match kinds.first().copied() {
            _ if library => None,
            Some("bin") => Some(verbatim!("--bin")),
            Some("test") => Some(verbatim!("--test")),
            Some("bench") => Some(verbatim!("--bench")),
            Some("example") => Some(verbatim!("--example")),
            _ => return,
        };
        let options = match option {
            Some(option) => vec![option.to_owned(), name.to_owned()],
            None => vec![verbatim!("--lib").to_owned()],
        };
        let package = message["package_id"].as_str().and_then(package_of);
        let id = package.map(|package| {
            let kind = match kinds.first().copied() {
                _ if library => return package,
                Some("bin") => verbatim!("::bin/"),
                Some("bench") => verbatim!("::bench/"),
                Some("example") => verbatim!("::example/"),
                _ => verbatim!("::"),
            };
            [package.as_str(), kind, name].concat()
        });
        let file = executable_name(executable);
        let target = CargoTarget {
            manifest,
            options,
            id,
        };
        (self.executables).insert(file, target);
    }
}

/// The name of the package a cargo package ID names:
/// `path+file:///w/p#0.1.0` (the name left out when it's the folder's),
/// `path+file:///w#root@0.1.0`, `registry+…#name@1.0.0`, or, before cargo
/// 1.77, `p 0.1.0 (path+file:///w/p)`.
fn package_of(id: &str) -> Option<String> {
    if let Some((url, fragment)) = id.rsplit_once('#') {
        let name = match fragment.split_once('@') {
            Some((name, _)) => name,
            None => url.rsplit('/').next()?,
        };
        return (!name.is_empty()).then(|| name.to_owned());
    }
    let (name, _) = id.split_once(' ')?;
    Some(name.to_owned())
}

/// A test executable's file name without cargo's hash, as cargo names its
/// tests' binary: `p` for `p-0123456789abcdef`.
fn unhashed(file: &str) -> &str {
    match file.rsplit_once('-') {
        Some((stem, hash))
            if !stem.is_empty()
                && hash.len() >= 8
                && hash.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            stem
        }
        _ => file,
    }
}

/// An executable's file name, without `.exe`.
fn executable_name(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    file.strip_suffix(verbatim!(".exe"))
        .unwrap_or(file)
        .to_owned()
}

/// `line` without its ANSI escapes: cargo colours its status words when
/// its configuration or environment says to.
fn plain(line: &str) -> std::borrow::Cow<'_, str> {
    if !line.contains('\x1b') {
        return line.into();
    }
    let mut plain = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            plain.push(c);
            continue;
        }
        // `ESC [ … final`, the final byte from `@` to `~`.
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    plain.into()
}

/// The executable a `cargo test` `Running` line says it runs: its file
/// name, cargo's hash kept. The line is cargo's only when its status word
/// is right-aligned in cargo's 12 columns, as [`LibtestParser`] reads it:
/// a test's output that says `Running` isn't. None for any other line.
fn running_executable(line: &str) -> Option<String> {
    let rest = line.trim_start_matches(' ');
    let word = verbatim!("Running");
    if line.len() - rest.len() != 12 - word.len() {
        return None;
    }
    let rest = rest.strip_prefix(word)?.strip_prefix(' ')?.trim_end();
    // `unittests src/lib.rs (target/debug/deps/p-0123456789abcdef)`; older
    // cargo wrote the path alone.
    let path = match rest
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" ("))
    {
        Some((_, path)) => path,
        None => rest,
    };
    Some(executable_name(path))
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
    /// Under `cargo test`, what its output said of its binaries.
    binaries: Option<Binaries>,
    /// The executables cargo's `Running` lines named so far, the latest of
    /// each name, by their names without cargo's hash: the parser's name
    /// for a result's binary.
    running: HashMap<String, String>,
    counts: Counts,
    /// Whether the counts changed in this batch.
    changed: bool,
    /// The parts read so far of a line longer than the runner reads as one.
    partial: String,
    /// Whether that line has passed [`MAX_EVENT_LINE`]: it's dropped.
    overlong: bool,
}

impl TestSink {
    fn new(nextest: bool, results: Results, binaries: Binaries) -> Self {
        let parser: Box<dyn TestOutputParser + Send> = match nextest {
            true => Box::new(NextestParser::new()),
            false => Box::new(LibtestParser::new()),
        };
        Self {
            parser,
            results,
            binaries: (!nextest).then_some(binaries),
            running: HashMap::new(),
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
            let mut id = event.id;
            if let Some(binaries) = &self.binaries {
                // The executable the parser's binary is: the latest of its
                // name; a doc test's, its `Doc-tests` name.
                let executable = match id.binary.starts_with("Doc-tests ") {
                    true => Some(id.binary.clone()),
                    false => self.running.get(&id.binary).cloned(),
                };
                let mut binaries = lock(binaries);
                let target = executable
                    .as_deref()
                    .and_then(|e| binaries.executables.get(e));
                if let Some(named) = target.and_then(|target| target.id.clone()) {
                    id.binary = named;
                }
                binaries.of_result.push(executable);
            }
            results.push(TestResult {
                id,
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
        let joined;
        let line = match self.partial.is_empty() {
            true => text,
            false => {
                let mut line = std::mem::take(&mut self.partial);
                line.push_str(text);
                joined = line;
                joined.as_str()
            }
        };
        // A `Running` line ends the binary before it: what the parser
        // gives for it is that binary's.
        let events = self.parser.feed_line(line);
        self.take(events);
        if let Some(binaries) = &self.binaries {
            let line = plain(line);
            match running_executable(&line) {
                Some(executable) => {
                    let name = unhashed(&executable).to_owned();
                    self.running.insert(name, executable);
                }
                None => lock(binaries).read_artifact(&line),
            }
        }
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
    /// Failed tests of a run, again ([`failed_plan`]).
    Failed(Rerun),
}

/// A rerun of some of a run's failed tests, before it's planned.
#[derive(Clone, Debug)]
pub(crate) struct Rerun {
    /// The run's command with no tests picked ([`Plan::base`]).
    pub(crate) base: RunSpec,
    pub(crate) nextest: bool,
    /// The tests, each by its binary and its name: under `cargo test`,
    /// all of one binary's.
    pub(crate) tests: Vec<TestId>,
    /// Under `cargo test`, the target their binary was built from.
    pub(crate) target: Option<CargoTarget>,
    /// Whether its title names its target: a run's failures rerun one
    /// binary at a time.
    pub(crate) labelled: bool,
}

/// A test run, as it will start.
#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub(crate) spec: RunSpec,
    pub(crate) nextest: bool,
    /// What a rerun of its failed tests starts from: its command with no
    /// tests picked, its subcommand and options without the name filters
    /// before its `--` or anything after it ([`unfiltered`]).
    pub(crate) base: RunSpec,
}

/// The options of cargo's and nextest's, before a command's `--`, that
/// take the argument after them.
const TAKES_VALUE: &[&str] = &[
    "-p",
    "--package",
    "--exclude",
    "--test",
    "--bin",
    "--bench",
    "--example",
    "-F",
    "--features",
    "--target",
    "--target-dir",
    "--profile",
    "--cargo-profile",
    "--manifest-path",
    "-j",
    "--jobs",
    "--color",
    "--config",
    "-Z",
    "--message-format",
    "-E",
    "--filterset",
    "--filter-expr",
];

/// What picks a package: those that take a value, and those that don't.
const PACKAGE_OPTIONS: (&[&str], &[&str]) =
    (&["-p", "--package", "--exclude"], &["--workspace", "--all"]);

/// What picks a package's targets: those that take a value, and those that
/// don't.
const TARGET_OPTIONS: (&[&str], &[&str]) = (
    &["--test", "--bin", "--bench", "--example"],
    &[
        "--lib",
        "--bins",
        "--tests",
        "--benches",
        "--examples",
        "--doc",
        "--all-targets",
    ],
);

/// `args`, a test run's arguments after its program, with no tests picked:
/// its subcommand (`test`, or `nextest run`) and its options, without the
/// name filters before its `--` or anything after it. A filter matches any
/// test whose name it's in, and nextest's `--exact` doesn't reach those
/// before `--`, so a filter kept would pick more than the tests named.
fn unfiltered(args: &[OsString], nextest: bool) -> Vec<OsString> {
    let words = if nextest { 2 } else { 1 };
    let mut kept: Vec<OsString> = args.iter().take(words).cloned().collect();
    let mut rest = args.iter().skip(words);
    while let Some(arg) = rest.next() {
        if arg == "--" {
            break;
        }
        let text = arg.to_string_lossy();
        if !text.starts_with('-') {
            continue;
        }
        kept.push(arg.clone());
        if TAKES_VALUE.contains(&text.as_ref())
            && let Some(value) = rest.next()
        {
            kept.push(value.clone());
        }
    }
    kept
}

/// `args` without the options `(with_value, alone)`, and the values of
/// those that take one.
fn without(args: &[OsString], (with_value, alone): (&[&str], &[&str])) -> Vec<OsString> {
    let mut kept = Vec::new();
    let mut skip = false;
    for arg in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        let text = arg.to_string_lossy();
        let (option, joined) = match text.split_once('=') {
            Some((option, _)) => (option, true),
            None => (text.as_ref(), false),
        };
        if with_value.contains(&option) {
            skip = !joined;
        } else if !alone.contains(&option) {
            kept.push(arg.clone());
        }
    }
    kept
}

/// `text` as a nextest filterset matcher's string: its backslashes,
/// closing parentheses, commas and slashes escaped.
fn filterset_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | ')' | ',' | '/') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// The nextest filterset that picks `tests` and nothing else: each its
/// binary's test of its name, exactly.
pub(crate) fn filterset(tests: &[TestId]) -> String {
    let each: Vec<String> = (tests.iter())
        .map(|test| {
            let binary = filterset_text(&test.binary);
            let name = filterset_text(&test.name);
            [
                verbatim!("(binary_id(="),
                &binary,
                verbatim!(") & test(="),
                &name,
                verbatim!("))"),
            ]
            .concat()
        })
        .collect();
    each.join(verbatim!(" | "))
}

/// The name of the package whose manifest is `manifest`.
fn package_name(files: &dyn crate::task_defs::ProjectFiles, manifest: &Path) -> Option<String> {
    let manifest = files.read_to_string(manifest).ok()?;
    let manifest = manifest.parse::<toml::Table>().ok()?;
    let name = manifest.get("package")?.get("name")?.as_str()?;
    Some(name.to_owned())
}

/// What a rerun says when it can't tell which cargo target `binary` was
/// built from, so it can't pick its tests alone.
pub(crate) fn unknown_target(binary: &str) -> String {
    tr!("workbench-test-runs-rerun-unknown-target", binary = binary).into()
}

/// The run of `rerun`'s tests, and only those, from its base command.
/// Under nextest, a filterset picks each by its binary and name
/// ([`filterset`]). Under `cargo test`, which can't tie a name to a binary,
/// the tests are all one binary's: its package (`-p`, in place of the
/// base's) and target, as cargo built them ([`CargoTarget`]), then `--
/// --exact` and the names, as a test binary takes any number of filters
/// after `--`. Its title says it reruns the base's failed tests, and names
/// the package and target when the run's are rerun one binary at a time.
/// It reads the package's manifest, so it's for a background thread; why
/// not if it can't.
pub(crate) fn failed_plan(
    files: &dyn crate::task_defs::ProjectFiles,
    rerun: &Rerun,
) -> Result<Plan, String> {
    let base = &rerun.base;
    let mut args = base.args.clone();
    let mut label = None;
    if rerun.nextest {
        args.push(verbatim!("-E").into());
        args.push(filterset(&rerun.tests).into());
    } else {
        let binary = rerun.tests.first().map_or("", |test| test.binary.as_str());
        let target = rerun
            .target
            .as_ref()
            .ok_or_else(|| unknown_target(binary))?;
        let package =
            package_name(files, &target.manifest).ok_or_else(|| unknown_target(binary))?;
        args = without(&without(&args, TARGET_OPTIONS), PACKAGE_OPTIONS);
        args.extend([verbatim!("-p").into(), OsString::from(&package)]);
        args.extend(target.options.iter().map(OsString::from));
        args.push(verbatim!("--").into());
        args.push(verbatim!("--exact").into());
        let mut names: Vec<&str> = Vec::new();
        for test in &rerun.tests {
            if !names.contains(&test.name.as_str()) {
                names.push(&test.name);
            }
        }
        args.extend(names.into_iter().map(OsString::from));
        label = rerun.labelled.then(|| {
            let mut label = vec![package];
            label.extend(target.options.iter().cloned());
            label.join(" ")
        });
    }
    let title = match &label {
        Some(target) => tr!(
            "workbench-test-runs-rerun-title-binary",
            title = &base.title,
            binary = target
        ),
        None => tr!("workbench-test-runs-rerun-title", title = &base.title),
    };
    Ok(Plan {
        spec: RunSpec {
            title: title.into(),
            args,
            ..base.clone()
        },
        nextest: rerun.nextest,
        base: base.clone(),
    })
}

/// The reruns of `failed`, the failed tests of a run whose base command is
/// `base`, each with the binary it ran in as `binaries` names it: one under
/// nextest, else one a binary, in the order they failed, each with the
/// target cargo built it from. Why not if a binary's target isn't known:
/// a rerun without it would pick more than the tests.
pub(crate) fn reruns(
    base: &RunSpec,
    nextest: bool,
    failed: Vec<(TestId, Option<String>)>,
    binaries: &CargoBinaries,
) -> Result<Vec<Rerun>, String> {
    if nextest {
        return Ok(vec![Rerun {
            base: base.clone(),
            nextest,
            tests: failed.into_iter().map(|(test, _)| test).collect(),
            target: None,
            labelled: false,
        }]);
    }
    let mut groups: Vec<(Option<String>, Vec<TestId>)> = Vec::new();
    for (test, binary) in failed {
        match groups.iter_mut().find(|(group, _)| *group == binary) {
            Some((_, tests)) => tests.push(test),
            None => groups.push((binary, vec![test])),
        }
    }
    let labelled = groups.len() > 1;
    (groups.into_iter())
        .map(|(binary, tests)| {
            let target = binary.as_deref().and_then(|binary| binaries.target(binary));
            let Some(target) = target else {
                return Err(unknown_target(&tests[0].binary));
            };
            Ok(Rerun {
                base: base.clone(),
                nextest,
                tests,
                target: Some(target),
                labelled,
            })
        })
        .collect()
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
        Scope::Failed(rerun) => return failed_plan(files, rerun),
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
    let spec = RunSpec {
        kind: RunKind::Test,
        title,
        program: program.into(),
        args: args.map(OsString::from).collect(),
        cwd,
        env,
        // Cargo's `Running` lines are on stderr, the results they head on
        // stdout: the parser reads them in the order they were written.
        merged: true,
    };
    Ok(Plan {
        base: RunSpec {
            args: unfiltered(&spec.args, nextest),
            ..spec.clone()
        },
        spec,
        nextest,
    })
}

/// `argv`, a `cargo test` command line, as the run's: its tests' output
/// captured, so a failure's is its own; not stopping at the first failing
/// binary, or, under nextest, test; cargo's messages as JSON with its
/// diagnostics rendered, under `cargo test`; and as `cargo nextest run`
/// with libtest-json if `nextest`.
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
    // Cargo's messages as JSON, its diagnostics rendered as ever: its
    // `compiler-artifact` messages name the target each test executable was
    // built from, for reruns ([`CargoBinaries`]).
    if !nextest {
        args.extend(["--message-format", "json-render-diagnostics"].map(str::to_owned));
    }
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
pub(crate) fn history_dir(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<HistoryDir>() {
        Some(HistoryDir(dir)) => Some(dir.clone()),
        None if cfg!(test) => None,
        None => fe_config::paths::state_dir(),
    }
}

/// How many runs' results have been saved to a history: set again each time
/// one is, so what shows from a history (the code lenses') reads it again.
#[derive(Default)]
pub(crate) struct HistorySaved(pub(crate) u64);

impl Global for HistorySaved {}

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

/// One test's last results in its root's history, for the detail view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestTrack {
    /// Oldest first, at most the history's eight.
    pub last: Vec<Outcome>,
    /// Whether it both passed and failed in them, and how often it failed.
    pub flaky: Option<Flaky>,
}

/// Tests' last results, by test.
pub type Tracks = HashMap<TestId, TestTrack>;

/// One run's results and its program, kept while [`Runs`](crate::runs::Runs) keeps the run.
struct TestRun {
    run: RunId,
    /// Its program: none for a run recorded from elsewhere
    /// ([`Workbench::record_test_run`]).
    process: Option<Entity<RunProcess>>,
    results: Results,
    /// What its `cargo test` output said of its binaries.
    binaries: Binaries,
    /// What a rerun of its failed tests starts from, and whether it runs
    /// under nextest: none for a recorded run.
    rerun: Option<(RunSpec, bool)>,
    /// Its tests' last results, given with a recorded run in place of the
    /// history's.
    tracks: Option<Tracks>,
    /// The project root and state directory its history is kept in.
    root: PathBuf,
    state_dir: Option<PathBuf>,
    /// Its end has been seen.
    ended: bool,
    _process: Option<Subscription>,
}

impl TestRun {
    fn new(
        process: Entity<RunProcess>,
        results: Results,
        binaries: Binaries,
        plan: &Plan,
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
            process: Some(process.clone()),
            results,
            binaries,
            rerun: Some((plan.base.clone(), plan.nextest)),
            tracks: None,
            root,
            state_dir,
            ended: false,
            _process: Some(observed),
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
        let (saved, written) = async_channel::bounded::<()>(1);
        queue_history_job(
            Box::new(move || {
                let mut history = History::load(&state_dir, &root);
                if let Err(error) = history.record(&outcomes) {
                    log::warn!(
                        "couldn't save the test history of {}: {error}",
                        root.display()
                    );
                }
                saved.try_send(()).ok();
            }),
            cx,
        );
        cx.spawn(async move |_, cx| {
            if written.recv().await.is_ok() {
                cx.update(|cx| {
                    let runs = cx.try_global::<HistorySaved>().map_or(0, |saved| saved.0);
                    cx.set_global(HistorySaved(runs + 1));
                });
            }
        })
        .detach();
    }
}

/// A history's read, record and write, or a read alone, done off the main
/// thread.
type HistoryJob = Box<dyn FnOnce() + Send>;

/// The queue the histories' writes go through, one at a time, in the order
/// the runs ended: a later run's results come after an earlier one's. Reads
/// go through it too, so a read queued after a run ended sees its results.
struct HistoryQueue(async_channel::Sender<HistoryJob>);

impl Global for HistoryQueue {}

/// How long each of the next history writes waits before it starts, so a
/// test can hold one up.
#[cfg(test)]
pub(crate) struct DelayHistoryWrites(pub(crate) Vec<Duration>);

#[cfg(test)]
impl Global for DelayHistoryWrites {}

/// Queues `job` after the history jobs queued before it.
fn queue_history_job(job: HistoryJob, cx: &mut App) {
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
        let (results, binaries) = (Results::default(), Binaries::default());
        let sink = TestSink::new(plan.nextest, results.clone(), binaries.clone());
        let starting = RunProcess::start(plan.spec.clone(), sink, self.runs().clone(), cx);
        let state_dir = history_dir(cx);
        cx.spawn(async move |workbench, cx| {
            let started = starting.await;
            let kept = workbench.update(cx, |workbench, cx| match started {
                Ok(process) => {
                    let test_run = cx.new(|cx| {
                        TestRun::new(process, results, binaries, &plan, root, state_dir, cx)
                    });
                    workbench.keep_test_run(test_run, cx);
                }
                Err(refused) => toast::show(cx, Toast::warning(refused.message().to_owned())),
            });
            kept.ok();
        })
        .detach();
    }

    /// Adds a finished test run titled `title` with `results`, its tests'
    /// last results `tracks`, as a runner other than the workbench's ran
    /// it: nothing's recorded in the history, and it can't be rerun. It
    /// failed if a test did. For the screenshots, whose scenes run no
    /// tests.
    pub fn record_test_run(
        &mut self,
        title: impl Into<String>,
        results: Vec<TestResult>,
        tracks: Tracks,
        cx: &mut Context<Self>,
    ) -> RunId {
        let count = |outcome| (results.iter()).filter(|r| r.outcome == outcome).count();
        let (passed, failed) = (count(TestOutcome::Passed), count(TestOutcome::Failed));
        let summary = RunSummary::Tests {
            passed,
            failed,
            total: Some(results.len()),
        };
        let end = match failed {
            0 => RunEnd::Passed,
            _ => RunEnd::Failed { code: Some(101) },
        };
        let title = title.into();
        let run = self.runs().clone().update(cx, |runs, cx| {
            let run = runs.begin(RunKind::Test, title, cx);
            runs.finish(run, end, Some(summary), cx);
            run
        });
        let root = self.root().map(Path::to_owned).unwrap_or_default();
        let test_run = cx.new(|_| TestRun {
            run,
            process: None,
            results: Arc::new(Mutex::new(results)),
            binaries: Binaries::default(),
            rerun: None,
            tracks: Some(tracks),
            root,
            state_dir: None,
            ended: true,
            _process: None,
        });
        self.keep_test_run(test_run, cx);
        run
    }

    /// `runs.rerun_failed` on `run`: the tests that failed in it so far,
    /// and only those, as new runs of the same command ([`reruns`],
    /// [`failed_plan`]): one under nextest, one a test binary under `cargo
    /// test`. Its files are saved first, as [`Self::start_test_run`] saves
    /// them. A toast says why not if `run` isn't a test run this workbench
    /// ran, it ran in another folder than the one open, whose history its
    /// results would go in, no test failed in it, or cargo didn't say which
    /// target a failed test's binary was built from.
    pub(crate) fn rerun_failed(&mut self, run: RunId, window: &mut Window, cx: &mut Context<Self>) {
        let found = self.test_run(run, cx).and_then(|test_run| {
            let test_run = test_run.read(cx);
            let (base, nextest) = test_run.rerun.clone()?;
            let failed = failed_tests(&lock(&test_run.results), &test_run.binaries);
            let reruns = reruns(&base, nextest, failed, &lock(&test_run.binaries));
            Some((reruns, test_run.root.clone()))
        });
        let Some((reruns, root)) = found else {
            return toast::show(cx, Toast::info(tr!("workbench-test-runs-rerun-none")));
        };
        if self.root() != Some(root.as_path()) {
            let other = tr!("workbench-test-runs-rerun-other-folder");
            return toast::show(cx, Toast::info(other));
        }
        let reruns = match reruns {
            Ok(reruns) if reruns.iter().all(|rerun| rerun.tests.is_empty()) => {
                let none = tr!("workbench-test-runs-rerun-no-failures");
                return toast::show(cx, Toast::info(none));
            }
            Ok(reruns) => reruns,
            Err(why) => return toast::show(cx, Toast::info(why)),
        };
        for rerun in reruns {
            self.start_test_run(Scope::Failed(rerun), window, cx);
        }
    }

    /// Keeps a run's results and its program, dropping those of the runs
    /// [`Runs`](crate::runs::Runs) no longer keeps.
    fn keep_test_run(&mut self, test_run: Entity<TestRun>, cx: &mut Context<Self>) {
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
        self.test_run(run, cx)?.read(cx).process.clone()
    }
}

/// The tests that failed in `results`, each with the binary it ran in as
/// `binaries` names it, each once, in the order they failed.
fn failed_tests(results: &[TestResult], binaries: &Binaries) -> Vec<(TestId, Option<String>)> {
    let binaries = lock(binaries);
    let mut failed: Vec<(TestId, Option<String>)> = Vec::new();
    for (ix, result) in results.iter().enumerate() {
        let binary = binaries.of_result.get(ix).cloned().flatten();
        let test = (result.id.clone(), binary);
        if result.outcome == TestOutcome::Failed && !failed.contains(&test) {
            failed.push(test);
        }
    }
    failed
}

/// The last results of the tests `ids` of the test run that made `run` in
/// `workbench`, as its root's history has them once the history jobs queued
/// so far are done: a run that's ended is in it. Those given with a
/// recorded run, at once. None if `run` isn't a test run; none of the tests
/// if no history is kept.
pub(crate) fn test_tracks(
    workbench: &Entity<Workbench>,
    run: RunId,
    ids: Vec<TestId>,
    cx: &mut App,
) -> Option<Task<Tracks>> {
    let test_run = workbench.read(cx).test_run(run, cx)?;
    let test_run = test_run.read(cx);
    if let Some(tracks) = &test_run.tracks {
        let given = (ids.into_iter())
            .filter_map(|id| Some((id.clone(), tracks.get(&id)?.clone())))
            .collect();
        return Some(Task::ready(given));
    }
    let Some(state_dir) = test_run.state_dir.clone() else {
        return Some(Task::ready(Tracks::default()));
    };
    let root = test_run.root.clone();
    let (sender, received) = async_channel::bounded::<Tracks>(1);
    let read: HistoryJob = Box::new(move || {
        let history = History::load(&state_dir, &root);
        let tracks = (ids.into_iter())
            .map(|id| {
                let track = TestTrack {
                    last: history.last8(&id),
                    flaky: history.flaky(&id),
                };
                (id, track)
            })
            .collect();
        sender.try_send(tracks).ok();
    });
    queue_history_job(read, cx);
    Some(cx.background_spawn(async move { received.recv().await.unwrap_or_default() }))
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
