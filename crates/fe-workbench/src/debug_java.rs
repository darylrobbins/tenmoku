//! Java debugging through jdtls (docs/decisions/r1b9dw3g0g-debugger.md
//! §13, card D21b; design calls in
//! docs/decisions/hjj9vcwd41-java-debugging.md).
//!
//! java-debug is a jdtls bundle, the jar
//! `com.microsoft.java.debug.plugin-<version>.jar`. Tenmoku never ships or
//! downloads it (EPL-1.0): it finds one ([`find_jar`]) and hands it to the
//! window's jdtls through fe-lsp's bundles (`LspManager::set_jdtls_bundles`),
//! which reach jdtls's next start at a root.
//!
//! - **Finding the jar**: each entry of `[debug.adapters.java] path`, a
//!   jar or a folder holding one, in order; then the well-known installs
//!   ([`well_known`]): VS Code's Debugger for Java extension (VS Code,
//!   Insiders and VSCodium's extension folders, newest version first), then
//!   Mason's `java-debug-adapter`. A folder with several jars gives the
//!   newest. The debugger looks again, off the main thread, whenever its
//!   settings are set, and only the latest look counts.
//! - **Detection**: the main classes jdtls lists
//!   (`vscode.java.resolveMainClass`, asked as the configurations are read,
//!   for at most [`MAIN_CLASS_WAIT`]) become "Debug <class>" configurations
//!   ([`crate::launch_defs`]).
//! - **Starting** a configuration of the built-in `java` adapter, after
//!   its pre-launch task: its main class, if it names none, from
//!   `vscode.java.resolveMainClass` (there must be one); its `classPaths`
//!   and `modulePaths`, unless it gives either, from
//!   `vscode.java.resolveClasspath`; then `vscode.java.startDebugSession`
//!   starts java-debug and answers its port, which the session connects to
//!   through fe-dap's `Port` launch kind. Each command goes to the jdtls
//!   running for the window's root (`LspManager::execute_command_at`).
//! - **Why it can't**: no jdtls for the root; jdtls without java-debug's
//!   commands, because the jar wasn't found, the user's
//!   `initialization_options` can't take it, or jdtls started before it
//!   was found (restart it); and jdtls's own errors. Each is said in the
//!   failed start's toast.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, Weak},
    time::Duration,
};

use fe_config::debug_settings::DebugSettings;
use fe_dap::registry::JAVA;
use fe_lsp::{BundlesRefused, JdtlsBundles, LspError, LspManager};
use futures::FutureExt as _;
use gpui::{App, AppContext as _, Context, Global, Task};
use kiln_l10n::{tr, verbatim};
use serde_json::{Map, Value, json};

use crate::{
    Workbench,
    launch_defs::{AdapterRef, JavaMain, LaunchConfig, variables},
};

/// The jdtls server, by fe-lsp's built-in name.
pub(crate) const JDTLS: &str = "jdtls";

/// Starts java-debug, answering its port.
pub const START_DEBUG_SESSION: &str = "vscode.java.startDebugSession";
/// Lists the main classes: `[{ mainClass, projectName, filePath }]`.
pub const RESOLVE_MAIN_CLASS: &str = "vscode.java.resolveMainClass";
/// A main class's `[modulePaths, classPaths]`, from its class and project.
pub const RESOLVE_CLASSPATH: &str = "vscode.java.resolveClasspath";

/// How long reading the configurations waits for jdtls's main classes,
/// which a project still importing may be slow to list: listed without
/// them after that.
pub const MAIN_CLASS_WAIT: Duration = Duration::from_secs(10);

/// java-debug's jar names start so.
const JAR_PREFIX: &str = "com.microsoft.java.debug.plugin-";

/// VS Code's Debugger for Java extension folders start so.
const EXTENSION_PREFIX: &str = "vscjava.vscode-java-debug-";

/// Where the well-known installs are looked for: the user's home, and the
/// XDG data folder Mason installs under. Tests set their own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JavaDebugHome {
    pub home: Option<PathBuf>,
    /// `$XDG_DATA_HOME`, else `<home>/.local/share`.
    pub data_home: Option<PathBuf>,
}

impl JavaDebugHome {
    /// This process's: `$HOME` and `$XDG_DATA_HOME`, each if absolute.
    pub fn from_env() -> Self {
        let absolute = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        let home = absolute(verbatim!("HOME"));
        let data_home = absolute(verbatim!("XDG_DATA_HOME")).or_else(|| {
            home.as_ref()
                .map(|home| home.join(".local").join(verbatim!("share")))
        });
        Self { home, data_home }
    }
}

impl Global for JavaDebugHome {}

/// Looks for the well-known installs in `home` from now on.
pub fn set_java_debug_home(cx: &mut App, home: JavaDebugHome) {
    cx.set_global(home);
}

fn java_debug_home(cx: &App) -> JavaDebugHome {
    (cx.try_global::<JavaDebugHome>().cloned()).unwrap_or_else(JavaDebugHome::from_env)
}

/// Where java-debug's jar was looked for, and what was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Jar {
    Found(PathBuf),
    /// `hint` says how to get one, in the active locale.
    NotFound {
        hint: String,
    },
}

/// One well-known place a jar may be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// A folder of editor extensions: the jar is in a Debugger for Java
    /// extension's `server/`, the newest extension first.
    Extensions(PathBuf),
    /// A folder holding the jar.
    Folder(PathBuf),
}

/// The well-known installs, in the order they're tried: VS Code's,
/// VS Code Insiders' and VSCodium's extensions, then Mason's
/// `java-debug-adapter` package.
pub fn well_known(home: &JavaDebugHome) -> Vec<Place> {
    let mut places = Vec::new();
    if let Some(home) = &home.home {
        for editor in [".vscode", ".vscode-insiders", ".vscode-oss"] {
            places.push(Place::Extensions(home.join(editor).join("extensions")));
        }
    }
    if let Some(data) = &home.data_home {
        let mason = data.join("nvim").join("mason").join("packages");
        let package = mason.join("java-debug-adapter").join("extension");
        places.push(Place::Folder(package.join("server")));
    }
    places
}

/// java-debug's jar: from `settings`' `[debug.adapters.java] path`, each
/// entry a jar or a folder holding one (`~/` is the home), in order; else
/// from the [`well_known`] installs; else not found, with a hint. Reads
/// folders, so off the main thread.
pub fn find_jar(settings: &DebugSettings, home: &JavaDebugHome) -> Jar {
    let paths = (settings.adapters.get(JAVA))
        .and_then(|adapter| adapter.path.as_deref())
        .unwrap_or_default();
    for entry in paths {
        let path = match (entry.strip_prefix("~/"), &home.home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(entry),
        };
        if path.is_file() {
            return Jar::Found(path);
        }
        if let Some(jar) = newest_jar(&path) {
            return Jar::Found(jar);
        }
        log::warn!("debug.adapters.java.path's {entry} holds no java-debug jar");
    }
    for place in well_known(home) {
        let jar = match &place {
            Place::Folder(folder) => newest_jar(folder),
            Place::Extensions(folder) => {
                let mut extensions = versioned(folder, EXTENSION_PREFIX, "");
                extensions.sort_by(|a, b| b.0.cmp(&a.0));
                (extensions.into_iter())
                    .filter(|(_, path)| path.is_dir())
                    .find_map(|(_, path)| newest_jar(&path.join("server")))
            }
        };
        if let Some(jar) = jar {
            return Jar::Found(jar);
        }
    }
    Jar::NotFound {
        hint: tr!("workbench-debug-java-no-jar").into(),
    }
}

/// The newest java-debug jar in `folder`, if any.
fn newest_jar(folder: &Path) -> Option<PathBuf> {
    let mut jars = versioned(folder, JAR_PREFIX, ".jar");
    jars.retain(|(_, path)| path.is_file());
    jars.into_iter()
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, path)| path)
}

/// The entries of `folder` named `<prefix><version><suffix>`, with their
/// versions.
fn versioned(folder: &Path, prefix: &str, suffix: &str) -> Vec<(Version, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    (entries.flatten())
        .filter_map(|entry| {
            let name = entry.file_name();
            let version = name.to_str()?.strip_prefix(prefix)?.strip_suffix(suffix)?;
            Some((Version::parse(version), entry.path()))
        })
        .collect()
}

/// A version as its parts compare: numbers as numbers, the rest as text
/// (`0.10.0` after `0.9.1`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version(Vec<Part>);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Text(String),
    Number(u64),
}

impl Version {
    fn parse(text: &str) -> Self {
        Self(
            text.split(['.', '-'])
                .map(|part| match part.parse() {
                    Ok(number) => Part::Number(number),
                    Err(_) => Part::Text(part.to_owned()),
                })
                .collect(),
        )
    }
}

/// The debugger's java-debug jar: the last look's, and the look under way.
#[derive(Default)]
pub(crate) struct JavaJar {
    /// `None` until the first look ends.
    pub(crate) jar: Option<Jar>,
    generation: u64,
    _finding: Option<Task<()>>,
}

impl JavaJar {
    /// The bundles jdtls gets: the jar, if found; none if not, or not
    /// looked for yet.
    pub(crate) fn bundles(&self) -> Vec<PathBuf> {
        match &self.jar {
            Some(Jar::Found(jar)) => vec![jar.clone()],
            _ => Vec::new(),
        }
    }
}

/// Whether `config` debugs through jdtls: its adapter is the built-in
/// `java`.
pub(crate) fn is_java(config: &LaunchConfig) -> bool {
    matches!(&config.adapter, AdapterRef::Named { name: Some(name), .. } if name == JAVA)
}

/// The main classes in jdtls's answer to [`RESOLVE_MAIN_CLASS`].
pub(crate) fn main_classes(answer: &Value) -> Option<Vec<JavaMain>> {
    (answer.as_array()?.iter())
        .map(|item| {
            let class = item.get("mainClass")?.as_str()?.to_owned();
            let project = (item.get("projectName").and_then(Value::as_str))
                .filter(|project| !project.is_empty())
                .map(str::to_owned);
            Some(JavaMain { class, project })
        })
        .collect()
}

/// `[modulePaths, classPaths]` from jdtls's answer to
/// [`RESOLVE_CLASSPATH`].
fn classpath(answer: &Value) -> Option<(Value, Value)> {
    let paths = |value: &Value| {
        let items = value.as_array()?;
        items.iter().all(Value::is_string).then(|| value.clone())
    };
    match answer.as_array()?.as_slice() {
        [modules, classes] => Some((paths(modules)?, paths(classes)?)),
        _ => None,
    }
}

/// `args` as one string, as java-debug reads `args` and `vmArgs` and as
/// vscode-java-debug joins them: by spaces, each holding a quote, a
/// backslash before a quote, or white space in double quotes with its
/// quotes and backslashes escaped.
pub(crate) fn joined_args(args: &[String]) -> String {
    let quoted = args.iter().map(|arg| {
        if arg
            .chars()
            .any(|c| c == '"' || c == '\'' || c.is_whitespace())
        {
            let mut quoted = String::with_capacity(arg.len() + 2);
            quoted.push('"');
            for c in arg.chars() {
                if matches!(c, '"' | '\'' | '\\') {
                    quoted.push('\\');
                }
                quoted.push(c);
            }
            quoted.push('"');
            quoted
        } else {
            arg.clone()
        }
    });
    quoted.collect::<Vec<_>>().join(" ")
}

/// What a start learned from jdtls: java-debug's port, and the launch
/// arguments jdtls resolved (`mainClass`, `projectName`, `classPaths`,
/// `modulePaths`), which the configuration's own adapter arguments
/// override.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JavaLaunch {
    pub(crate) port: u16,
    pub(crate) arguments: Map<String, Value>,
}

/// A start's questions for jdtls, asked off the main thread.
pub(crate) struct JavaStart {
    manager: Weak<Mutex<LspManager>>,
    root: PathBuf,
    /// The configuration's, expanded.
    main_class: Option<String>,
    project: Option<String>,
    /// Whether it gives `classPaths` or `modulePaths` itself.
    has_paths: bool,
    /// For explaining jdtls without java-debug.
    jar: Option<Jar>,
    bundles: JdtlsBundles,
}

impl JavaStart {
    /// Asks jdtls, in turn, for the main class if needed, the classpath if
    /// needed, and java-debug's port.
    pub(crate) async fn run(self) -> Result<JavaLaunch, String> {
        let mut arguments = Map::new();
        let (class, project) = match self.main_class.clone() {
            Some(class) => (class, self.project.clone()),
            None => {
                let answer = (self.execute(RESOLVE_MAIN_CLASS, Vec::new())?.await)
                    .map_err(|error| self.explain(error, RESOLVE_MAIN_CLASS, Step::MainClass))?;
                let mains = main_classes(&answer).ok_or_else(|| bad_answer(RESOLVE_MAIN_CLASS))?;
                match mains.as_slice() {
                    [] => return Err(tr!("workbench-debug-java-no-main-class").into()),
                    [main] => (
                        main.class.clone(),
                        self.project.clone().or_else(|| main.project.clone()),
                    ),
                    several => {
                        let classes: Vec<&str> = several.iter().map(|m| m.class.as_str()).collect();
                        let classes = classes.join(", ");
                        return Err(
                            tr!("workbench-debug-java-main-classes", classes = classes).into()
                        );
                    }
                }
            }
        };
        if !self.has_paths {
            let project_arg = project.clone().map_or(Value::Null, Value::String);
            let args = vec![json!(class), project_arg];
            let answer = (self.execute(RESOLVE_CLASSPATH, args)?.await)
                .map_err(|error| self.explain(error, RESOLVE_CLASSPATH, Step::Classpath(&class)))?;
            let (modules, classes) =
                classpath(&answer).ok_or_else(|| bad_answer(RESOLVE_CLASSPATH))?;
            arguments.insert(verbatim!("modulePaths").into(), modules);
            arguments.insert(verbatim!("classPaths").into(), classes);
        }
        let answer = (self.execute(START_DEBUG_SESSION, Vec::new())?.await)
            .map_err(|error| self.explain(error, START_DEBUG_SESSION, Step::Start))?;
        let port = (answer.as_u64())
            .and_then(|port| u16::try_from(port).ok())
            .filter(|&port| port != 0)
            .ok_or_else(|| bad_answer(START_DEBUG_SESSION))?;
        arguments.insert(verbatim!("mainClass").into(), Value::String(class));
        if let Some(project) = project {
            arguments.insert(verbatim!("projectName").into(), Value::String(project));
        }
        Ok(JavaLaunch { port, arguments })
    }

    /// `command` with `arguments` sent to the root's jdtls.
    fn execute(
        &self,
        command: &str,
        arguments: Vec<Value>,
    ) -> Result<impl Future<Output = Result<Value, LspError>> + use<>, String> {
        let manager = self.manager.upgrade().ok_or_else(|| self.no_jdtls())?;
        let manager = manager.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(manager
            .execute_command_at(JDTLS, &self.root, command, arguments)
            .boxed())
    }

    fn no_jdtls(&self) -> String {
        let root = self.root.display().to_string();
        tr!("workbench-debug-java-no-jdtls", root = root).into()
    }

    /// Why `command`, the start's `step`, failed with `error`, in words.
    fn explain(&self, error: LspError, command: &str, step: Step<'_>) -> String {
        match error {
            LspError::ServerDown => self.no_jdtls(),
            // Not one of jdtls's commands: java-debug isn't loaded.
            LspError::Unavailable(_) => {
                let root = self.root.display().to_string();
                match (&self.jar, self.bundles.refused) {
                    (Some(Jar::NotFound { hint }), _) => hint.clone(),
                    (None, _) => tr!("workbench-debug-java-no-jar").into(),
                    (_, Some(BundlesRefused::OptionsNotATable)) => {
                        tr!("workbench-debug-java-options-not-table").into()
                    }
                    (_, Some(BundlesRefused::BundlesNotAList)) => {
                        tr!("workbench-debug-java-bundles-not-list").into()
                    }
                    (Some(Jar::Found(_)), None) if self.bundles.stale.contains(&self.root) => {
                        tr!("workbench-debug-java-restart", root = root).into()
                    }
                    (Some(Jar::Found(jar)), None) => tr!(
                        "workbench-debug-java-not-loaded",
                        command = command,
                        jar = jar.display().to_string()
                    )
                    .into(),
                }
            }
            error => {
                let error = error.to_string();
                match step {
                    Step::MainClass => {
                        tr!("workbench-debug-java-main-class-failed", error = error)
                    }
                    Step::Classpath(class) => tr!(
                        "workbench-debug-java-classpath-failed",
                        class = class,
                        error = error
                    ),
                    Step::Start => tr!("workbench-debug-java-start-failed", error = error),
                }
                .into()
            }
        }
    }
}

/// Which of a start's questions failed.
enum Step<'a> {
    MainClass,
    Classpath(&'a str),
    Start,
}

fn bad_answer(command: &str) -> String {
    tr!("workbench-debug-java-bad-answer", command = command).into()
}

/// A string field of a configuration's adapter arguments, expanded.
fn adapter_string(
    config: &LaunchConfig,
    key: &str,
    values: &variables::Values,
) -> Result<Option<String>, String> {
    match config.adapter_args.get(key).and_then(Value::as_str) {
        Some(text) => variables::expand(text, values).map(Some),
        None => Ok(None),
    }
}

impl Workbench {
    /// Looks for java-debug's jar again, off the main thread, with the
    /// debugger's settings, and hands what it finds to jdtls: the window's
    /// manager now, and each one made after. Only the latest look counts.
    pub(crate) fn find_java_jar(&mut self, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let settings = debugger.read(cx).settings().clone();
        let home = java_debug_home(cx);
        let generation = debugger.update(cx, |debugger, _| {
            debugger.java.generation += 1;
            debugger.java.generation
        });
        let finding = cx.background_spawn(async move { find_jar(&settings, &home) });
        let task = cx.spawn(async move |workbench, cx| {
            let jar = finding.await;
            let found = workbench.update(cx, |workbench, cx| {
                let Some(debugger) = workbench.debugger(cx) else {
                    return;
                };
                let current = debugger.update(cx, |debugger, _| {
                    let current = debugger.java.generation == generation;
                    if current {
                        debugger.java.jar = Some(jar);
                    }
                    current
                });
                if current {
                    let bundles = debugger.read(cx).java.bundles();
                    workbench.set_java_bundles(bundles);
                }
            });
            found.ok();
        });
        debugger.update(cx, |debugger, _| debugger.java._finding = Some(task));
    }

    /// The bundles a new manager's jdtls gets.
    pub(crate) fn java_bundles(&self, cx: &Context<Self>) -> Vec<PathBuf> {
        (self.debugger(cx)).map_or_else(Vec::new, |debugger| debugger.read(cx).java.bundles())
    }

    /// jdtls's main classes at `root`, the window's, for the configurations
    /// being read: none without a jdtls running there (or one not answering
    /// within [`MAIN_CLASS_WAIT`]).
    pub(crate) fn java_main_classes(
        &self,
        root: &Path,
        cx: &Context<Self>,
    ) -> Option<impl Future<Output = Vec<JavaMain>> + Send + 'static + use<>> {
        if !self.root_trusted() {
            return None;
        }
        let manager = self.lsp_manager_at(root)?;
        let asked = (manager.lock().unwrap_or_else(PoisonError::into_inner)).execute_command_at(
            JDTLS,
            root,
            RESOLVE_MAIN_CLASS,
            Vec::new(),
        );
        let timer = cx.background_executor().timer(MAIN_CLASS_WAIT);
        Some(async move {
            match futures::future::select(asked.boxed(), timer).await {
                futures::future::Either::Left((Ok(answer), _)) => {
                    main_classes(&answer).unwrap_or_default()
                }
                futures::future::Either::Left((Err(error), _)) => {
                    log::debug!("no main classes from jdtls: {error}");
                    Vec::new()
                }
                futures::future::Either::Right(_) => {
                    log::debug!("jdtls didn't list its main classes in time");
                    Vec::new()
                }
            }
        })
    }

    /// The questions start `config` asks jdtls at `root`: its main class,
    /// project and whether it gives its paths, expanded by `values`.
    pub(crate) fn java_start(
        &self,
        root: &Path,
        config: &LaunchConfig,
        values: &variables::Values,
        cx: &Context<Self>,
    ) -> Result<JavaStart, String> {
        let main_class = adapter_string(config, "mainClass", values)?;
        let project = adapter_string(config, "projectName", values)?;
        let has_paths = ["classPaths", "modulePaths"]
            .iter()
            .any(|key| config.adapter_args.contains_key(*key));
        let manager = self.lsp_manager_at(root);
        let bundles = (manager.as_ref())
            .map(|manager| {
                let mut manager = manager.lock().unwrap_or_else(PoisonError::into_inner);
                manager.jdtls_bundles()
            })
            .unwrap_or_default();
        let jar = (self.debugger(cx)).and_then(|debugger| debugger.read(cx).java.jar.clone());
        Ok(JavaStart {
            manager: manager.as_ref().map_or_else(Weak::new, Arc::downgrade),
            root: root.to_owned(),
            main_class,
            project,
            has_paths,
            jar,
            bundles,
        })
    }
}
