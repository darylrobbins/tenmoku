//! A Rust project's build run as a run in the workbench's
//! [`Runs`](crate::runs::Runs) (`docs/design/design-system.md`, "Runs:
//! builds, tests and agent tasks in one list"), its compiler messages in the
//! Problems panel.
//!
//! `runs.build` runs `cargo build --workspace
//! --message-format=json-render-diagnostics` in the root, through
//! [`RunProcess`] rather than a terminal tab, as a [`RunKind::Build`] run.
//! It saves the window's files first, as the test runs do
//! ([`crate::test_runner`]), and runs only in a trusted root, and only in a
//! Rust project: one with a `Cargo.toml` in the root or a folder above it.
//! Elsewhere it says why not.
//!
//! [`CargoParser`] reads its stdout, cargo's JSON messages, and its stderr,
//! cargo's own lines and the diagnostics it renders, apart, each line with
//! when it came. The run's summary is [`RunSummary::Build`], updated once a
//! batch of lines as artifacts and diagnostics arrive: the crates whose
//! artifacts have come, and the warnings and errors rendered so far. Once
//! the output ends the counts are the parser's. Its progress stays unknown.
//! Its output, the rendered diagnostics among it, is kept by its
//! [`RunProcess`] for the Output view ([`Workbench::build_process`]); the
//! build as the parser read it, once it's ended, for the waterfall
//! ([`Workbench::build_timeline`]). Both are dropped as
//! [`Runs`](crate::runs::Runs) drops the run.
//!
//! Once a build that wasn't cancelled ends, the messages the compiler gave a
//! file and a line become problems in the Problems panel, as one source with
//! the run's title, in place of what the last build listed, the way a task
//! tab's do ([`crate::task_run`]): a build's problems replace those of the
//! builds started before it, never those of one started after. Rustc's
//! columns count chars. A relative file is one in the root, or else in the
//! nearest folder above it with a `Cargo.toml` that has it: cargo gives
//! rustc paths from the workspace's root.
//!
//! Its text is the catalogue's (`l10n/en-US/build_runs.ftl`).

use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use fe_editor::diagnostics::{Problem, Severity};
use fe_runlog::{BuildMessage, BuildTimeline, CargoParser};
use fe_text::Text;
use gpui::{App, AppContext as _, Context, Entity, EntityId, Global, Subscription, Task, Window};
use kiln::Cx;
use kiln_l10n::{tr, verbatim};

use crate::{
    Workbench,
    dock::problems_panel::{ColumnUnit, SourceProblem, byte_column, read_text},
    run_process::{LineSink, RunProcess, RunSpec, RunUpdate, Stream},
    runs::{RunEnd, RunId, RunKind, RunSummary, RunsEvent},
    task_commands::{Files, project_files},
    task_problems::MAX_PROBLEMS,
    task_run::untrusted_message,
    test_commands::{no_folder, on_workbench},
    workbench::toast::{self, Toast},
};

/// The command's declaration: see the module's docs. It's handled by
/// [`init`]'s listener, as [`crate::test_runner`]'s are.
#[allow(dead_code)]
struct BuildRunCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl BuildRunCommands {
    /// Runs: Build Project
    #[command(id = "runs.build")]
    fn runs_build(&mut self, _: &mut Cx<Self>) {}
}

/// A Rust package's manifest's file name.
const CARGO_TOML: &str = verbatim!("Cargo.toml");

/// The longest line a build reassembles from its parts: a longer one is
/// dropped, said once in the log.
const MAX_MESSAGE_LINE: usize = 32 * 1024 * 1024;

/// The build as the parser read it, once its output has ended, shared with
/// the main thread.
type Timeline = Arc<Mutex<Option<BuildTimeline>>>;

fn lock<T>(shared: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    shared.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// What `runs.build` says in a folder with no `Cargo.toml` in or above it.
pub(crate) fn no_cargo_project() -> String {
    tr!("workbench-build-runs-no-cargo").into()
}

/// A build to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) spec: RunSpec,
    /// The folders with a `Cargo.toml`, the root's first if it has one, then
    /// those above it, nearest first: where a relative file is looked for.
    pub(crate) manifest_dirs: Vec<PathBuf>,
}

/// The build of `root`, its programs on `path` if one's given, else on the
/// login shell's as [`RunProcess`] finds it: why not, in words, if `root`
/// isn't in a Rust project. It reads the project's files, so it's for a
/// background thread.
pub(crate) fn plan(
    files: &dyn crate::task_defs::ProjectFiles,
    root: &Path,
    path: Option<OsString>,
) -> Result<Plan, String> {
    let manifest_dirs: Vec<PathBuf> = (root.ancestors())
        .filter(|dir| files.exists(&dir.join(CARGO_TOML)))
        .map(Path::to_owned)
        .collect();
    if manifest_dirs.is_empty() {
        return Err(no_cargo_project());
    }
    let folder = root.file_name().unwrap_or(root.as_os_str());
    let folder = folder.to_string_lossy().into_owned();
    let title = tr!("workbench-build-runs-title", folder = folder).into();
    let args = [
        "build",
        "--workspace",
        "--message-format=json-render-diagnostics",
    ];
    let mut env: Vec<(OsString, OsString)> = Vec::new();
    if let Some(path) = path {
        env.push((verbatim!("PATH").into(), path));
    }
    // Plain lines, whatever cargo's configured to do: the parser and the
    // Output view read them as text.
    env.push((
        verbatim!("CARGO_TERM_COLOR").into(),
        verbatim!("never").into(),
    ));
    let spec = RunSpec {
        kind: RunKind::Build,
        title,
        program: verbatim!("cargo").into(),
        args: args.map(OsString::from).into(),
        cwd: root.to_owned(),
        env,
        // The parser reads stdout's JSON and stderr's lines apart.
        merged: false,
    };
    Ok(Plan {
        spec,
        manifest_dirs,
    })
}

/// A diagnostic's first line as rustc renders it, `warning: text` or
/// `error[E0308]: text`: whether it's an error, its code if it has one, and
/// its text. Not one of the lines rustc and cargo print after a crate's
/// diagnostics, which count nothing, as [`CargoParser`] reads them.
fn diagnostic_header(line: &str) -> Option<(bool, Option<&str>, &str)> {
    if line.starts_with(' ') {
        return None;
    }
    let (level, text) = line.split_once(':')?;
    let code = (level.strip_prefix("error["))
        .and_then(|rest| rest.strip_suffix(']'))
        .filter(|code| !code.is_empty());
    let is_error = level == "error" || code.is_some();
    if !is_error && level != "warning" {
        return None;
    }
    let text = text.trim();
    (!is_summary(text)).then_some((is_error, code, text))
}

/// The lines rustc and cargo print after a crate's diagnostics: "aborting
/// due to 1 previous error", "2 warnings emitted", "`a` (lib) generated 2
/// warnings (…)" and "could not compile `a` (lib) due to 1 previous error",
/// as [`CargoParser`] tells them.
fn is_summary(text: &str) -> bool {
    let words: Vec<_> = text.split_whitespace().collect();
    let counted = |word: &str| matches!(word, "warning" | "warnings" | "error" | "errors");
    text.starts_with("aborting due to ")
        || text.starts_with("could not compile `")
        || matches!(words[..], [n, what, "emitted"] if n.parse::<u32>().is_ok() && counted(what))
        || (text.starts_with('`')
            && matches!(words.iter().position(|&w| w == "generated"),
                Some(at) if words.get(at + 1).is_some_and(|n| n.parse::<u32>().is_ok())
                    && words.get(at + 2).is_some_and(|&w| counted(w))))
}

/// The package a `compiler-artifact` message `line` is of.
fn artifact_package(line: &str) -> Option<String> {
    if !line
        .trim_start()
        .starts_with(r#"{"reason":"compiler-artifact""#)
    {
        return None;
    }
    let message: serde_json::Value = serde_json::from_str(line).ok()?;
    message["package_id"].as_str().map(str::to_owned)
}

/// Reads a build's lines into its parser and its counts, on the runner's
/// thread.
struct BuildSink {
    parser: CargoParser,
    /// The packages whose artifacts have come.
    built: HashSet<String>,
    /// The warnings and errors rendered so far.
    warnings: usize,
    errors: usize,
    /// Whether the counts changed in this batch.
    changed: bool,
    /// Each stream's parts read so far of a line longer than the runner
    /// reads as one, by [`stream_index`].
    partial: [String; 2],
    /// Whether that line has passed [`MAX_MESSAGE_LINE`]: it's dropped.
    overlong: [bool; 2],
    /// Where the build goes once its output's ended.
    timeline: Timeline,
}

fn stream_index(stream: Stream) -> usize {
    match stream {
        Stream::Stdout => 0,
        Stream::Stderr => 1,
    }
}

impl BuildSink {
    fn new(timeline: Timeline) -> Self {
        Self {
            parser: CargoParser::new(),
            built: HashSet::new(),
            warnings: 0,
            errors: 0,
            changed: false,
            partial: Default::default(),
            overlong: [false; 2],
            timeline,
        }
    }

    /// Feeds the parser one whole line from `stream`: `text`, after the
    /// parts before it.
    fn feed(&mut self, stream: Stream, text: &str, at: Duration) {
        let ix = stream_index(stream);
        if std::mem::take(&mut self.overlong[ix]) {
            self.partial[ix].clear();
            log::warn!("a build's line passed {MAX_MESSAGE_LINE} bytes: it's dropped");
            return;
        }
        let joined;
        let line = match self.partial[ix].is_empty() {
            true => text,
            false => {
                let mut line = std::mem::take(&mut self.partial[ix]);
                line.push_str(text);
                joined = line;
                joined.as_str()
            }
        };
        match stream {
            Stream::Stdout => {
                self.parser.feed_stdout(line, at);
                if let Some(package) = artifact_package(line) {
                    self.changed |= self.built.insert(package);
                }
            }
            Stream::Stderr => {
                self.parser.feed_stderr(line, at);
                if let Some((is_error, ..)) = diagnostic_header(line) {
                    match is_error {
                        true => self.errors += 1,
                        false => self.warnings += 1,
                    }
                    self.changed = true;
                }
            }
        }
    }

    fn summary(&self, warnings: usize, errors: usize) -> RunSummary {
        RunSummary::Build {
            crates: self.built.len(),
            warnings,
            errors,
        }
    }

    /// The counts, if they've changed since they were last said.
    fn updates(&mut self) -> Vec<RunUpdate> {
        if !std::mem::take(&mut self.changed) {
            return Vec::new();
        }
        vec![RunUpdate::Summary(self.summary(self.warnings, self.errors))]
    }
}

impl LineSink for BuildSink {
    fn line(&mut self, stream: Stream, text: &str, at: Duration) -> Vec<RunUpdate> {
        self.feed(stream, text, at);
        Vec::new()
    }

    fn fragment(&mut self, stream: Stream, text: &str, _: Duration) -> Vec<RunUpdate> {
        let ix = stream_index(stream);
        if self.partial[ix].len() + text.len() > MAX_MESSAGE_LINE {
            self.overlong[ix] = true;
            self.partial[ix].clear();
        } else if !self.overlong[ix] {
            self.partial[ix].push_str(text);
        }
        Vec::new()
    }

    fn batch_end(&mut self) -> Vec<RunUpdate> {
        self.updates()
    }

    /// The build as the parser read it, and its counts.
    fn end(&mut self) -> Vec<RunUpdate> {
        for stream in [Stream::Stdout, Stream::Stderr] {
            let ix = stream_index(stream);
            if !self.partial[ix].is_empty() || self.overlong[ix] {
                self.feed(stream, "", Duration::ZERO);
            }
        }
        let timeline = std::mem::take(&mut self.parser).finish();
        let summary = self.summary(timeline.warnings, timeline.errors);
        *lock(&self.timeline) = Some(timeline);
        vec![RunUpdate::Summary(summary)]
    }
}

/// `message`'s file: as it's named if that's absolute, else in the first of
/// `dirs` that has it, else in the first of them.
fn message_path(file: &str, dirs: &[PathBuf]) -> PathBuf {
    let file = Path::new(file);
    if file.is_absolute() {
        return file.to_owned();
    }
    (dirs.iter())
        .map(|dir| dir.join(file))
        .find(|path| path.is_file())
        .unwrap_or_else(|| {
            dirs.first()
                .map_or_else(|| file.to_owned(), |dir| dir.join(file))
        })
}

/// What the Problems panel says of `message`: its first line's text, with
/// its code as a task tab's rustc problems have it.
fn problem_message(message: &BuildMessage) -> (Severity, String) {
    let first = message.rendered.lines().next().unwrap_or_default();
    let severity = match message.level.starts_with("error") {
        true => Severity::Error,
        false => Severity::Warning,
    };
    let text = match diagnostic_header(first) {
        Some((_, Some(code), text)) => tr!(
            "workbench-lsp-tasks-problem-code",
            message = text,
            code = code
        )
        .into(),
        Some((_, None, text)) => text.to_owned(),
        None => first.to_owned(),
    };
    (severity, text)
}

/// The problems `messages` name a file and a line for, as the Problems panel
/// lists them from source `title`: the files they name are read, so it's for
/// a background thread. Each once; the first [`MAX_PROBLEMS`].
fn build_problems(messages: &[BuildMessage], title: &str, dirs: &[PathBuf]) -> Vec<SourceProblem> {
    let mut texts: HashMap<PathBuf, Option<Text>> = HashMap::new();
    let mut problems: Vec<SourceProblem> = Vec::new();
    for message in messages {
        let (Some(file), Some(line)) = (&message.file, message.line) else {
            continue;
        };
        let path = message_path(file, dirs);
        let row = u32::try_from(line.saturating_sub(1)).unwrap_or(u32::MAX);
        let column = message.column.unwrap_or(1).saturating_sub(1);
        let column = u32::try_from(column).unwrap_or(u32::MAX);
        let (severity, text) = problem_message(message);
        let file_text =
            (texts.entry(path.clone())).or_insert_with_key(|path| read_text(path.clone()));
        let unit = ColumnUnit::Chars;
        let file_column = (file_text.as_ref()).map_or(column, |file_text| {
            byte_column(file_text, row, column, unit)
        });
        let problem = SourceProblem {
            path,
            problem: Problem {
                range: 0..0,
                row,
                column,
                severity,
                message: text,
                source: Some(title.to_owned()),
                code: None,
                related: Vec::new(),
                code_href: None,
            },
            unit,
            file_column,
        };
        if !problems.contains(&problem) {
            problems.push(problem);
        }
        if problems.len() >= MAX_PROBLEMS {
            log::warn!("build {title}: listing its first {MAX_PROBLEMS} problems");
            break;
        }
    }
    problems
}

/// The `PATH` set for the tests' runs, under test.
fn runner_path(cx: &App) -> Option<OsString> {
    #[cfg(test)]
    if let Some(crate::test_runner::RunnerPath(path)) =
        cx.try_global::<crate::test_runner::RunnerPath>()
    {
        return Some(path.clone());
    }
    let _ = cx;
    None
}

/// One build this workbench ran, kept while [`Runs`](crate::runs::Runs)
/// keeps its run.
struct Build {
    run: RunId,
    process: Entity<RunProcess>,
    timeline: Timeline,
    /// The root it built, and its plan's folders with a `Cargo.toml`.
    root: PathBuf,
    manifest_dirs: Vec<PathBuf>,
    /// Its end has been seen.
    ended: bool,
    /// To its program, for its end.
    _ended: Subscription,
    /// Reading its problems' files.
    _listing: Option<Task<()>>,
}

/// The builds of each workbench's [`Runs`](crate::runs::Runs), by its
/// entity, and what the Problems panel lists of them.
#[derive(Default)]
struct BuildRuns(HashMap<EntityId, Kept>);

impl Global for BuildRuns {}

struct Kept {
    builds: Vec<Build>,
    /// The builds' source in the Problems panel, once one's listed.
    source: Option<u64>,
    /// The build whose problems it lists.
    listed: Option<RunId>,
    /// Drops the builds whose runs [`Runs`](crate::runs::Runs) no longer
    /// keeps, as it changes.
    _changed: Subscription,
    /// Drops them all, and so stops their programs, once the runs go with
    /// their workbench.
    _released: Subscription,
}

/// Drops the builds of the [`Runs`](crate::runs::Runs) `runs`, keyed by
/// `key`, whose runs it no longer keeps: their output and their timelines.
fn prune(key: EntityId, runs: &Entity<crate::runs::Runs>, cx: &mut App) {
    let listed = runs.read(cx);
    let gone: Vec<RunId> = match cx.try_global::<BuildRuns>().and_then(|b| b.0.get(&key)) {
        Some(kept) => (kept.builds.iter())
            .filter(|build| listed.get(build.run).is_none())
            .map(|build| build.run)
            .collect(),
        None => return,
    };
    if gone.is_empty() {
        return;
    }
    let dropped: Vec<Build> = match cx.global_mut::<BuildRuns>().0.get_mut(&key) {
        Some(kept) => {
            let (dropped, builds) = std::mem::take(&mut kept.builds)
                .into_iter()
                .partition(|build| gone.contains(&build.run));
            kept.builds = builds;
            dropped
        }
        None => Vec::new(),
    };
    drop(dropped);
}

impl Workbench {
    /// `runs.build`: saves the window's files, works out the build off the
    /// main thread, and starts it if the root is still the one it was for.
    /// A root that isn't trusted is refused with a toast offering to trust
    /// it.
    fn start_build(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                async move { plan(&*files, &root, path) }
            });
            let planned = planning.await;
            let started = workbench.update(cx, |workbench, cx| {
                if workbench.root() != Some(root.as_path()) {
                    return;
                }
                match planned {
                    Ok(plan) => workbench.begin_build(plan, root, cx),
                    Err(message) => toast::show(cx, Toast::info(message)),
                }
            });
            started.ok();
        })
        .detach();
    }

    /// Starts `plan`'s program for a new run, kept here.
    fn begin_build(&mut self, plan: Plan, root: PathBuf, cx: &mut Context<Self>) {
        let timeline = Timeline::default();
        let sink = BuildSink::new(timeline.clone());
        let starting = RunProcess::start(plan.spec, sink, self.runs().clone(), cx);
        let manifest_dirs = plan.manifest_dirs;
        cx.spawn(async move |workbench, cx| {
            let started = starting.await;
            let kept = workbench.update(cx, |workbench, cx| match started {
                Ok(process) => {
                    let run = process.read(cx).run();
                    let ended = cx.observe(&process, move |workbench, _, cx| {
                        workbench.build_changed(run, cx)
                    });
                    let build = Build {
                        run,
                        process,
                        timeline,
                        root,
                        manifest_dirs,
                        ended: false,
                        _ended: ended,
                        _listing: None,
                    };
                    workbench.keep_build(build, cx);
                    // It may have ended before it was observed.
                    workbench.build_changed(run, cx);
                }
                Err(refused) => toast::show(cx, Toast::warning(refused.message().to_owned())),
            });
            kept.ok();
        })
        .detach();
    }

    /// Keeps `build`, dropping those whose runs [`Runs`](crate::runs::Runs)
    /// no longer keeps.
    fn keep_build(&mut self, build: Build, cx: &mut Context<Self>) {
        let runs = self.runs().clone();
        let key = runs.entity_id();
        let app: &mut App = cx;
        let mut kept = match app.default_global::<BuildRuns>().0.remove(&key) {
            Some(kept) => kept,
            None => Kept {
                builds: Vec::new(),
                source: None,
                listed: None,
                _changed: app.subscribe(&runs, move |runs, _: &RunsEvent, cx| {
                    prune(key, &runs, cx);
                }),
                _released: app.observe_release(&runs, move |_, cx| {
                    if cx.has_global::<BuildRuns>() {
                        cx.global_mut::<BuildRuns>().0.remove(&key);
                    }
                }),
            },
        };
        kept.builds.push(build);
        app.global_mut::<BuildRuns>().0.insert(key, kept);
    }

    /// This workbench's builds and what the Problems panel lists of them.
    fn kept_builds<'a>(&self, cx: &'a App) -> Option<&'a Kept> {
        cx.try_global::<BuildRuns>()?
            .0
            .get(&self.runs().entity_id())
    }

    fn kept_builds_mut<'a>(&self, cx: &'a mut App) -> Option<&'a mut Kept> {
        let key = self.runs().entity_id();
        if !cx.has_global::<BuildRuns>() {
            return None;
        }
        cx.global_mut::<BuildRuns>().0.get_mut(&key)
    }

    /// The build `run`, while [`Runs`](crate::runs::Runs) keeps it.
    fn build<'a>(&self, run: RunId, cx: &'a App) -> Option<&'a Build> {
        self.runs().read(cx).get(run)?;
        (self.kept_builds(cx)?.builds.iter()).find(|build| build.run == run)
    }

    /// Once the build `run` has ended, and wasn't cancelled, reads its
    /// problems' files off the main thread, then lists them.
    fn build_changed(&mut self, run: RunId, cx: &mut Context<Self>) {
        let Some(build) = self.build(run, cx) else {
            return;
        };
        let end = build.process.read(cx).end();
        let Some(end) = end.filter(|_| !build.ended) else {
            return;
        };
        let root = build.root.clone();
        let dirs = build.manifest_dirs.clone();
        let timeline = build.timeline.clone();
        let title = (self.runs().read(cx).get(run)).map(|run| run.title.clone());
        if let Some(build) = (self.kept_builds_mut(cx))
            .and_then(|kept| kept.builds.iter_mut().find(|build| build.run == run))
        {
            build.ended = true;
        }
        if end == RunEnd::Cancelled || self.root() != Some(root.as_path()) {
            return;
        }
        // None if its program never started.
        let Some(messages) = lock(&timeline).as_ref().map(|t| t.messages.clone()) else {
            return;
        };
        let title = title.unwrap_or_default();
        let listing = cx.spawn(async move |workbench, cx| {
            let reading =
                cx.background_spawn(async move { build_problems(&messages, &title, &dirs) });
            let problems = reading.await;
            let listed = workbench.update(cx, |workbench, cx| {
                workbench.list_build_problems(run, &root, problems, cx)
            });
            listed.ok();
        });
        if let Some(build) = (self.kept_builds_mut(cx))
            .and_then(|kept| kept.builds.iter_mut().find(|build| build.run == run))
        {
            build._listing = Some(listing);
        }
    }

    /// Lists `problems`, those of the build `run` of `root`, in place of
    /// what the last build listed, unless the root's changed or a build
    /// started after it has listed its own.
    fn list_build_problems(
        &mut self,
        run: RunId,
        root: &Path,
        problems: Vec<SourceProblem>,
        cx: &mut Context<Self>,
    ) {
        if self.root() != Some(root) {
            return;
        }
        let Some(kept) = self.kept_builds(cx) else {
            return;
        };
        if kept.listed.is_some_and(|listed| listed > run) {
            return;
        }
        let panel = self.dock().read(cx).problems().clone();
        let source = match kept.source {
            Some(source) => source,
            None => panel.update(cx, |panel, _| panel.task_tabs.new_key()),
        };
        if let Some(kept) = self.kept_builds_mut(cx) {
            kept.source = Some(source);
            kept.listed = Some(run);
        }
        panel.update(cx, |panel, _| panel.set_source(source, problems));
        self.sync_problems(cx);
    }

    /// The program of the build that made `run`, and the output it's kept,
    /// its rendered diagnostics among it, while
    /// [`Runs`](crate::runs::Runs) keeps it: none if it isn't one.
    pub fn build_process(&self, run: RunId, cx: &App) -> Option<Entity<RunProcess>> {
        Some(self.build(run, cx)?.process.clone())
    }

    /// The build that made `run` as the parser read it, once its output has
    /// ended, while [`Runs`](crate::runs::Runs) keeps it: none if it isn't
    /// one, it was cancelled or its program never started.
    pub fn build_timeline(&self, run: RunId, cx: &App) -> Option<BuildTimeline> {
        lock(&self.build(run, cx)?.timeline).clone()
    }
}

/// Handles the command.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &RunsBuild, cx| {
        on_workbench(cx, |workbench, window, cx| {
            workbench.start_build(window, cx)
        });
    });
}
