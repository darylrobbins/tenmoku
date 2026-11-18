//! The Python environment a Python language server runs in: the
//! project's virtualenv, found at the server's root, so a server installed
//! into it (`pip install pyright` puts `pyright-langserver` in
//! `.venv/bin`) is found, and the server sees the project's packages.
//!
//! Any spec whose `languages` hold `python` gets one ([`serves_python`]):
//! pyright, or a server of the user's such as ty or pyrefly. The first of
//! these with an interpreter (`bin/python`, `Scripts\python.exe` on
//! Windows) is the environment ([`find_python_env`]):
//!
//! 1. the `lsp.python_environment` setting ([`ServerSpec::python_environment`]):
//!    an absolute path, one under `~`, or one relative to the server's root;
//! 2. `$VIRTUAL_ENV`, the spec's own or Tenmoku's, if it's absolute;
//! 3. `<root>/.venv`;
//! 4. `<root>/venv`.
//!
//! With none, the server is looked up on the `PATH` as any other is
//! ([`LoginPath::resolve`](crate::LoginPath::resolve)). With one,
//! [`LocalLauncher`](crate::LocalLauncher) looks a bare command up in the
//! environment's bin before the `PATH`, and the server runs with that bin
//! first on its `PATH` and `VIRTUAL_ENV` set to the environment, as an
//! activated shell would have it. A spec's
//! [`interpreter_setting`](ServerSpec::interpreter_setting) names where its
//! `workspace/configuration` answers have the interpreter.
//!
//! Running a project's `.venv` is no more than running its servers at all:
//! they start only in trusted roots ([`LspManager::set_trust`](crate::LspManager::set_trust)).
//! Only the environment's own absolute bin is added to the `PATH`, never a
//! relative entry.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};

use kiln_l10n::tr;
use serde_json::Value;

use crate::event::ServerId;
use crate::launcher::{ServerSpec, home_dir, is_bare};
use crate::manager::LspManager;

/// The language whose servers get a Python environment.
const PYTHON: &str = "python";

/// The folders under a project's root an environment is looked for in,
/// in order.
const PROJECT_ENVS: [&str; 2] = [".venv", "venv"];

/// The folder of an environment's programs.
#[cfg(not(windows))]
const BIN: &str = "bin";
#[cfg(windows)]
const BIN: &str = kiln_l10n::verbatim!("Scripts");

/// An environment's interpreter, in its bin.
#[cfg(not(windows))]
const PYTHON_PROGRAM: &str = "python";
#[cfg(windows)]
const PYTHON_PROGRAM: &str = "python.exe";

/// A Python environment (a virtualenv, or any prefix with `bin/python`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonEnv {
    dir: PathBuf,
}

impl PythonEnv {
    /// The environment's folder, `VIRTUAL_ENV` to its programs.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The folder of its programs: `bin`, or `Scripts` on Windows.
    pub fn bin(&self) -> PathBuf {
        self.dir.join(BIN)
    }

    /// Its interpreter: `bin/python`, or `Scripts\python.exe` on Windows.
    pub fn python(&self) -> PathBuf {
        self.bin().join(PYTHON_PROGRAM)
    }

    /// What the status item calls it: its folder's name, such as `.venv`.
    pub fn name(&self) -> String {
        self.dir.file_name().map_or_else(
            || self.dir.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    /// Whether it has an interpreter: a file at [`python`](Self::python),
    /// through any symlink (a virtualenv's usually is one).
    fn has_python(&self) -> bool {
        std::fs::metadata(self.python()).is_ok_and(|meta| meta.is_file())
    }
}

/// Which Python environment a launch is in ([`ServerSpec::python_env_choice`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PythonEnvChoice {
    /// The launcher finds one ([`python_env`]).
    #[default]
    Find,
    /// fe-lsp found this one, or none, for the launch: the launcher looks
    /// for no other.
    Chosen(Option<PythonEnv>),
}

/// Whether `spec`'s server is one of Python's, which runs in a Python
/// environment: its `languages` hold `python`.
pub fn serves_python(spec: &ServerSpec) -> bool {
    spec.languages.iter().any(|language| language == PYTHON)
}

/// The Python environment of a server at `root`, the first of these with
/// an interpreter: `setting` (absolute, under `~` as `home`, or relative
/// to `root`), `virtual_env` if it's absolute, `<root>/.venv` and
/// `<root>/venv`. `None` with none, or with an empty `setting` and nothing
/// else.
pub fn find_python_env(
    setting: Option<&Path>,
    virtual_env: Option<&OsStr>,
    root: &Path,
    home: Option<&Path>,
) -> Option<PythonEnv> {
    let setting = setting
        .filter(|setting| !setting.as_os_str().is_empty())
        .and_then(|setting| setting_dir(setting, root, home));
    let virtual_env = virtual_env
        .map(PathBuf::from)
        .filter(|env| env.is_absolute());
    setting
        .into_iter()
        .chain(virtual_env)
        .chain(PROJECT_ENVS.iter().map(|name| root.join(name)))
        .map(|dir| PythonEnv { dir })
        .find(PythonEnv::has_python)
}

/// The folder `setting` names: under `home` for a leading `~` (`None` with
/// no home), else under `root` unless it's absolute.
fn setting_dir(setting: &Path, root: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let mut components = setting.components();
    match components.next() {
        Some(Component::Normal(first)) if first == "~" => Some(home?.join(components.as_path())),
        _ => Some(root.join(setting)),
    }
}

/// The Python environment of `spec`'s server at `root`: the one its
/// [`python_env_choice`](ServerSpec::python_env_choice) has chosen, else,
/// if it serves Python ([`serves_python`]), [`find_python_env`] with the
/// spec's `python_environment`, the `VIRTUAL_ENV` its `env` sets, else
/// Tenmoku's own, and the home directory.
pub fn python_env(spec: &ServerSpec, root: &Path) -> Option<PythonEnv> {
    if let PythonEnvChoice::Chosen(env) = &spec.python_env_choice {
        return env.clone();
    }
    if !serves_python(spec) {
        return None;
    }
    let virtual_env = spec
        .env
        .iter()
        .rev()
        .find(|(name, _)| name == "VIRTUAL_ENV")
        .map(|(_, value)| OsString::from(value))
        .or_else(|| std::env::var_os("VIRTUAL_ENV"));
    find_python_env(
        spec.python_environment.as_deref(),
        virtual_env.as_deref(),
        root,
        home_dir().as_deref(),
    )
}

impl LspManager {
    /// The Python environment `server` was launched in, if it's a server of
    /// Python's with one: the one its session runs with, found just before
    /// each launch and handed to the launcher, so it changes on a restart.
    /// What a host shows, rather than looking again: an environment made or
    /// removed since doesn't count until the server launches again.
    pub fn python_env(&self, server: ServerId) -> Option<PythonEnv> {
        self.session(server, |session| session.python_env.clone())
    }
}

/// `settings` with `env`'s interpreter at the dotted key `key` (pyright's
/// `python.pythonPath`), the objects on the way made as needed. Unchanged
/// with no key or no environment, where a value is set already (the
/// user's), where a value on the way isn't an object, or for an
/// interpreter path that isn't UTF-8.
pub(crate) fn with_interpreter(
    mut settings: Value,
    key: Option<&str>,
    env: Option<&PythonEnv>,
) -> Value {
    let (Some(key), Some(env)) = (key, env) else {
        return settings;
    };
    let Some(python) = env.python().to_str().map(str::to_owned) else {
        return settings;
    };
    if settings.is_null() {
        settings = Value::Object(serde_json::Map::new());
    }
    let mut parts = key.split('.').peekable();
    let mut at = &mut settings;
    while let Some(part) = parts.next() {
        let Value::Object(object) = at else {
            break;
        };
        if parts.peek().is_none() {
            object
                .entry(part)
                .or_insert_with(|| Value::String(python.clone()));
            break;
        }
        at = object
            .entry(part)
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
    }
    settings
}

/// [`LocalLauncher`](crate::LocalLauncher)'s error for a server of
/// Python's whose bare command it didn't find: the command and where it
/// looked, `env`'s bin (the environment at `root`) and the `PATH`, or, with
/// no environment, the `PATH` (none being at `root`). Its message is the
/// status the server stops with ([`searched`]). `None` for any other
/// server, or a command that isn't bare.
pub(crate) fn not_found(
    spec: &ServerSpec,
    root: &Path,
    env: Option<&PythonEnv>,
) -> Option<io::Error> {
    if !serves_python(spec) || !is_bare(&spec.command) {
        return None;
    }
    let command = spec.command.display().to_string();
    let text = match env {
        Some(env) => tr!(
            "lsp-server-not-found-in-env",
            command = command,
            bin = env.bin().display().to_string()
        ),
        None => tr!(
            "lsp-server-not-found-no-env",
            command = command,
            root = root.display().to_string()
        ),
    };
    Some(io::Error::new(
        io::ErrorKind::NotFound,
        Searched(text.to_string()),
    ))
}

/// Where a launch looked for a server it didn't find, if `err` says: one
/// of [`not_found`]'s. A host's launcher, whose search fe-lsp doesn't
/// know, gives none.
pub(crate) fn searched(err: &io::Error) -> Option<String> {
    let searched = err.get_ref()?.downcast_ref::<Searched>()?;
    Some(searched.0.clone())
}

/// [`not_found`]'s error: the command and where it was looked for.
#[derive(Debug)]
struct Searched(String);

impl std::fmt::Display for Searched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Searched {}

#[cfg(test)]
#[path = "python_env_tests.rs"]
mod tests;
