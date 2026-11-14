//! What a project can run: the tasks its `.tenmoku/tasks.toml` defines and
//! the ones its build files imply. This is the data model only; nothing here
//! runs a task, and a task's command comes from the project, so whoever runs
//! one has to hold the folder's trust first.
//!
//! [`load`] reads through [`ProjectFiles`], so no test needs a real project.
//!
//! # The tasks file
//!
//! ```toml
//! [[task]]
//! label = "server"                 # required, unique in the file
//! command = "cargo run -p server"  # a string runs in the shell...
//! # command = ["cargo", "run"]     # ...an array runs directly
//! cwd = "crates/server"            # relative to the root, which is the default
//! env = { RUST_LOG = "debug" }     # strings only
//! group = "run"                    # build, test or run
//! default = true                   # the group's default task
//! problem_matcher = "rustc"        # carried as is, for whoever reads output
//! ```
//!
//! An entry that can't be used is left out with a [`TaskDiagnostic`] naming
//! it, by label or else by its place in the file (`#2`), and the rest
//! survive. So do the detected tasks when the file doesn't parse at all. A
//! later entry repeating a label is the one left out. A key this schema
//! doesn't have is reported but keeps its entry, so a typo in `defualt` is
//! seen and nothing else is lost. `cwd` is checked by its words alone: `..`
//! can't climb past the root, but a symlink inside the project isn't followed.
//!
//! # Detection
//!
//! Only the root's own files count, with no recursion and no workspace
//! members, except that a .NET solution is also looked for one folder down
//! (`src/`), in this order, each in its file's order:
//!
//! - `Cargo.toml`: `cargo build`, `cargo test`, `cargo run`, `cargo clippy`,
//!   for a package or a `[workspace]` alike, so it's only looked for.
//! - `package.json`: each script, as `npm run <name>`, or by the lockfile
//!   beside it `pnpm run <name>`, `yarn <name>` or `bun run <name>`.
//! - `build.gradle`, `build.gradle.kts`, `settings.gradle` or
//!   `settings.gradle.kts`: `build`, `test`, `run`, `clean` and `check`, as
//!   `./gradlew <task>` when a `gradlew` is beside it, else `gradle <task>`.
//! - `pom.xml`: `compile`, `test`, `package`, `verify` and `clean`, as
//!   `./mvnw <goal>` when an `mvnw` is beside it, else `mvn <goal>`.
//! - .NET: each `.sln` or `.slnx` at the root, or else in any immediate
//!   subfolder, as `dotnet build|test|clean|restore <solution>`
//!   (the path relative to the root). With no solution, each root `.csproj`,
//!   `.fsproj` or `.vbproj` gets the same four plus `dotnet run --project
//!   <project>`. With a solution, `run` is left out: finding which projects
//!   are executables would mean reading every project file.
//! - `GNUmakefile`, `makefile` or `Makefile`, the first of them as `make`
//!   picks: each target as `make <target>`, by a scan of its lines.
//! - `justfile` or `Justfile`: each public recipe as `just <recipe>`.
//!
//! Detected commands are always argument lists, so a name in a project's file
//! never reaches a shell. A build file that doesn't parse just yields nothing:
//! it isn't this file's to report on.
//!
//! Detection is best-effort: a makefile is scanned by its lines, not run.
//! A name starting with `-` is passed after `--`, so it's a task, not an
//! option, and its label stays the command as written.
//!
//! # Order and defaults
//!
//! File tasks come first, then detected ones, and a file task replaces the
//! detected one with its label. Each of the groups has at most one default:
//! the first marked, with a diagnostic for any later one. Failing that, the
//! build and test groups fall to the first detected Cargo, npm, Gradle, Maven or .NET task named
//! so, in that order, so Gradle's default beats Maven's when a root has both. `default` on a task with no group has nothing to apply to, so it's
//! cleared with a diagnostic.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

use kiln_l10n::{tr, verbatim};
use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};

/// The tasks file, from the project root.
pub const TASKS_FILE: &str = ".tenmoku/tasks.toml";

/// The keys a `[[task]]` entry can have.
const ENTRY_KEYS: [&str; 7] = [
    "label",
    "command",
    "cwd",
    "env",
    "group",
    "default",
    "problem_matcher",
];

/// The makefile names, in the order `make` looks for them.
const MAKEFILES: [&str; 3] = ["GNUmakefile", "makefile", verbatim!("Makefile")];

const JUSTFILES: [&str; 2] = ["justfile", verbatim!("Justfile")];

/// What opens a string in a justfile, the longest first.
const JUST_QUOTES: [&str; 6] = ["\"\"\"", "'''", "```", "\"", "'", "`"];

/// A make line's first word that makes it something other than a rule.
const MAKE_DIRECTIVES: [&str; 14] = [
    "ifeq", "ifneq", "ifdef", "ifndef", "else", "endif", "include", "-include", "sinclude", "load",
    "-load", "vpath", "unexport", "undefine",
];

/// The file reads [`load`] needs, on full paths.
pub trait ProjectFiles {
    /// The file's text.
    ///
    /// # Errors
    ///
    /// When it can't be read: [`io::ErrorKind::NotFound`] if it isn't there.
    fn read_to_string(&self, path: &Path) -> io::Result<String>;

    /// Whether a file is at `path`.
    fn exists(&self, path: &Path) -> bool;

    /// The entries (files and folders) directly in `dir`; empty when it
    /// isn't a readable folder, or when the source can't list.
    fn list_dir(&self, _dir: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
}

/// [`ProjectFiles`] on the real disk. Tests can't name it: they all fake the
/// seam.
#[cfg(not(test))]
#[derive(Clone, Copy, Debug, Default)]
pub struct DiskFiles;

#[cfg(not(test))]
impl ProjectFiles for DiskFiles {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn list_dir(&self, dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
            .unwrap_or_default()
    }
}

/// One thing a project can run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    /// Its name, unique among a project's tasks.
    pub label: String,
    pub command: TaskCommand,
    /// Where it runs: the root, or the entry's `cwd` under it.
    pub cwd: PathBuf,
    /// What it adds to the environment it inherits.
    pub env: BTreeMap<String, String>,
    pub group: Option<Group>,
    /// Whether it's its group's default; see the module's docs.
    pub default: bool,
    pub source: Source,
    /// The file's name for how to read its output, not interpreted here.
    pub problem_matcher: Option<String>,
}

/// How a task's command runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskCommand {
    /// A line for the shell.
    Shell(String),
    /// A program and its arguments, with no shell.
    Argv(Vec<String>),
}

/// The kind of work a task does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Build,
    Test,
    Run,
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Build => "build",
            Self::Test => "test",
            Self::Run => "run",
        })
    }
}

/// Where a task came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The tasks file.
    File,
    Cargo,
    Npm,
    Gradle,
    Maven,
    Dotnet,
    Make,
    Just,
}

/// A problem with the tasks file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskDiagnostic {
    /// The entry it's about, by label or else its place in the file (`#2`);
    /// `None` for the file as a whole.
    pub entry: Option<String>,
    /// The whole problem in words, the entry's name included.
    pub message: String,
}

impl TaskDiagnostic {
    fn file(problem: impl fmt::Display) -> Self {
        Self {
            entry: None,
            message: tr!(
                "workbench-lsp-tasks-file-problem",
                file = TASKS_FILE,
                problem = problem.to_string()
            )
            .into(),
        }
    }

    fn entry(entry: &str, problem: impl fmt::Display) -> Self {
        Self {
            entry: Some(entry.to_owned()),
            message: tr!(
                "workbench-lsp-tasks-entry-problem",
                entry = entry,
                problem = problem.to_string()
            )
            .into(),
        }
    }
}

/// The project's tasks, in order, and what was wrong with its tasks file.
pub fn load(files: &dyn ProjectFiles, root: &Path) -> (Vec<Task>, Vec<TaskDiagnostic>) {
    let mut diagnostics = Vec::new();
    let mut tasks = file_tasks(files, root, &mut diagnostics);
    let labels: HashSet<String> = tasks.iter().map(|task| task.label.clone()).collect();
    tasks.extend(
        detected_tasks(files, root)
            .into_iter()
            .filter(|task| !labels.contains(&task.label)),
    );
    choose_defaults(&mut tasks, &mut diagnostics);
    (tasks, diagnostics)
}

/// The tasks file's usable entries: none if it isn't there.
fn file_tasks(
    files: &dyn ProjectFiles,
    root: &Path,
    diagnostics: &mut Vec<TaskDiagnostic>,
) -> Vec<Task> {
    let text = match files.read_to_string(&root.join(TASKS_FILE)) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            diagnostics.push(TaskDiagnostic::file(error));
            return Vec::new();
        }
    };
    let table = match toml::from_str::<toml::Table>(&text) {
        Ok(table) => table,
        Err(error) => {
            diagnostics.push(TaskDiagnostic::file(error));
            return Vec::new();
        }
    };
    let Some(entries) = table.get("task") else {
        return Vec::new();
    };
    let Some(entries) = entries.as_array() else {
        diagnostics.push(TaskDiagnostic::file(tr!(
            "workbench-lsp-tasks-not-an-array"
        )));
        return Vec::new();
    };

    let mut tasks = Vec::new();
    let mut labels = HashSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let name = entry_name(entry, index);
        for key in entry.as_table().into_iter().flat_map(toml::Table::keys) {
            if !ENTRY_KEYS.contains(&key.as_str()) {
                diagnostics.push(TaskDiagnostic::entry(
                    &name,
                    tr!("workbench-lsp-tasks-unknown-key", key = key),
                ));
            }
        }
        match parse_entry(entry, root) {
            Ok(task) if labels.insert(task.label.clone()) => tasks.push(task),
            Ok(_) => diagnostics.push(TaskDiagnostic::entry(
                &name,
                tr!("workbench-lsp-tasks-repeated-label"),
            )),
            Err(problem) => {
                let problem = tr!("workbench-lsp-tasks-left-out", problem = problem);
                diagnostics.push(TaskDiagnostic::entry(&name, problem));
            }
        }
    }
    tasks
}

/// What a diagnostic calls an entry: its label if it has one, else its place.
fn entry_name(entry: &toml::Value, index: usize) -> String {
    entry
        .get("label")
        .and_then(toml::Value::as_str)
        .filter(|label| !label.trim().is_empty())
        .map_or_else(|| format!("#{}", index + 1), str::to_owned)
}

fn parse_entry(entry: &toml::Value, root: &Path) -> Result<Task, String> {
    let entry =
        (entry.as_table()).ok_or_else(|| problem(tr!("workbench-lsp-tasks-not-a-table")))?;
    let label = match entry.get("label") {
        None => return Err(problem(tr!("workbench-lsp-tasks-no-label"))),
        Some(label) => {
            (label.as_str()).ok_or_else(|| problem(tr!("workbench-lsp-tasks-label-not-string")))?
        }
    };
    if label.trim().is_empty() {
        return Err(problem(tr!("workbench-lsp-tasks-empty-label")));
    }
    let cwd = match entry.get("cwd") {
        None => root.to_path_buf(),
        Some(cwd) => {
            let cwd =
                (cwd.as_str()).ok_or_else(|| problem(tr!("workbench-lsp-tasks-cwd-not-string")))?;
            resolve_cwd(root, cwd)
                .ok_or_else(|| problem(tr!("workbench-lsp-tasks-cwd-outside", cwd = cwd)))?
        }
    };
    Ok(Task {
        label: label.to_owned(),
        command: parse_command(entry.get("command"))?,
        cwd,
        env: parse_env(entry.get("env"))?,
        group: entry.get("group").map(parse_group).transpose()?,
        default: match entry.get("default") {
            None => false,
            Some(default) => (default.as_bool())
                .ok_or_else(|| problem(tr!("workbench-lsp-tasks-default-not-boolean")))?,
        },
        source: Source::File,
        problem_matcher: match entry.get("problem_matcher") {
            None => None,
            Some(matcher) => Some(
                matcher
                    .as_str()
                    .ok_or_else(|| problem(tr!("workbench-lsp-tasks-matcher-not-string")))?
                    .to_owned(),
            ),
        },
    })
}

/// A tasks file entry's problem, as `parse_entry` says it.
fn problem(text: gpui::SharedString) -> String {
    text.into()
}

fn parse_command(command: Option<&toml::Value>) -> Result<TaskCommand, String> {
    let empty = || problem(tr!("workbench-lsp-tasks-empty-command"));
    let wrong_type = || problem(tr!("workbench-lsp-tasks-command-wrong-type"));
    match command {
        None => Err(problem(tr!("workbench-lsp-tasks-no-command"))),
        Some(toml::Value::String(line)) if line.trim().is_empty() => Err(empty()),
        Some(toml::Value::String(line)) => Ok(TaskCommand::Shell(line.clone())),
        Some(toml::Value::Array(words)) => {
            let argv = words
                .iter()
                .map(|word| word.as_str().map(str::to_owned).ok_or_else(wrong_type))
                .collect::<Result<Vec<_>, _>>()?;
            match argv.first() {
                Some(program) if !program.trim().is_empty() => Ok(TaskCommand::Argv(argv)),
                _ => Err(empty()),
            }
        }
        Some(_) => Err(wrong_type()),
    }
}

fn parse_env(env: Option<&toml::Value>) -> Result<BTreeMap<String, String>, String> {
    let Some(env) = env else {
        return Ok(BTreeMap::new());
    };
    let env = (env.as_table()).ok_or_else(|| problem(tr!("workbench-lsp-tasks-env-not-table")))?;
    env.iter()
        .map(|(name, value)| {
            if name.is_empty() || name.contains('=') {
                return Err(problem(tr!(
                    "workbench-lsp-tasks-env-bad-name",
                    name = name
                )));
            }
            let value = value
                .as_str()
                .ok_or_else(|| problem(tr!("workbench-lsp-tasks-env-not-string", name = name)))?;
            Ok((name.clone(), value.to_owned()))
        })
        .collect()
}

fn parse_group(group: &toml::Value) -> Result<Group, String> {
    match group.as_str() {
        Some("build") => Ok(Group::Build),
        Some("test") => Ok(Group::Test),
        Some("run") => Ok(Group::Run),
        Some(other) => Err(problem(tr!(
            "workbench-lsp-tasks-unknown-group",
            group = other
        ))),
        None => Err(problem(tr!("workbench-lsp-tasks-group-not-string"))),
    }
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

/// Leaves at most one default in each group; see the module's docs.
fn choose_defaults(tasks: &mut [Task], diagnostics: &mut Vec<TaskDiagnostic>) {
    for task in tasks.iter_mut().filter(|task| task.group.is_none()) {
        if task.default {
            task.default = false;
            diagnostics.push(TaskDiagnostic::entry(
                &task.label,
                tr!("workbench-lsp-tasks-default-needs-group"),
            ));
        }
    }
    for group in [Group::Build, Group::Test, Group::Run] {
        let mut marked = None;
        for task in tasks
            .iter_mut()
            .filter(|task| task.default && task.group == Some(group))
        {
            if let Some(first) = &marked {
                task.default = false;
                diagnostics.push(TaskDiagnostic::entry(
                    &task.label,
                    tr!(
                        "workbench-lsp-tasks-default-taken",
                        first = first,
                        group = group.to_string()
                    ),
                ));
            } else {
                marked = Some(task.label.clone());
            }
        }
        if marked.is_none() && group != Group::Run {
            let by_name = tasks.iter_mut().find(|task| {
                task.group == Some(group)
                    && matches!(
                        task.source,
                        Source::Cargo
                            | Source::Npm
                            | Source::Gradle
                            | Source::Maven
                            | Source::Dotnet
                    )
            });
            if let Some(task) = by_name {
                task.default = true;
            }
        }
    }
}

fn detected_tasks(files: &dyn ProjectFiles, root: &Path) -> Vec<Task> {
    let mut tasks = cargo_tasks(files, root);
    tasks.extend(npm_tasks(files, root));
    tasks.extend(gradle_tasks(files, root));
    tasks.extend(maven_tasks(files, root));
    tasks.extend(dotnet_tasks(files, root));
    if let Some(text) = first_readable(files, root, &MAKEFILES) {
        tasks.extend(
            make_targets(&text)
                .into_iter()
                .map(|target| detected(Source::Make, None, &["make", &target], root)),
        );
    }
    if let Some(text) = first_readable(files, root, &JUSTFILES) {
        tasks.extend(
            just_recipes(&text)
                .into_iter()
                .map(|recipe| detected(Source::Just, None, &["just", &recipe], root)),
        );
    }
    tasks
}

/// A detected task, labelled with its command.
fn detected(source: Source, group: Option<Group>, argv: &[&str], root: &Path) -> Task {
    detected_as(argv.join(" "), source, group, argv, root)
}

/// A detected task with its own label. A last word starting with `-` is a
/// name, so the command puts `--` before it and no runner reads it as an option.
fn detected_as(
    label: String,
    source: Source,
    group: Option<Group>,
    argv: &[&str],
    root: &Path,
) -> Task {
    let mut words: Vec<String> = argv.iter().map(|&word| word.to_owned()).collect();
    if words.len() > 1 && words.last().is_some_and(|name| name.starts_with('-')) {
        words.insert(words.len() - 1, "--".to_owned());
    }
    Task {
        label,
        command: TaskCommand::Argv(words),
        cwd: root.to_path_buf(),
        env: BTreeMap::new(),
        group,
        default: false,
        source,
        problem_matcher: None,
    }
}

/// The text of the first of `names` under `root` that's there.
fn first_readable(files: &dyn ProjectFiles, root: &Path, names: &[&str]) -> Option<String> {
    names
        .iter()
        .map(|name| root.join(name))
        .find(|path| files.exists(path))
        .and_then(|path| files.read_to_string(&path).ok())
}

fn cargo_tasks(files: &dyn ProjectFiles, root: &Path) -> Vec<Task> {
    if !files.exists(&root.join(verbatim!("Cargo.toml"))) {
        return Vec::new();
    }
    [
        ("build", Some(Group::Build)),
        ("test", Some(Group::Test)),
        ("run", Some(Group::Run)),
        ("clippy", None),
    ]
    .into_iter()
    .map(|(command, group)| detected(Source::Cargo, group, &["cargo", command], root))
    .collect()
}

/// Gradle's fixed task list, by the wrapper when the root holds one.
fn gradle_tasks(files: &dyn ProjectFiles, root: &Path) -> Vec<Task> {
    let marked = [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ]
    .iter()
    .any(|name| files.exists(&root.join(name)));
    if !marked {
        return Vec::new();
    }
    let runner = if files.exists(&root.join("gradlew")) {
        "./gradlew"
    } else {
        "gradle"
    };
    [
        ("build", Some(Group::Build)),
        ("test", Some(Group::Test)),
        ("run", None),
        ("clean", None),
        ("check", None),
    ]
    .into_iter()
    .map(|(task, group)| detected(Source::Gradle, group, &[runner, task], root))
    .collect()
}

/// Maven's fixed goal list, by the wrapper when the root holds one.
fn maven_tasks(files: &dyn ProjectFiles, root: &Path) -> Vec<Task> {
    if !files.exists(&root.join("pom.xml")) {
        return Vec::new();
    }
    let runner = if files.exists(&root.join("mvnw")) {
        "./mvnw"
    } else {
        "mvn"
    };
    [
        ("compile", Some(Group::Build)),
        ("test", Some(Group::Test)),
        ("package", Some(Group::Build)),
        ("verify", None),
        ("clean", None),
    ]
    .into_iter()
    .map(|(goal, group)| detected(Source::Maven, group, &[runner, goal], root))
    .collect()
}

/// Folders a solution is never looked for in.
const DOTNET_SKIP: [&str; 4] = ["node_modules", "target", "bin", "obj"];

fn extension_in(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| extensions.iter().any(|want| ext.eq_ignore_ascii_case(want)))
}

/// The root-relative names of the files in `dir` with one of `extensions`,
/// sorted.
fn files_with(files: &dyn ProjectFiles, dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = files
        .list_dir(dir)
        .into_iter()
        .filter(|path| extension_in(path, extensions) && files.exists(path))
        .collect();
    found.sort();
    found
}

/// `dotnet` build, test, clean and restore per solution (root, else one
/// folder down), or per root project when there's no solution, with
/// `dotnet run` for those projects.
fn dotnet_tasks(files: &dyn ProjectFiles, root: &Path) -> Vec<Task> {
    const SOLUTIONS: [&str; 2] = ["sln", "slnx"];
    const PROJECTS: [&str; 3] = ["csproj", "fsproj", "vbproj"];
    let mut solutions = files_with(files, root, &SOLUTIONS);
    if solutions.is_empty() {
        let mut folders: Vec<PathBuf> = files
            .list_dir(root)
            .into_iter()
            .filter(|path| {
                !files.exists(path)
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| !name.starts_with('.') && !DOTNET_SKIP.contains(&name))
            })
            .collect();
        folders.sort();
        for folder in folders {
            solutions.extend(files_with(files, &folder, &SOLUTIONS));
        }
    }
    let (targets, runnable) = if solutions.is_empty() {
        (files_with(files, root, &PROJECTS), true)
    } else {
        (solutions, false)
    };
    let mut tasks = Vec::new();
    for target in targets {
        let rel = target.strip_prefix(root).unwrap_or(&target);
        let rel = rel
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        for (verb, group) in [
            ("build", Some(Group::Build)),
            ("test", Some(Group::Test)),
            ("clean", None),
            ("restore", None),
        ] {
            tasks.push(detected(
                Source::Dotnet,
                group,
                &["dotnet", verb, &rel],
                root,
            ));
        }
        if runnable {
            tasks.push(detected(
                Source::Dotnet,
                None,
                &["dotnet", "run", "--project", &rel],
                root,
            ));
        }
    }
    tasks
}

fn npm_tasks(files: &dyn ProjectFiles, root: &Path) -> Vec<Task> {
    let Ok(text) = files.read_to_string(&root.join("package.json")) else {
        return Vec::new();
    };
    // A derived struct would also read an array, by position.
    if !text.trim_start().starts_with('{') {
        return Vec::new();
    }
    let Ok(package) = serde_json::from_str::<PackageJson>(&text) else {
        return Vec::new();
    };
    let has = |lockfile: &str| files.exists(&root.join(lockfile));
    let runner: &[&str] = if has("pnpm-lock.yaml") {
        &["pnpm", "run"]
    } else if has("yarn.lock") {
        &["yarn"]
    } else if has("bun.lockb") || has("bun.lock") {
        &["bun", "run"]
    } else {
        &["npm", "run"]
    };
    package
        .scripts
        .0
        .iter()
        .map(|script| {
            let group = match script.as_str() {
                "build" => Some(Group::Build),
                "test" => Some(Group::Test),
                _ => None,
            };
            let argv: Vec<&str> = runner.iter().copied().chain([script.as_str()]).collect();
            let label = argv.join(" ");
            // `yarn -x` has no `run` to put `--` after.
            let mut argv = argv;
            if script.starts_with('-') && argv.len() == 2 && argv[0] == "yarn" {
                argv.insert(1, "run");
            }
            detected_as(label, Source::Npm, group, &argv, root)
        })
        .collect()
}

/// The part of a `package.json` detection reads.
#[derive(Deserialize)]
struct PackageJson {
    #[serde(default)]
    scripts: ScriptNames,
}

/// A `scripts` object's names, in the file's order, which a map from
/// `serde_json` wouldn't keep. A script that isn't a string can't run, so it
/// isn't one.
#[derive(Default)]
struct ScriptNames(Vec<String>);

impl<'de> Deserialize<'de> for ScriptNames {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NamesVisitor;

        impl<'de> Visitor<'de> for NamesVisitor {
            type Value = ScriptNames;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(verbatim!("an object of scripts"))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ScriptNames, A::Error> {
                let mut names = Vec::new();
                while let Some((name, script)) = map.next_entry::<String, serde_json::Value>()? {
                    if script.is_string() && !name.is_empty() {
                        push_new(&mut names, name);
                    }
                }
                Ok(ScriptNames(names))
            }
        }

        deserializer.deserialize_map(NamesVisitor)
    }
}

fn push_new(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

/// A makefile's explicit targets, in its order. Dot targets, pattern rules
/// and names only make can work out (`$(BIN)`, `*.o`) can't be asked for by
/// name, so they aren't listed. This is a scan of the lines, not make: a
/// changed `.RECIPEPREFIX` isn't known.
fn make_targets(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut defines = 0_usize;
    for line in logical_lines(text) {
        if line.starts_with('\t') {
            continue;
        }
        let line = strip_make_comment(&line);
        let Some(first) = line.split_whitespace().next() else {
            continue;
        };
        let mut words = line
            .split_whitespace()
            .skip_while(|word| matches!(*word, "export" | "override" | "private"));
        let keyword = words.next();
        // `define := value` assigns a variable that's called `define`.
        let assigns = words.next().is_some_and(is_assignment_operator);
        if keyword == Some("define") && !assigns {
            defines += 1;
        } else if first == "endef" {
            defines = defines.saturating_sub(1);
        } else if defines == 0 && !MAKE_DIRECTIVES.contains(&first) {
            let names = rule_head(line).map(make_words).unwrap_or_default();
            for name in names.into_iter().filter(|&name| nameable(name)) {
                push_new(&mut targets, name.to_owned());
            }
        }
    }
    targets
}

/// Whether a word is a make assignment operator: `=`, `:=`, `::=`, `:::=`,
/// `?=`, `+=` or `!=`, with or without a value stuck to it.
fn is_assignment_operator(word: &str) -> bool {
    let rest = word.trim_start_matches([':', '?', '+', '!']);
    word.len() - rest.len() <= 3 && rest.starts_with('=')
}

/// The text's lines with each `\`-continued run joined, as make reads them.
fn logical_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        let backslashes = line.len() - line.trim_end_matches('\\').len();
        if backslashes % 2 == 1 {
            pending.push_str(&line[..line.len() - 1]);
            pending.push(' ');
        } else {
            pending.push_str(line);
            lines.push(std::mem::take(&mut pending));
        }
    }
    if !pending.is_empty() {
        lines.push(pending);
    }
    lines
}

fn strip_make_comment(line: &str) -> &str {
    find_unnested(line, |c| c == '#').map_or(line, |comment| &line[..comment])
}

/// What's before a rule's colon, or `None` if the line isn't a rule: a
/// variable assignment, a target-specific one (`debug: CFLAGS = -g`), or
/// neither.
fn rule_head(line: &str) -> Option<&str> {
    let split = find_unnested(line, |c| c == ':' || c == '=')?;
    if line[split..].starts_with('=') {
        return None;
    }
    // `:=`, `::=` and `:::=` assign; `::` alone is a double-colon rule.
    let rest = line[split..].trim_start_matches(':');
    if rest.starts_with('=') {
        return None;
    }
    let prerequisites = rest.split(';').next().unwrap_or(rest);
    if find_unnested(prerequisites, |c| c == '=').is_some() {
        return None;
    }
    // A grouped rule's `&:` or `&::` isn't a target's, but `foo & :` has one
    // named `&`, and an escaped one is part of its name.
    let head = &line[..split];
    let grouped = head
        .strip_suffix('&')
        .filter(|before| (before.len() - before.trim_end_matches('\\').len()) % 2 == 0);
    Some(grouped.unwrap_or(head))
}

/// A rule head's whitespace-separated words, keeping as one word each
/// reference (`$(addprefix out/,one two)`), archive member list
/// (`lib.a(one.o two.o)`) and escaped space. Only a `(` after a name and closed
/// by a `)` that ends the word is archive syntax; any other, like a brace
/// outside a reference, is just part of a name.
fn make_words(head: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut rest = head.trim_start();
    while !rest.is_empty() {
        let mut end = 0;
        let mut first_paren = true;
        while let Some(at) = find_unnested(&rest[end..], |c| c.is_whitespace() || c == '(') {
            end += at;
            if !rest[end..].starts_with('(') {
                break;
            }
            // Only a word's first `(`, after a name, can open an archive
            // member list, and the list ends at the first `)` that ends a
            // word: an inner `)` is part of a member.
            let close = if first_paren && end > 0 {
                rest[end..]
                    .match_indices(')')
                    .map(|(at, _)| end + at + 1)
                    .find(|&close| rest[close..].chars().next().is_none_or(char::is_whitespace))
            } else {
                None
            };
            first_paren = false;
            end = close.unwrap_or(end + 1);
        }
        let word_end = if rest[end..].starts_with(char::is_whitespace) {
            end
        } else {
            rest.len()
        };
        words.push(&rest[..word_end]);
        rest = rest[word_end..].trim_start();
    }
    words
}

/// The first char outside any `$(...)` or `${...}` that `wanted` accepts,
/// passing over each `\`-escaped one.
fn find_unnested(text: &str, wanted: impl Fn(char) -> bool) -> Option<usize> {
    let mut depth = 0_usize;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '$' => {
                if matches!(chars.peek(), Some((_, '(' | '{'))) {
                    chars.next();
                    depth += 1;
                }
            }
            '(' | '{' if depth > 0 => depth += 1,
            ')' | '}' if depth > 0 => depth -= 1,
            _ if depth == 0 && wanted(c) => return Some(index),
            _ => {}
        }
    }
    None
}

/// Whether a rule's target is one `make <target>` can ask for.
///
/// Archive members (`lib.a(one.o)`) aren't: a word of a name, then `(`, then
/// nonempty members up to a final `)` (`a()` is a plain name). Literal parentheses (`(x)`, `a(b)c`) are part of
/// a plain name.
fn nameable(target: &str) -> bool {
    let archive_member = target.ends_with(')')
        && target
            .find('(')
            .is_some_and(|at| at > 0 && at + 1 < target.len() - 1);
    !target.starts_with('.') && !target.contains(['%', '$', '\\', '*', '?', '[']) && !archive_member
}

/// A justfile's public recipes, in its order. Recipe bodies are indented, so
/// only a line at the margin can start one, and each is read with its strings
/// and comment taken out, however many lines a string runs over.
fn just_recipes(text: &str) -> Vec<String> {
    let mut recipes = Vec::new();
    let mut private = false;
    let mut open_string = None;
    let mut continuing = false;
    let mut bare = String::new();
    for line in text.lines() {
        let rest = match open_string {
            Some(quote) => {
                let Some(end) = just_string_end(line, quote) else {
                    continue;
                };
                &line[end + quote.len()..]
            }
            None if !continuing && (line.is_empty() || line.starts_with(char::is_whitespace)) => {
                continue;
            }
            None => line,
        };
        open_string = scan_just_line(rest, &mut bare);
        if open_string.is_some() {
            continue;
        }
        // A header or attribute goes on over an escaped newline or while a
        // bracket is open, whatever the next line's indent.
        let escaped = bare.ends_with('\\');
        if escaped {
            bare.pop();
        }
        continuing = escaped || open_delimiters(&bare) > 0;
        if continuing {
            bare.push(' ');
            continue;
        }
        if bare.starts_with('[') {
            private |= just_attribute_is_private(&bare);
        } else if !bare.is_empty() {
            if let Some(name) = just_recipe_name(&bare)
                && !private
                && !name.starts_with('_')
            {
                push_new(&mut recipes, name.to_owned());
            }
            private = false;
        }
        bare.clear();
    }
    recipes
}

/// How many of a justfile header's `(`, `[` and `{` are still open.
fn open_delimiters(bare: &str) -> isize {
    bare.chars().fold(0, |depth, c| match c {
        '(' | '[' | '{' => depth + 1,
        ')' | ']' | '}' => (depth - 1).max(0),
        _ => depth,
    })
}

/// Whether an attribute line (`[private]`, `[no-cd, private]`), its strings
/// taken out, holds `private`.
fn just_attribute_is_private(bare: &str) -> bool {
    bare.split(['[', ']', ','])
        .any(|attribute| attribute.trim() == "private")
}

/// The name of the recipe a line starts, its strings taken out and its
/// parameters and dependencies aside, or `None` for anything else: `x := 1`,
/// `alias b := build`, `set shell := [...]`, `mod tools`, `import 'other.just'`.
fn just_recipe_name(bare: &str) -> Option<&str> {
    let header = bare.strip_prefix('@').unwrap_or(bare);
    let end = header
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(header.len());
    let (name, rest) = header.split_at(end);
    if name.is_empty() || !(rest.starts_with(':') || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    let colon = rest.find(':')?;
    (!rest[colon + 1..].starts_with('=')).then_some(name)
}

/// Adds to `bare` what a justfile line has outside its strings and its
/// comment, and returns the delimiter of a string it leaves open.
fn scan_just_line(line: &str, bare: &mut String) -> Option<&'static str> {
    let mut at = 0;
    while let Some(c) = line[at..].chars().next() {
        if c == '#' {
            return None;
        }
        let quote = JUST_QUOTES
            .into_iter()
            .find(|quote| line[at..].starts_with(quote));
        if let Some(quote) = quote {
            let body = at + quote.len();
            let Some(end) = just_string_end(&line[body..], quote) else {
                return Some(quote);
            };
            at = body + end + quote.len();
        } else {
            bare.push(c);
            at += c.len_utf8();
        }
    }
    None
}

/// Where `quote` closes a string in `text`: only `"` strings have escapes.
fn just_string_end(text: &str, quote: &str) -> Option<usize> {
    if !quote.starts_with('"') {
        return text.find(quote);
    }
    let mut chars = text.char_indices();
    while let Some((index, c)) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if text[index..].starts_with(quote) {
            return Some(index);
        }
    }
    None
}
