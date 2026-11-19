//! Debug Test at Cursor (docs/decisions/r1b9dw3g0g-debugger.md §7 and §13;
//! card D13): `debug.test_at_cursor` debugs the test `tests.run_at_cursor`
//! would run, from the one [`ResolvedTest`] both resolve
//! ([`Workbench::resolve_test_then`], `test_runnables.rs`), and never from
//! its command line.
//!
//! - **Cargo** ([`TestFramework::Cargo`]): the test's target is built as
//!   Run would build it, with `--no-run` and cargo's JSON messages
//!   ([`build_argv`]), through the task runner's read-at-exit seam
//!   ([`Workbench::run_task_reading`]): the build's tab, its matcher into
//!   Problems. The test executable is the one cargo's `compiler-artifact`
//!   message names for that target's test build ([`test_artifact`]), and it
//!   is launched with `[name, "--exact", "--nocapture"]`, in the directory
//!   of the artifact's `manifest_path` with `CARGO_MANIFEST_DIR` set to it,
//!   as `cargo test` itself runs a test binary: not in the resolved test's
//!   `cwd`, which is where `cargo test` is invoked ([`cargo_config`]). A
//!   test in a module that the library and a binary could both hold names
//!   no one target, and is refused.
//! - **pytest** ([`TestFramework::Pytest`]): debugpy with `module =
//!   "pytest"` and the node id, in the resolved `cwd` ([`pytest_config`]).
//! - **Go** ([`TestFramework::GoTest`]) is refused, "Go tests need delve",
//!   until D19.
//!
//! The session starts as a launch configuration of its own, through
//! [`Workbench::start_debug_configuration`]: the folder's trust is checked
//! there, and again just before the launch. A newer Debug Test, or a new
//! root, gives up a build under way's launch.
//!
//! Its other ways in: the test lens's `Debug` item (`test_lens.rs`), which
//! sends the command the lens's row, and the failure detail's `Debug this
//! test` (`test_run_view.rs`, [`Workbench::debug_failed_test`]).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use fe_runlog::TestId;
use gpui::{AnyWindowHandle, AppContext as _, Context, EntityId, Global, Subscription, Window};
use kiln_l10n::{tr, verbatim};
use serde_json::{Map, Value};

use crate::{
    Workbench,
    debug_session::{DebuggerEvent, launch::is_artifact_message},
    launch_defs::{AdapterRef, Cwd, LaunchConfig, Program, Request, Source as ConfigSource},
    task_commands::project_files,
    task_defs::{Group, Source, Task, TaskCommand},
    task_run::TaskExit,
    test_commands::Target,
    test_run_view::test_sources,
    test_runnables::{
        CargoTarget, ResolvedTest, TestFramework, resolve_test, runnable_at, runnables,
    },
    workbench::toast::{self, Toast},
};

/// What a Go test says until D19 brings delve's test mode.
pub(crate) fn go_needs_delve() -> String {
    tr!("workbench-debug-test-go-needs-delve").into()
}

/// The environment variable cargo sets to a test's package directory.
const CARGO_MANIFEST_DIR: &str = verbatim!("CARGO_MANIFEST_DIR");

/// The most places a failed test's name is looked for in, to tell its
/// package's and target's from the others'.
const MOST_PLACES: usize = 64;

/// Each workbench's debug-test generation: what a debug test started at
/// its invocation is, which a newer debug test or any debug start (an
/// ordinary configuration's, the debugger's `Starting`) moves on. A debug
/// test whose generation moved on launches nothing.
#[derive(Default)]
struct DebugTests(HashMap<EntityId, Generation>);

struct Generation {
    current: u64,
    /// To the window's debugger, for its starts.
    _starts: Option<Subscription>,
}

impl Global for DebugTests {}

/// `cargo test`'s build, without running, of `package`'s `target`: cargo's
/// JSON messages on stdout, for the executable, and its diagnostics
/// rendered for the build's tab and the Problems panel.
pub(crate) fn build_argv(package: &str, target: &CargoTarget) -> Vec<String> {
    let mut argv: Vec<String> = ["cargo", "test", "-p", package].map(str::to_owned).into();
    argv.extend(target.options());
    argv.push(verbatim!("--no-run").to_owned());
    argv.push(verbatim!("--message-format=json-render-diagnostics").to_owned());
    argv
}

/// A test executable cargo built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TestArtifact {
    pub(crate) executable: PathBuf,
    /// The directory of its package's manifest, where `cargo test` runs it.
    pub(crate) package_dir: PathBuf,
}

/// The test executable cargo's `compiler-artifact` messages in `output`
/// name for `target`: the last test build (`profile.test`) of a target of
/// its kind and name with an executable, as a rebuild prints it again.
/// Lines that aren't JSON are skipped.
pub(crate) fn test_artifact(output: &str, target: &CargoTarget) -> Option<TestArtifact> {
    let (kinds, name): (&[&str], Option<&str>) = match target {
        CargoTarget::Lib => (
            &["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"],
            None,
        ),
        CargoTarget::Bin(name) => (&["bin"], Some(name)),
        CargoTarget::Test(name) => (&["test"], Some(name)),
        CargoTarget::Bench(name) => (&["bench"], Some(name)),
        CargoTarget::Example(name) => (&["example"], Some(name)),
    };
    let mut found = None;
    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let test = (message.get("profile"))
            .and_then(|profile| profile.get("test"))
            .and_then(Value::as_bool);
        if test != Some(true) {
            continue;
        }
        let Some(built) = message.get("target") else {
            continue;
        };
        let of_kind = (built.get("kind").and_then(Value::as_array)).is_some_and(|built| {
            (built.iter()).any(|kind| kind.as_str().is_some_and(|kind| kinds.contains(&kind)))
        });
        let named = name.is_none_or(|name| built.get("name").and_then(Value::as_str) == Some(name));
        if !of_kind || !named {
            continue;
        }
        let executable = message.get("executable").and_then(Value::as_str);
        let manifest = message.get("manifest_path").and_then(Value::as_str);
        let package_dir = manifest.and_then(|manifest| Path::new(manifest).parent());
        if let (Some(executable), Some(package_dir)) = (executable, package_dir) {
            found = Some(TestArtifact {
                executable: PathBuf::from(executable),
                package_dir: package_dir.to_owned(),
            });
        }
    }
    found
}

/// A configuration of `test`'s own, named for it, debugging with `adapter`
/// in `cwd`.
fn config(test: &ResolvedTest, adapter: AdapterRef, cwd: PathBuf) -> LaunchConfig {
    LaunchConfig {
        name: test.label.clone(),
        source: ConfigSource::Detected,
        adapter,
        request: Request::Launch,
        program: None,
        args: Vec::new(),
        cwd: Cwd::At(cwd),
        env: test.env.clone(),
        stop_on_entry: false,
        pre_launch: None,
        console: None,
        adapter_args: Map::new(),
        blockers: Vec::new(),
    }
}

/// How the Cargo test `name` (`exact` or not) in `test` debugs once cargo
/// built `artifact`: the executable with `[name, "--exact", "--nocapture"]`
/// in its package's directory, `CARGO_MANIFEST_DIR` set to it.
pub(crate) fn cargo_config(
    test: &ResolvedTest,
    name: &str,
    exact: bool,
    artifact: &TestArtifact,
) -> LaunchConfig {
    let mut args = Vec::new();
    if !name.is_empty() {
        args.push(name.to_owned());
    }
    if exact {
        args.push(verbatim!("--exact").to_owned());
    }
    args.push(verbatim!("--nocapture").to_owned());
    let dir = artifact.package_dir.clone();
    let mut config = config(test, AdapterRef::DefaultFor("rust".to_owned()), dir.clone());
    config.program = Some(Program::Written(
        artifact.executable.to_string_lossy().into_owned(),
    ));
    config.args = args;
    let dir = dir.to_string_lossy().into_owned();
    config.env.insert(CARGO_MANIFEST_DIR.to_owned(), dir);
    config
}

/// How the pytest test `node_id` in `test` debugs: debugpy running the
/// `pytest` module on it, in the resolved directory.
pub(crate) fn pytest_config(test: &ResolvedTest, node_id: &str) -> LaunchConfig {
    let adapter = verbatim!("debugpy");
    let adapter = AdapterRef::Named {
        written: adapter.to_owned(),
        name: Some(adapter.to_owned()),
    };
    let mut config = config(test, adapter, test.cwd.clone());
    config.args = vec![node_id.to_owned()];
    (config.adapter_args).insert(
        verbatim!("module").to_owned(),
        Value::String(verbatim!("pytest").to_owned()),
    );
    config
}

/// The task that builds `test`'s target, `argv`, where Run runs it.
fn build_task(test: &ResolvedTest, argv: Vec<String>) -> Task {
    Task {
        label: argv.join(" "),
        command: TaskCommand::Argv(argv),
        cwd: test.cwd.clone(),
        env: test.env.clone(),
        group: Some(Group::Build),
        default: false,
        source: Source::Cargo,
        problem_matcher: None,
    }
}

/// How a message names the Cargo test `name` of `test`: by its filter, else
/// (a whole file's) by its label.
fn shown_name(test: &ResolvedTest, name: &str) -> String {
    match name.is_empty() {
        true => test.label.clone(),
        false => name.to_owned(),
    }
}

/// The Rust test at `row` of the file at `path` under `root`, resolved as
/// Run would resolve it: why not, in words.
fn resolve_at(
    path: &Path,
    row: u32,
    root: &Path,
    files: &dyn crate::task_defs::ProjectFiles,
) -> Result<ResolvedTest, String> {
    let language = verbatim!("rust");
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let buffer = fe_buffer::Buffer::new(&text);
    let grammar = fe_syntax::language_by_name(language)
        .ok_or_else(|| tr!("workbench-lsp-tests-no-runner", language = language).to_string())?;
    let mut syntax = fe_syntax::Syntax::new(grammar, buffer.current());
    syntax.finish(syntax.parse_job().run());
    let found = runnables(language, &syntax, buffer.text());
    let runnable = runnable_at(&found, row).ok_or_else(crate::test_commands::no_test_here)?;
    resolve_test(files, language, runnable, path, root)
}

/// Whether `test` is the Cargo test `id`: its binary's id and its exact
/// name are the ones the run recorded.
fn is_test(test: &ResolvedTest, id: &TestId) -> bool {
    match &test.framework {
        TestFramework::Cargo {
            package,
            target: Some(target),
            name,
            exact: true,
        } => target.binary_id(package) == id.binary && *name == id.name,
        _ => false,
    }
}

/// The failed Cargo test `id`, resolved from its function under `root`:
/// the one place whose resolved test has `id`'s binary and name. Why not,
/// in words: none, or more than one.
fn resolve_failed(
    root: &Path,
    id: &TestId,
    files: &dyn crate::task_defs::ProjectFiles,
) -> Result<ResolvedTest, String> {
    let mut found: Vec<ResolvedTest> = Vec::new();
    for (path, row) in test_sources(root, id, MOST_PLACES) {
        let Ok(test) = resolve_at(&path, row, root, files) else {
            continue;
        };
        if is_test(&test, id) && !found.contains(&test) {
            found.push(test);
        }
    }
    let (name, binary) = (id.name.as_str(), id.binary.as_str());
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(tr!(
            "workbench-debug-test-not-found",
            name = name,
            binary = binary
        )
        .into()),
        _ => Err(tr!(
            "workbench-debug-test-ambiguous",
            name = name,
            binary = binary
        )
        .into()),
    }
}

impl Workbench {
    /// A new debug test's generation, which moves on any older one's. The
    /// window's debugger is followed from the first: its next start moves
    /// it on too.
    fn begin_debug_test(&mut self, cx: &mut Context<Self>) -> u64 {
        let key = cx.entity_id();
        let following = (cx.try_global::<DebugTests>())
            .and_then(|tests| tests.0.get(&key))
            .is_some_and(|generation| generation._starts.is_some());
        let starts = match following {
            true => None,
            false => self.debugger(cx).map(|debugger| {
                cx.subscribe(&debugger, |_, _, event: &DebuggerEvent, cx| {
                    if matches!(event, DebuggerEvent::Starting(_)) {
                        let key = cx.entity_id();
                        if let Some(generation) = cx.default_global::<DebugTests>().0.get_mut(&key)
                        {
                            generation.current += 1;
                        }
                    }
                })
            }),
        };
        let tests = cx.default_global::<DebugTests>();
        let generation = tests.0.entry(key).or_insert(Generation {
            current: 0,
            _starts: None,
        });
        generation.current += 1;
        if starts.is_some() {
            generation._starts = starts;
        }
        generation.current
    }

    /// Whether the debug test of `generation` is still the one to launch.
    fn debug_test_current(&self, generation: u64, cx: &Context<Self>) -> bool {
        (cx.try_global::<DebugTests>())
            .and_then(|tests| tests.0.get(&cx.entity_id()))
            .is_some_and(|current| current.current == generation)
    }

    /// `debug.test_at_cursor`: the test at the caret, or (`row`, from the
    /// test lens) at that row, resolved as Run resolves it, debugged.
    pub(crate) fn debug_test(
        &mut self,
        row: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = row.map_or(Target::Cursor, Target::Row);
        let generation = self.begin_debug_test(cx);
        self.resolve_test_then(target, window, cx, move |workbench, test, window, cx| {
            if workbench.debug_test_current(generation, cx) {
                workbench.debug_resolved(generation, test, window, cx)
            }
        });
    }

    /// The failure detail's Debug this test: the failed test `id`, found by
    /// its function under the root as Open test finds it, and resolved there
    /// as `debug.test_at_cursor` would resolve it; of the places its name
    /// could be, the one whose package, target and name are `id`'s. The
    /// window's files are saved first, as for Run.
    pub(crate) fn debug_failed_test(
        &mut self,
        id: TestId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.root().map(Path::to_owned) else {
            return toast::show(cx, Toast::info(crate::test_commands::no_folder()));
        };
        let generation = self.begin_debug_test(cx);
        let saving = self.save_all(window, cx);
        let files = project_files(cx);
        cx.spawn_in(window, async move |workbench, cx| {
            saving.await;
            let saved = workbench.update(cx, |workbench, cx| {
                workbench.root() == Some(root.as_path()) && workbench.all_saved(cx)
            });
            if !saved.unwrap_or(false) {
                return;
            }
            let resolving = cx.background_spawn({
                let root = root.clone();
                async move { resolve_failed(&root, &id, &*files) }
            });
            let resolved = resolving.await;
            let debugged = workbench.update_in(cx, |workbench, window, cx| {
                if workbench.root() != Some(root.as_path())
                    || !workbench.debug_test_current(generation, cx)
                {
                    return;
                }
                match resolved {
                    Ok(test) => workbench.debug_resolved(generation, test, window, cx),
                    Err(message) => toast::show(cx, Toast::info(message)),
                }
            });
            debugged.ok();
        })
        .detach();
    }

    /// Debugs `test`, the debug test of `generation`, by its framework: see
    /// the module's docs.
    fn debug_resolved(
        &mut self,
        generation: u64,
        test: ResolvedTest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(test)]
        crate::test_commands::consumed(cx, "debug", &test);
        match test.framework.clone() {
            TestFramework::GoTest { .. } => toast::show(cx, Toast::info(go_needs_delve())),
            TestFramework::Pytest { node_id } => {
                let config = pytest_config(&test, &node_id);
                self.start_debug_configuration(config, Vec::new(), window, cx);
            }
            TestFramework::Cargo {
                target: None, name, ..
            } => {
                let name = shown_name(&test, &name);
                let message = tr!("workbench-debug-test-which-target", name = name);
                toast::show(cx, Toast::info(message));
            }
            TestFramework::Cargo {
                package,
                target: Some(target),
                name,
                exact,
            } => {
                let root = self.root().map(Path::to_owned);
                let handle = window.window_handle();
                let task = build_task(&test, build_argv(&package, &target));
                let label = task.label.clone();
                let built = Built {
                    generation,
                    root,
                    test,
                    target,
                    name,
                    exact,
                    label,
                };
                self.run_task_reading(
                    task,
                    is_artifact_message,
                    Box::new(move |workbench, exit, cx| {
                        workbench.test_built(built, handle, exit, cx);
                    }),
                    window,
                    cx,
                );
            }
        }
    }

    /// Whether `built`'s debug test may still launch: still the newest, and
    /// in the root it was built for.
    fn built_current(&self, built: &Built, cx: &Context<Self>) -> bool {
        self.debug_test_current(built.generation, cx)
            && self.root().map(Path::to_owned) == built.root
    }

    /// A Cargo test's build exited, or (`None`) its tab closed or reran
    /// first: the test's executable is debugged, in the window `handle`,
    /// only if it exited 0 and named it, and the debug test is still
    /// current then and just before the launch.
    fn test_built(
        &mut self,
        built: Built,
        handle: AnyWindowHandle,
        exit: Option<TaskExit>,
        cx: &mut Context<Self>,
    ) {
        let Some(exit) = exit else {
            return;
        };
        if !self.built_current(&built, cx) {
            return;
        }
        if exit.code != Some(0) {
            let label = built.label.as_str();
            let message = tr!("workbench-debug-pre-launch-failed", label = label);
            return toast::show(cx, Toast::error(message));
        }
        let Some(artifact) = test_artifact(&exit.lines.join("\n"), &built.target) else {
            let name = shown_name(&built.test, &built.name);
            let message = tr!("workbench-debug-test-no-artifact", name = name);
            return toast::show(cx, Toast::error(message));
        };
        let config = cargo_config(&built.test, &built.name, built.exact, &artifact);
        // The workbench is being updated: its window is free once it isn't.
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    if let Some(Some(workbench)) = window.root::<Workbench>() {
                        workbench.update(cx, |workbench, cx| {
                            if workbench.built_current(&built, cx) {
                                workbench.start_debug_configuration(config, Vec::new(), window, cx)
                            }
                        });
                    }
                })
                .ok();
        });
    }
}

/// A Cargo test whose build runs: what it debugs once built.
struct Built {
    /// Its debug test's generation.
    generation: u64,
    /// The root it was built in.
    root: Option<PathBuf>,
    test: ResolvedTest,
    target: CargoTarget,
    name: String,
    exact: bool,
    /// The build's label.
    label: String,
}
