//! What a project can debug: the launch configurations its
//! `.tenmoku/launch.toml` defines, the ones its `.vscode/launch.json` has,
//! and the ones its files imply (docs/decisions/r1b9dw3g0g-debugger.md §4,
//! card D4). This is the data model only; nothing here starts a session or
//! runs a build, and a configuration comes from the project, so whoever
//! starts one has to hold the folder's trust first, detected ones too (a
//! cargo build runs build scripts).
//!
//! [`load`] reads through [`ProjectFiles`], as [`task_defs`](crate::task_defs)
//! does, so no test needs a real project.
//!
//! # The launch file
//!
//! ```toml
//! [[launch]]
//! name = "server"                    # required, unique in the file
//! adapter = "lldb-dap"               # an adapter's name or a VS Code `type`
//! request = "launch"                 # the default; "attach" is listed, not started
//! program = "${workspaceFolder}/target/debug/server"
//! args = ["--port", "8080"]          # an array only, never a shell string
//! cwd = "crates/server"              # relative to the root, the default
//! env = { RUST_LOG = "debug" }       # strings only
//! stop_on_entry = false
//! pre_launch_task = "build server"   # a task's label, the file's or detected
//! console = "integrated"             # "integrated" or "internal"
//!
//! [launch.adapter_args]              # passed to the adapter as written
//! sourceMap = { "/rustc/abc" = "/home/me/src/rust" }
//! ```
//!
//! An entry that can't be used is left out with a [`LaunchDiagnostic`]
//! naming it, by name or else by its place in the file (`#2`), and the rest
//! survive; so do the other sources when a file doesn't parse at all. A
//! later entry repeating a name is the one left out. A key this schema
//! doesn't have is reported but keeps its entry. `cwd` is checked by its
//! words alone: `..` can't climb past the root, but a symlink inside the
//! project isn't followed. A `cwd` with a variable other than a leading
//! `${workspaceFolder}` is checked when it starts ([`Cwd::Expand`]).
//! `pid`, `host` and `port` are attach's keys: recognised, and left for
//! card D20.
//!
//! # Listed but not startable
//!
//! Some configurations are listed with the reasons they can't start
//! ([`LaunchConfig::blockers`]), each also a diagnostic: an `attach`
//! request (until D20); an adapter, or a `launch.json` `type`, that fe-dap's
//! [`Registry`] doesn't know; a pre-launch task that isn't one of the
//! project's; and a variable that isn't one of §4's ([`variables`]), such as
//! `${command:…}` and `${input:…}`.
//!
//! # `.vscode/launch.json`
//!
//! Read only, as JSON with comments and trailing commas
//! ([`fe_config::jsonc`]). Its configurations come after the launch file's,
//! and a name the launch file has is the launch file's. `type` names the
//! adapter through fe-dap's aliases; `request`, `program`, `args`, `cwd`,
//! `env`, `stopOnEntry` and `preLaunchTask` are the typed fields, and every
//! other key goes to [`LaunchConfig::adapter_args`] as written. The sub-object
//! for this OS (`osx`, `linux` or `windows`) is laid over the configuration,
//! objects in it merged key by key, as VS Code does. `compounds`,
//! `serverReadyAction`, `postDebugTask` and `presentation` are ignored, with
//! a diagnostic. Tenmoku doesn't read `.vscode/tasks.json`, so a
//! `preLaunchTask` only it defines isn't found.
//!
//! # Detection
//!
//! From the root's own files, after both files' configurations, and left
//! out when one of them has the name:
//!
//! - `Cargo.toml` with a `[package]`: "Debug <bin>" per binary target, by
//!   cargo's rules for finding them (`src/main.rs`, `src/bin/`, `[[bin]]`,
//!   `autobins`), each built first by
//!   `cargo build --bin <bin> --message-format=json-render-diagnostics`,
//!   whose `compiler-artifact` message names the program
//!   ([`Program::CargoArtifact`]): a guess at `target/debug/<bin>` would miss
//!   `CARGO_TARGET_DIR`, profiles and workspaces. A workspace's members aren't
//!   looked into.
//! - A Python file in the active editor: "Debug current file", `${file}`
//!   under debugpy.
//!
//! Go's (D19) and Java's (D21b) come with their adapters.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

use fe_dap::{HostOs, Registry};
use kiln_l10n::tr;
use serde_json::{Map, Value};

pub use crate::task_defs::ProjectFiles;
use crate::task_defs::Task;

mod detect;
mod launch_json;
pub mod variables;

/// The launch file, from the project root.
pub const LAUNCH_FILE: &str = ".tenmoku/launch.toml";

/// VS Code's launch configurations, from the project root.
pub const LAUNCH_JSON: &str = ".vscode/launch.json";

/// The keys a `[[launch]]` entry can have.
const ENTRY_KEYS: [&str; 14] = [
    "name",
    "adapter",
    "request",
    "program",
    "args",
    "cwd",
    "env",
    "stop_on_entry",
    "pre_launch_task",
    "console",
    "adapter_args",
    // Attach's, for D20.
    "pid",
    "host",
    "port",
];

/// What [`load`] reads and checks against.
#[derive(Clone, Copy)]
pub struct Project<'a> {
    pub files: &'a dyn ProjectFiles,
    pub root: &'a Path,
    /// The adapters an `adapter` or `type` may name.
    pub adapters: &'a Registry,
    /// The project's tasks, as [`task_defs::load`](crate::task_defs::load)
    /// has them, that a pre-launch task names.
    pub tasks: &'a [Task],
    /// The active editor's language, by fe-syntax's names.
    pub active_language: Option<&'a str>,
    /// Whose sub-object of a `launch.json` configuration applies.
    pub os: HostOs,
}

impl<'a> Project<'a> {
    /// `root` with no tasks, no active editor, on this OS.
    pub fn new(files: &'a dyn ProjectFiles, root: &'a Path, adapters: &'a Registry) -> Self {
        Self {
            files,
            root,
            adapters,
            tasks: &[],
            active_language: None,
            os: HostOs::current(),
        }
    }

    #[must_use]
    pub fn with_tasks(mut self, tasks: &'a [Task]) -> Self {
        self.tasks = tasks;
        self
    }

    #[must_use]
    pub fn with_active_language(mut self, language: Option<&'a str>) -> Self {
        self.active_language = language;
        self
    }

    #[must_use]
    pub fn with_os(mut self, os: HostOs) -> Self {
        self.os = os;
        self
    }
}

/// One way to start a debug session.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchConfig {
    /// Its name, unique among a project's configurations.
    pub name: String,
    pub source: Source,
    pub adapter: AdapterRef,
    pub request: Request,
    pub program: Option<Program>,
    /// With their variables still to expand, as are `program`'s, `env`'s and
    /// `adapter_args`' strings.
    pub args: Vec<String>,
    pub cwd: Cwd,
    /// What it adds to the environment the debuggee inherits.
    pub env: BTreeMap<String, String>,
    pub stop_on_entry: bool,
    pub pre_launch: Option<PreLaunch>,
    /// `None` leaves it to the adapter.
    pub console: Option<Console>,
    /// Merged last into the launch arguments, as written.
    pub adapter_args: Map<String, Value>,
    /// Why it can't start, in words; empty when it can.
    pub blockers: Vec<String>,
}

impl LaunchConfig {
    /// Whether nothing stops it starting (trust aside).
    pub fn can_start(&self) -> bool {
        self.blockers.is_empty()
    }
}

/// Where a configuration came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// [`LAUNCH_FILE`].
    File,
    /// [`LAUNCH_JSON`].
    LaunchJson,
    /// What the project's files imply.
    Detected,
}

impl Source {
    /// The file it's from, for a diagnostic.
    fn file(self) -> &'static str {
        match self {
            Self::File | Self::Detected => LAUNCH_FILE,
            Self::LaunchJson => LAUNCH_JSON,
        }
    }
}

/// Which adapter debugs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterRef {
    /// By name or VS Code `type`, as written; `name` is the adapter it
    /// names, by fe-dap's name, or `None` when none answers to it.
    Named {
        written: String,
        name: Option<String>,
    },
    /// The language's default, chosen when it starts: for Rust, lldb-dap
    /// when found, else codelldb (§13 Q3).
    DefaultFor(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Launch,
    /// Listed, not started, until D20.
    Attach,
}

/// What it debugs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Program {
    /// As written, variables unexpanded.
    Written(String),
    /// The executable that the `compiler-artifact` message of
    /// [`PreLaunch::CargoBuild`]'s output names for the binary `bin` (D7).
    CargoArtifact { bin: String },
}

/// Where the debuggee runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cwd {
    /// The root, or a folder under it.
    At(PathBuf),
    /// As written, with variables that only a start can expand
    /// (`${fileDirname}`); checked then.
    Expand(String),
}

/// What runs before it starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreLaunch {
    /// A task, by label: the tasks file's or a detected one.
    Task(String),
    /// A cargo build, run as a task, whose output names the program; the
    /// program and its arguments.
    CargoBuild(Vec<String>),
}

/// Where the debuggee's output goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Console {
    /// A terminal tab, by the adapter's `runInTerminal` request.
    Integrated,
    /// The debug console.
    Internal,
}

/// A problem with a launch file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchDiagnostic {
    /// [`LAUNCH_FILE`] or [`LAUNCH_JSON`].
    pub file: &'static str,
    /// The entry it's about, by name or else its place in the file (`#2`);
    /// `None` for the file as a whole.
    pub entry: Option<String>,
    /// The whole problem in words, the file and the entry's name included.
    pub message: String,
}

impl LaunchDiagnostic {
    fn file(file: &'static str, problem: impl fmt::Display) -> Self {
        Self {
            file,
            entry: None,
            message: tr!(
                "workbench-debug-launch-file-problem",
                file = file,
                problem = problem.to_string()
            )
            .into(),
        }
    }

    fn entry(file: &'static str, entry: &str, problem: impl fmt::Display) -> Self {
        Self {
            file,
            entry: Some(entry.to_owned()),
            message: tr!(
                "workbench-debug-launch-entry-problem",
                file = file,
                entry = entry,
                problem = problem.to_string()
            )
            .into(),
        }
    }
}

/// The project's launch configurations, in order, and what was wrong with
/// its launch files.
pub fn load(project: &Project<'_>) -> (Vec<LaunchConfig>, Vec<LaunchDiagnostic>) {
    let mut diagnostics = Vec::new();
    let mut configs = file_configs(project, &mut diagnostics);
    let mut names: HashSet<String> = configs.iter().map(|config| config.name.clone()).collect();
    for config in launch_json::configs(project, &mut diagnostics) {
        if names.contains(&config.name) {
            diagnostics.push(LaunchDiagnostic::entry(
                LAUNCH_JSON,
                &config.name,
                tr!("workbench-debug-launch-shadowed", file = LAUNCH_FILE),
            ));
        } else {
            names.insert(config.name.clone());
            configs.push(config);
        }
    }
    for config in &mut configs {
        block(config, project, &mut diagnostics);
    }
    configs.extend(
        detect::configs(project)
            .into_iter()
            .filter(|config| !names.contains(&config.name)),
    );
    (configs, diagnostics)
}

/// The launch file's usable entries: none if it isn't there.
fn file_configs(
    project: &Project<'_>,
    diagnostics: &mut Vec<LaunchDiagnostic>,
) -> Vec<LaunchConfig> {
    let problem = |problem| LaunchDiagnostic::file(LAUNCH_FILE, problem);
    let Some(text) = read(project, LAUNCH_FILE, diagnostics) else {
        return Vec::new();
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let table = match toml::from_str::<toml::Table>(text) {
        Ok(table) => table,
        Err(error) => {
            diagnostics.push(problem(error.to_string()));
            return Vec::new();
        }
    };
    let Some(entries) = table.get("launch") else {
        return Vec::new();
    };
    let Some(entries) = entries.as_array() else {
        diagnostics.push(problem(tr!("workbench-debug-launch-not-an-array").into()));
        return Vec::new();
    };

    let mut configs = Vec::new();
    let mut names = HashSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let typed = typed_json(entry);
        let name = entry_name(&typed, index);
        let report = |problem| LaunchDiagnostic::entry(LAUNCH_FILE, &name, problem);
        for key in typed.as_object().into_iter().flat_map(Map::keys) {
            if !ENTRY_KEYS.contains(&key.as_str()) {
                diagnostics.push(report(tr!("workbench-debug-launch-unknown-key", key = key)));
            }
        }
        match parse_file_entry(&typed, entry.get("adapter_args"), project) {
            Ok(config) if names.insert(config.name.clone()) => configs.push(config),
            Ok(_) => diagnostics.push(report(tr!("workbench-debug-launch-repeated-name"))),
            Err(problem) => {
                diagnostics.push(report(tr!(
                    "workbench-debug-launch-left-out",
                    problem = problem
                )));
            }
        }
    }
    configs
}

/// A launch file's text: `None` when it isn't there, or, with a diagnostic,
/// can't be read.
fn read(
    project: &Project<'_>,
    file: &'static str,
    diagnostics: &mut Vec<LaunchDiagnostic>,
) -> Option<String> {
    match project.files.read_to_string(&project.root.join(file)) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            diagnostics.push(LaunchDiagnostic::file(file, error));
            None
        }
    }
}

/// What a diagnostic calls an entry: its name if it has one, else its place.
fn entry_name(entry: &Value, index: usize) -> String {
    entry
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map_or_else(|| format!("#{}", index + 1), str::to_owned)
}

/// An entry from its [`typed_json`] form, with its `adapter_args` as the
/// file wrote them.
fn parse_file_entry(
    entry: &Value,
    adapter_args: Option<&toml::Value>,
    project: &Project<'_>,
) -> Result<LaunchConfig, String> {
    let entry =
        (entry.as_object()).ok_or_else(|| problem(tr!("workbench-debug-launch-not-a-table")))?;
    let mut config = parse_common(entry, &FILE_KEYS, project)?;
    config.console = match entry.get("console") {
        None => None,
        Some(Value::String(console)) if console == "integrated" => Some(Console::Integrated),
        Some(Value::String(console)) if console == "internal" => Some(Console::Internal),
        Some(Value::String(console)) => {
            return Err(problem(tr!(
                "workbench-debug-launch-unknown-console",
                console = console.as_str()
            )));
        }
        Some(_) => return Err(not_a_string("console")),
    };
    config.adapter_args = match adapter_args {
        None => Map::new(),
        Some(toml::Value::Table(args)) => {
            let mut path = "adapter_args".to_owned();
            (args.iter())
                .map(|(key, value)| Ok((key.clone(), passthrough(value, &mut path, key)?)))
                .collect::<Result<_, String>>()?
        }
        Some(_) => {
            return Err(problem(tr!(
                "workbench-debug-launch-not-table",
                key = "adapter_args"
            )));
        }
    };
    Ok(config)
}

/// A file's names for the typed fields both files have.
struct Keys {
    adapter: &'static str,
    stop_on_entry: &'static str,
    pre_launch_task: &'static str,
    source: Source,
}

const FILE_KEYS: Keys = Keys {
    adapter: "adapter",
    stop_on_entry: "stop_on_entry",
    pre_launch_task: "pre_launch_task",
    source: Source::File,
};

/// The fields both files have, under `keys`' names, with no console,
/// passthrough or blockers yet.
fn parse_common(
    entry: &Map<String, Value>,
    keys: &Keys,
    project: &Project<'_>,
) -> Result<LaunchConfig, String> {
    let name = match entry.get("name") {
        None => return Err(problem(tr!("workbench-debug-launch-missing", key = "name"))),
        Some(name) => name.as_str().ok_or_else(|| not_a_string("name"))?,
    };
    if name.trim().is_empty() {
        return Err(problem(tr!("workbench-debug-launch-empty-name")));
    }
    let adapter = match entry.get(keys.adapter) {
        None => {
            return Err(problem(tr!(
                "workbench-debug-launch-missing",
                key = keys.adapter
            )));
        }
        Some(adapter) => adapter.as_str().ok_or_else(|| not_a_string(keys.adapter))?,
    };
    let request = match entry.get("request") {
        None => Request::Launch,
        Some(Value::String(request)) if request == "launch" => Request::Launch,
        Some(Value::String(request)) if request == "attach" => Request::Attach,
        Some(Value::String(request)) => {
            return Err(problem(tr!(
                "workbench-debug-launch-unknown-request",
                request = request.as_str()
            )));
        }
        Some(_) => return Err(not_a_string("request")),
    };
    let program = match entry.get("program") {
        None => None,
        Some(program) => Some(Program::Written(
            program
                .as_str()
                .ok_or_else(|| not_a_string("program"))?
                .to_owned(),
        )),
    };
    let args = match entry.get("args") {
        None => Vec::new(),
        Some(args) => strings(args)
            .ok_or_else(|| problem(tr!("workbench-debug-launch-not-strings", key = "args")))?,
    };
    let cwd = match entry.get("cwd") {
        None => Cwd::At(project.root.to_path_buf()),
        Some(cwd) => parse_cwd(
            cwd.as_str().ok_or_else(|| not_a_string("cwd"))?,
            project.root,
        )?,
    };
    let stop_on_entry = match entry.get(keys.stop_on_entry) {
        None => false,
        Some(stop) => stop.as_bool().ok_or_else(|| {
            problem(tr!(
                "workbench-debug-launch-not-boolean",
                key = keys.stop_on_entry
            ))
        })?,
    };
    let pre_launch = match entry.get(keys.pre_launch_task) {
        None => None,
        Some(task) => Some(PreLaunch::Task(
            task.as_str()
                .ok_or_else(|| not_a_string(keys.pre_launch_task))?
                .to_owned(),
        )),
    };
    Ok(LaunchConfig {
        name: name.to_owned(),
        source: keys.source,
        adapter: AdapterRef::Named {
            written: adapter.to_owned(),
            name: project.adapters.name_for(adapter),
        },
        request,
        program,
        args,
        cwd,
        env: parse_env(entry.get("env"), keys.source)?,
        stop_on_entry,
        pre_launch,
        console: None,
        adapter_args: Map::new(),
        blockers: Vec::new(),
    })
}

/// An entry's problem, as `parse_common` says it.
fn problem(text: gpui::SharedString) -> String {
    text.into()
}

fn not_a_string(key: &str) -> String {
    problem(tr!("workbench-debug-launch-not-string", key = key))
}

/// A JSON array's strings, if it's an array of nothing else.
fn strings(value: &Value) -> Option<Vec<String>> {
    (value.as_array()?)
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

/// `cwd`, resolved under the root when only a start could change it.
fn parse_cwd(cwd: &str, root: &Path) -> Result<Cwd, String> {
    const WORKSPACE: &str = "${workspaceFolder}";
    let relative = match cwd.strip_prefix(WORKSPACE) {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            rest.trim_start_matches(['/', '\\'])
        }
        Some(_) => return Ok(Cwd::Expand(cwd.to_owned())),
        None => cwd,
    };
    if relative.contains("${") {
        return Ok(Cwd::Expand(cwd.to_owned()));
    }
    resolve_cwd(root, relative)
        .map(Cwd::At)
        .ok_or_else(|| problem(tr!("workbench-debug-launch-cwd-outside", cwd = cwd)))
}

/// `cwd` under `root` with its `.` and `..` worked out, unless it leaves the
/// root or isn't relative.
fn resolve_cwd(root: &Path, cwd: &str) -> Option<PathBuf> {
    let mut resolved = root.to_path_buf();
    let mut depth = 0_usize;
    for component in Path::new(cwd).components() {
        match component {
            Component::Normal(name) => {
                resolved.push(name);
                depth += 1;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                depth = depth.checked_sub(1)?;
                resolved.pop();
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(resolved)
}

fn parse_env(env: Option<&Value>, source: Source) -> Result<BTreeMap<String, String>, String> {
    let Some(env) = env else {
        return Ok(BTreeMap::new());
    };
    let env = env.as_object().ok_or_else(|| {
        problem(match source {
            Source::LaunchJson => tr!("workbench-debug-launch-not-object", key = "env"),
            Source::File | Source::Detected => tr!("workbench-debug-launch-not-table", key = "env"),
        })
    })?;
    env.iter()
        .map(|(name, value)| {
            if name.is_empty() || name.contains('=') {
                return Err(problem(tr!(
                    "workbench-debug-launch-env-bad-name",
                    name = name
                )));
            }
            let value = value.as_str().ok_or_else(|| {
                problem(tr!("workbench-debug-launch-env-not-string", name = name))
            })?;
            Ok((name.clone(), value.to_owned()))
        })
        .collect()
}

/// A TOML entry as JSON, for its typed fields: a date or time, which TOML
/// has and JSON doesn't, is `null`, so a typed field holding one has the
/// wrong type (TOML has no `null` of its own).
fn typed_json(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(text) => Value::String(text.clone()),
        toml::Value::Integer(number) => Value::from(*number),
        toml::Value::Float(number) => {
            serde_json::Number::from_f64(*number).map_or(Value::Null, Value::Number)
        }
        toml::Value::Boolean(flag) => Value::Bool(*flag),
        toml::Value::Datetime(_) => Value::Null,
        toml::Value::Array(items) => Value::Array(items.iter().map(typed_json).collect()),
        toml::Value::Table(table) => Value::Object(
            table
                .iter()
                .map(|(key, value)| (key.clone(), typed_json(value)))
                .collect(),
        ),
    }
}

/// `value`, the key `key` under `path`, as the JSON an adapter is passed,
/// or why it can't be: JSON has no dates or times, nor infinite or NaN
/// numbers, and changing one into something else would pass the adapter
/// what the file didn't say.
fn passthrough(value: &toml::Value, path: &mut String, key: &str) -> Result<Value, String> {
    let len = path.len();
    path.push('.');
    path.push_str(key);
    let value = passthrough_at(value, path);
    path.truncate(len);
    value
}

fn passthrough_at(value: &toml::Value, path: &mut String) -> Result<Value, String> {
    let not_json = |path: &str| problem(tr!("workbench-debug-launch-not-json", key = path));
    Ok(match value {
        toml::Value::String(text) => Value::String(text.clone()),
        toml::Value::Integer(number) => Value::from(*number),
        toml::Value::Float(number) => {
            Value::Number(serde_json::Number::from_f64(*number).ok_or_else(|| not_json(path))?)
        }
        toml::Value::Boolean(flag) => Value::Bool(*flag),
        toml::Value::Datetime(_) => return Err(not_json(path)),
        toml::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let len = path.len();
                path.push_str(&format!("[{index}]"));
                let item = passthrough_at(item, path);
                path.truncate(len);
                out.push(item?);
            }
            Value::Array(out)
        }
        toml::Value::Table(table) => Value::Object(
            table
                .iter()
                .map(|(key, value)| Ok((key.clone(), passthrough(value, path, key)?)))
                .collect::<Result<_, String>>()?,
        ),
    })
}

/// Fills in why `config` can't start, each reason also a diagnostic.
fn block(
    config: &mut LaunchConfig,
    project: &Project<'_>,
    diagnostics: &mut Vec<LaunchDiagnostic>,
) {
    let mut blockers: Vec<String> = Vec::new();
    if config.request == Request::Attach {
        blockers.push(tr!("workbench-debug-launch-attach-unsupported").into());
    }
    if let AdapterRef::Named {
        written,
        name: None,
    } = &config.adapter
    {
        blockers.push(
            match config.source {
                Source::LaunchJson => {
                    tr!(
                        "workbench-debug-launch-no-adapter-for-type",
                        kind = written.as_str()
                    )
                }
                Source::File | Source::Detected => {
                    tr!(
                        "workbench-debug-launch-no-adapter-named",
                        adapter = written.as_str()
                    )
                }
            }
            .into(),
        );
    }
    if let Some(PreLaunch::Task(label)) = &config.pre_launch
        && !project.tasks.iter().any(|task| task.label == *label)
    {
        blockers.push(
            tr!(
                "workbench-debug-launch-task-not-found",
                task = label.as_str()
            )
            .into(),
        );
    }
    let mut texts: Vec<&str> = Vec::new();
    if let Some(Program::Written(program)) = &config.program {
        texts.push(program);
    }
    texts.extend(config.args.iter().map(String::as_str));
    if let Cwd::Expand(cwd) = &config.cwd {
        texts.push(cwd);
    }
    texts.extend(config.env.values().map(String::as_str));
    for value in config.adapter_args.values() {
        json_strings(value, &mut texts);
    }
    for text in texts {
        for reason in variables::problems(text) {
            if !blockers.contains(&reason) {
                blockers.push(reason);
            }
        }
    }
    let file = config.source.file();
    for reason in &blockers {
        diagnostics.push(LaunchDiagnostic::entry(
            file,
            &config.name,
            tr!(
                "workbench-debug-launch-cannot-start",
                problem = reason.as_str()
            ),
        ));
    }
    config.blockers = blockers;
}

/// Every string in `value`, however deep.
fn json_strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| json_strings(item, out)),
        Value::Object(map) => map.values().for_each(|item| json_strings(item, out)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
