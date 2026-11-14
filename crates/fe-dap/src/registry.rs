// SPDX-License-Identifier: Apache-2.0

//! Which adapter a launch configuration gets
//! (docs/decisions/r1b9dw3g0g-debugger.md §3, §13; card D3). A
//! configuration names an adapter by name or by its VS Code `type`; the
//! [`Registry`] turns that into an [`AdapterSpec`] the launcher can run.
//!
//! **Layers**, as fe-lsp's `registry.rs` has them, highest first:
//!
//! 1. specs the host [registers](Registry::register): used as they are;
//! 2. the user's `[debug.adapters.<name>]` settings: over an extension's
//!    or a built-in spec of that name each key they set, or an adapter of
//!    the user's own with a `command`;
//! 3. specs extensions contribute (D15): one named like a built-in
//!    replaces it, as an installed codelldb (D14) does;
//! 4. the built-in lldb-dap, codelldb, debugpy and delve ([`builtin`]).
//!
//! Each adapter sits in the layer that won it: a built-in or an
//! extension's spec the user's settings change is the user's, so it comes
//! before every extension's and built-in spec. Within the user's layer
//! adapters go by name; within the others, in the order they were given.
//! A `type` is looked up, after names, and a language's default chosen,
//! in that one order.
//!
//! The settings reaching here are the effective ones, whose project layer
//! fe-config has narrowed (`narrow_debug`), so a folder can't choose what
//! runs.
//!
//! **Finding programs (§13).** A bare command is looked up on the login
//! shell's `PATH`, as language servers' are, through fe-lsp's
//! [`LoginPath`]: fe-dap depends on fe-lsp for it rather than taking a
//! resolver from the host, so the two can't drift. A program not found
//! leaves the adapter listed with [`Status::NotFound`] and a hint. Looking
//! programs up may run the login shell once (up to 2 s), and finishing a
//! spec may run `rustc` or `xcrun`, so resolve off the UI thread, and only
//! for a folder that is trusted (§8): a project's `.venv` Python and its
//! `rust-toolchain.toml` are the folder's own.

use std::collections::BTreeMap;
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use futures::AsyncReadExt as _;
use futures::future::Either;

use fe_config::debug_settings::{DebugAdapterSettings, DebugSettings, DebugTransport};
use fe_lsp::LoginPath;
use kiln_l10n::tr;

use crate::launcher::{AdapterSpec, AdapterTransport, is_runnable};

pub mod builtin;

pub use builtin::{
    HostOs, Lookup, XCRUN, lldb_dap_lookups, python_interpreter, rust_init_commands,
};

/// Which layer an adapter's spec came from: the highest that set any of
/// it. Ordered highest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Host,
    User,
    Extension,
    Builtin,
}

/// An adapter as the registry resolved it.
#[derive(Clone, Debug, PartialEq)]
pub struct Adapter {
    pub source: Source,
    /// Ready for the launcher: when found, its `command` is the program
    /// found, and an LLDB adapter debugging Rust has the toolchain's
    /// formatters at the end of its `init_commands`. When not found, its
    /// `command` is what was looked for.
    pub spec: AdapterSpec,
    pub status: Status,
}

/// Whether an adapter's program was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Found,
    /// Not on the `PATH` it was looked up on, or not a program; `hint` says
    /// how to get it, in the active locale.
    NotFound {
        hint: String,
    },
}

/// What a resolution is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    /// The folder the debuggee runs in: relative commands are under it, and
    /// `rustc` and `xcrun` run in it.
    pub root: PathBuf,
    /// The debuggee's language, by fe-syntax's names, if known: Rust's gets
    /// the formatters under lldb-dap.
    pub language: Option<String>,
    /// An active virtual environment, `$VIRTUAL_ENV`.
    pub virtual_env: Option<PathBuf>,
}

impl Context {
    /// For `root`, with this process's `$VIRTUAL_ENV` if it's absolute.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            language: None,
            virtual_env: std::env::var_os("VIRTUAL_ENV")
                .map(PathBuf::from)
                .filter(|env| env.is_absolute()),
        }
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }
}

/// Runs a short command for what it prints: `rustc --print sysroot`, and
/// `xcrun -f lldb-dap` on macOS. A seam so tests can fake them.
pub trait Runner: Send + Sync {
    /// What `program args` prints on stdout, run in `cwd`, if it ran and
    /// exited successfully within its time.
    fn stdout(&self, program: &Path, args: &[&str], cwd: &Path) -> Option<String>;
}

/// How long [`SystemRunner`] lets a command run by default: a rustup
/// proxy may be slow on its first run.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(5);

/// The default [`Runner`]: a real process in a process group of its own
/// (on Unix), with no stdin and no stderr. Its exit and the end of its
/// stdout are both awaited within one deadline, [`RUN_TIMEOUT`] by
/// default: a program that leaves a descendant holding its stdout counts
/// as having run out of time. Then, or on any failure, its whole group is
/// killed and its stdout closed, so nothing it started outlives the call.
#[derive(Clone, Copy, Debug)]
pub struct SystemRunner {
    timeout: Duration,
}

impl Default for SystemRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemRunner {
    pub fn new() -> Self {
        Self {
            timeout: RUN_TIMEOUT,
        }
    }

    /// With `timeout` as its deadline.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Runner for SystemRunner {
    fn stdout(&self, program: &Path, args: &[&str], cwd: &Path) -> Option<String> {
        let mut command = std::process::Command::new(program);
        command.args(args).current_dir(cwd);
        #[cfg(unix)]
        command.process_group(0);
        let mut command = async_process::Command::from(command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .inspect_err(|err| log::debug!("running {}: {err}", program.display()))
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let outcome = async_io::block_on(async {
            let run = async {
                let mut out = Vec::new();
                let read = stdout.read_to_end(&mut out).await;
                let status = child.status().await;
                (read, status, out)
            };
            let deadline = async_io::Timer::after(self.timeout);
            match futures::future::select(pin!(run), deadline).await {
                Either::Left((outcome, _)) => Some(outcome),
                Either::Right(_) => None,
            }
        });
        let out = match outcome {
            Some((Ok(_), Ok(status), out)) if status.success() => Some(out),
            Some((read, status, _)) => {
                log::debug!("{} failed: {read:?}, {status:?}", program.display());
                None
            }
            None => {
                log::warn!(
                    "{} took over {} ms, so it was stopped",
                    program.display(),
                    self.timeout.as_millis()
                );
                None
            }
        };
        if out.is_none() {
            kill_group(&mut child);
        }
        // Dropping `stdout` closes it; `kill_on_drop` and async-process's
        // reaper see to the child.
        drop(stdout);
        String::from_utf8(out?).ok()
    }
}

/// Kills `child` and, on Unix, the process group it leads.
fn kill_group(child: &mut async_process::Child) {
    #[cfg(unix)]
    if let Some(group) = i32::try_from(child.id())
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        && let Err(err) = rustix::process::kill_process_group(group, rustix::process::Signal::KILL)
        && err != rustix::io::Errno::SRCH
    {
        log::warn!("killing process group {}: {err}", child.id());
    }
    let _ = child.kill();
}

/// One adapter before resolution: its spec, and how to finish it.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    source: Source,
    spec: AdapterSpec,
    program: Program,
    /// The user's `path`: folders a bare command is looked up in first.
    search: Vec<String>,
    /// Whether Rust's LLDB formatters go in its `init_commands`.
    rust_formatters: bool,
    hint: Hint,
}

/// How an adapter's program is found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Program {
    /// Its `command`, as given.
    Command,
    /// The project's Python interpreter ([`python_interpreter`]).
    Python,
    /// `lldb-dap`, as [`lldb_dap_lookups`] says for the OS.
    LldbDap,
}

/// What a "not found" status suggests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hint {
    LldbDap,
    Codelldb,
    Debugpy,
    Delve,
    /// Any other adapter's, or a built-in's whose command the user set.
    Command,
}

impl Hint {
    /// The hint for `spec` not found, in the active locale.
    pub(crate) fn text(self, spec: &AdapterSpec) -> String {
        match self {
            Hint::LldbDap => tr!("dap-hint-lldb-dap"),
            Hint::Codelldb => tr!("dap-hint-codelldb"),
            Hint::Debugpy => tr!("dap-hint-debugpy"),
            Hint::Delve => tr!("dap-hint-delve"),
            Hint::Command => tr!(
                "dap-hint-not-found",
                name = spec.name.as_str(),
                command = spec.command.display().to_string()
            ),
        }
        .to_string()
    }
}

/// The adapters a launch configuration may name, in their layers.
pub struct Registry {
    host: Vec<AdapterSpec>,
    user: BTreeMap<String, DebugAdapterSettings>,
    extension: Vec<AdapterSpec>,
    login: Arc<LoginPath>,
    runner: Arc<dyn Runner>,
    os: HostOs,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("host", &self.host)
            .field("user", &self.user)
            .field("extension", &self.extension)
            .field("os", &self.os)
            .finish_non_exhaustive()
    }
}

impl Registry {
    /// The built-ins alone, looking bare commands up on `login`'s `PATH`
    /// (the host passes `LoginPath::system()`'s, as fe-lsp's launcher
    /// does), running commands for real, for this OS.
    pub fn new(login: Arc<LoginPath>) -> Self {
        Self {
            host: Vec::new(),
            user: BTreeMap::new(),
            extension: Vec::new(),
            login,
            runner: Arc::new(SystemRunner::new()),
            os: HostOs::current(),
        }
    }

    /// Running `rustc` and `xcrun` through `runner`.
    pub fn with_runner(mut self, runner: Arc<dyn Runner>) -> Self {
        self.runner = runner;
        self
    }

    /// Finding adapters as on `os`.
    pub fn with_os(mut self, os: HostOs) -> Self {
        self.os = os;
        self
    }

    /// Adds `spec` to the host's layer, replacing one of the same name: it
    /// wins over every other spec of that name, as it is.
    pub fn register(&mut self, spec: AdapterSpec) {
        match self.host.iter_mut().find(|host| host.name == spec.name) {
            Some(host) => *host = spec,
            None => self.host.push(spec),
        }
    }

    /// Takes the user's `[debug.adapters]` from the effective settings.
    pub fn set_settings(&mut self, settings: &DebugSettings) {
        for (name, adapter) in &settings.adapters {
            let known = builtin::entries()
                .iter()
                .any(|entry| entry.spec.name == *name)
                || self.extension.iter().any(|spec| spec.name == *name)
                || self.host.iter().any(|spec| spec.name == *name);
            if adapter.command.is_none() && !known {
                log::warn!("debug adapter {name} has no command, so it can't start");
            }
        }
        self.user = settings.adapters.clone();
    }

    /// Replaces the extensions' specs (D15), in order. A later one named
    /// like an earlier one, or like a host spec, is ignored.
    pub fn set_extension_specs(&mut self, specs: Vec<AdapterSpec>) {
        self.extension = specs;
    }

    /// Every adapter's name, in the order of the layers that won them:
    /// the host's, the user's (by name), the extensions', then the
    /// built-ins.
    pub fn names(&self) -> Vec<String> {
        self.entries()
            .into_iter()
            .map(|entry| entry.spec.name)
            .collect()
    }

    /// The adapter a configuration names, by name, else by VS Code `type`
    /// (the first in [`names`](Self::names)' order answering to it), and
    /// resolved for `context`.
    pub fn find(&self, name_or_type: &str, context: &Context) -> Option<Adapter> {
        let entries = self.entries();
        let index = entries
            .iter()
            .position(|entry| entry.spec.name == name_or_type)
            .or_else(|| {
                entries
                    .iter()
                    .position(|entry| entry.spec.types.iter().any(|ty| ty == name_or_type))
            })?;
        let entry = entries.into_iter().nth(index)?;
        Some(self.resolve(entry, context))
    }

    /// The adapter for debugging `language` with no adapter named: the
    /// first listing it, in [`names`](Self::names)' order, whose program
    /// is found, else the first listing it, not found. Among the built-ins,
    /// for Rust, C and C++ that is lldb-dap when found, else codelldb (§13
    /// Q3); one the user's settings change comes before them.
    pub fn default_for(&self, language: &str, context: &Context) -> Option<Adapter> {
        let mut first = None;
        for entry in self.entries() {
            if !entry.spec.languages.iter().any(|l| l == language) {
                continue;
            }
            let adapter = self.resolve(entry, context);
            if adapter.status == Status::Found {
                return Some(adapter);
            }
            first.get_or_insert(adapter);
        }
        first
    }

    /// Every adapter, resolved for `context`, in [`names`](Self::names)'
    /// order: those not found too, with their hints.
    pub fn list(&self, context: &Context) -> Vec<Adapter> {
        self.entries()
            .into_iter()
            .map(|entry| self.resolve(entry, context))
            .collect()
    }

    /// The effective adapters, each name once, in the layers' order.
    fn entries(&self) -> Vec<Entry> {
        let taken = |entries: &[Entry], name: &str| entries.iter().any(|e| e.spec.name == name);
        let mut entries: Vec<Entry> = Vec::new();
        for spec in &self.host {
            entries.push(Entry::from_spec(Source::Host, spec.clone()));
        }
        // One named like a built-in replaces it.
        for spec in &self.extension {
            if taken(&entries, &spec.name) {
                log::debug!(
                    "an extension's debug adapter {} is ignored: another has the name",
                    spec.name
                );
                continue;
            }
            entries.push(Entry::from_spec(Source::Extension, spec.clone()));
        }
        for entry in builtin::entries() {
            if !taken(&entries, &entry.spec.name) {
                entries.push(entry);
            }
        }
        // The user's settings over every spec but the host's, which lifts it
        // to their layer; then the user's own adapters.
        for entry in &mut entries {
            if entry.source != Source::Host
                && let Some(settings) = self.user.get(&entry.spec.name)
            {
                *entry = entry.clone().with_settings(Some(settings));
            }
        }
        for (name, settings) in &self.user {
            if !taken(&entries, name)
                && let Some(entry) = own(name, settings)
            {
                entries.push(entry);
            }
        }
        // Each by the layer that won it: the user's by name, the others in
        // the order they came in.
        entries.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| match (a.source, b.source) {
                    (Source::User, Source::User) => a.spec.name.cmp(&b.spec.name),
                    _ => std::cmp::Ordering::Equal,
                })
        });
        entries
    }

    /// `entry` finished for `context`: its program found, or not.
    fn resolve(&self, entry: Entry, context: &Context) -> Adapter {
        let Entry {
            source,
            mut spec,
            program,
            search,
            rust_formatters,
            hint,
        } = entry;
        // The host started it; there's nothing to find.
        if let AdapterTransport::Port(_) = spec.transport {
            return Adapter {
                source,
                spec,
                status: Status::Found,
            };
        }
        let found = match program {
            Program::Command => self.lookup(&spec, &search, &context.root),
            Program::Python => {
                spec.command =
                    python_interpreter(&context.root, context.virtual_env.as_deref(), is_runnable);
                self.lookup(&spec, &search, &context.root)
            }
            Program::LldbDap => {
                lldb_dap_lookups(self.os)
                    .into_iter()
                    .find_map(|lookup| match lookup {
                        Lookup::OnPath => self.lookup(&spec, &search, &context.root),
                        Lookup::Run { program, args } => self
                            .runner
                            .stdout(Path::new(program), args, &context.root)
                            .map(|out| PathBuf::from(out.trim_end_matches(['\n', '\r'])))
                            .filter(|found| found.is_absolute() && is_runnable(found)),
                    })
            }
        };
        let Some(found) = found else {
            let hint = hint.text(&spec);
            return Adapter {
                source,
                spec,
                status: Status::NotFound { hint },
            };
        };
        spec.command = found;
        if rust_formatters && context.language.as_deref() == Some("rust") {
            spec.init_commands.extend(self.rust_init_commands(context));
        }
        Adapter {
            source,
            spec,
            status: Status::Found,
        }
    }

    /// The program `spec.command` runs, in `root`: a bare name looked up in
    /// `search`, then on the `PATH` its `env` sets, else the login
    /// shell's, and only a file that may be run.
    fn lookup(&self, spec: &AdapterSpec, search: &[String], root: &Path) -> Option<PathBuf> {
        let env_path = spec
            .env
            .iter()
            .rev()
            .find(|(name, _)| name == "PATH")
            .map(|(_, path)| OsString::from(path));
        let path = if search.is_empty() {
            env_path
        } else {
            let rest = env_path
                .or_else(|| self.login.path().map(ToOwned::to_owned))
                .or_else(|| std::env::var_os("PATH"))
                .unwrap_or_default();
            let dirs = search
                .iter()
                .map(PathBuf::from)
                .chain(std::env::split_paths(&rest).collect::<Vec<_>>());
            match std::env::join_paths(dirs) {
                Ok(path) => Some(path),
                Err(err) => {
                    log::warn!("debug adapter {}'s path is ignored: {err}", spec.name);
                    Some(rest)
                }
            }
        };
        self.login
            .resolve(&spec.command, path.as_deref(), root)
            .filter(|program| is_runnable(program))
    }

    /// The Rust toolchain's LLDB formatters, from `rustc --print sysroot`
    /// run in the root with the `rustc` on the login shell's `PATH`; none
    /// without one.
    fn rust_init_commands(&self, context: &Context) -> Vec<String> {
        let Some(rustc) = self
            .login
            .resolve(Path::new("rustc"), None, &context.root)
            .filter(|rustc| is_runnable(rustc))
        else {
            log::debug!("no rustc on the login shell's PATH, so no Rust formatters for LLDB");
            return Vec::new();
        };
        self.runner
            .stdout(&rustc, &["--print", "sysroot"], &context.root)
            .map(|sysroot| rust_init_commands(&sysroot))
            .unwrap_or_default()
    }
}

impl Entry {
    /// A host's or an extension's spec, used as it is.
    fn from_spec(source: Source, spec: AdapterSpec) -> Self {
        Self {
            source,
            spec,
            program: Program::Command,
            search: Vec::new(),
            rust_formatters: false,
            hint: Hint::Command,
        }
    }

    /// With each key `settings` set over it: `command`, `args`, `path`,
    /// `transport`, `languages` and `types` replace its own, and `env` is
    /// set on top of its own.
    fn with_settings(mut self, settings: Option<&DebugAdapterSettings>) -> Self {
        let Some(settings) = settings else {
            return self;
        };
        self.source = Source::User;
        if let Some(command) = &settings.command {
            self.spec.command = command.into();
            self.program = Program::Command;
            self.hint = Hint::Command;
        }
        if let Some(args) = &settings.args {
            self.spec.args.clone_from(args);
        }
        for (name, value) in settings.env.iter().flatten() {
            self.spec.env.retain(|(own, _)| own != name);
            self.spec.env.push((name.clone(), value.clone()));
        }
        if let Some(path) = &settings.path {
            self.search.clone_from(path);
        }
        if let Some(transport) = settings.transport {
            self.spec.transport = match transport {
                DebugTransport::Stdio => AdapterTransport::Stdio,
                DebugTransport::Tcp => AdapterTransport::Tcp,
            };
        }
        if let Some(languages) = &settings.languages {
            self.spec.languages.clone_from(languages);
        }
        if let Some(types) = &settings.types {
            self.spec.types.clone_from(types);
        }
        self
    }
}

/// An adapter of the user's own: `None` without a `command`.
fn own(name: &str, settings: &DebugAdapterSettings) -> Option<Entry> {
    let command = settings.command.as_deref()?;
    let entry = Entry::from_spec(Source::User, AdapterSpec::new(name, command));
    Some(entry.with_settings(Some(settings)))
}
