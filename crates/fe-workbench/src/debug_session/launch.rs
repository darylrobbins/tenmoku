//! Turning a launch configuration into what a session starts from
//! (docs/decisions/r1b9dw3g0g-debugger.md §4 "Building the launch
//! arguments"), cargo's artifact message read for a detected Cargo
//! configuration's program (§7), and the failures a toast explains in
//! words of its own (§10): ptrace on Linux, signing on macOS.
//!
//! Nothing here touches the workbench: [`session_options`] runs off the
//! main thread, as it may look adapters up on the login shell's `PATH` and
//! run `rustc`.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use fe_config::debug_settings::DebugSettings;
use fe_dap::{
    AdapterSpec, Context as AdapterContext, EnvExpansion, HostOs, LogMode, Registry,
    SessionOptions, StartRequest, Status, protocol::SourceBreakpoint,
};
use fe_lsp::LoginPath;
use kiln_l10n::{tr, verbatim};
use serde_json::{Map, Value};

use crate::launch_defs::{
    AdapterRef, Console, Cwd, LaunchConfig, Program,
    variables::{self, Values},
};

/// The login shell bare adapter commands are looked up on, as language
/// servers' are: `$SHELL`, else `/bin/sh` (none on Windows).
fn login_path() -> Arc<LoginPath> {
    static LOGIN: OnceLock<Arc<LoginPath>> = OnceLock::new();
    LOGIN
        .get_or_init(|| {
            let shell = if cfg!(windows) {
                PathBuf::new()
            } else {
                std::env::var_os(verbatim!("SHELL"))
                    .map(PathBuf::from)
                    .filter(|shell| shell.is_absolute())
                    .unwrap_or_else(|| PathBuf::from(verbatim!("/bin/sh")))
            };
            Arc::new(LoginPath::new(shell))
        })
        .clone()
}

/// The adapters a configuration may name: the host's `specs`, the user's
/// `[debug.adapters]` and the built-ins, bare commands looked up on
/// `login`'s `PATH` (by default the login shell's).
pub(crate) fn registry(
    specs: &[AdapterSpec],
    settings: &DebugSettings,
    login: Option<Arc<LoginPath>>,
) -> Registry {
    let mut registry = Registry::new(login.unwrap_or_else(login_path));
    registry.set_settings(settings);
    for spec in specs {
        registry.register(spec.clone());
    }
    registry
}

/// What a configuration is started with, besides itself.
pub(crate) struct Resolve {
    pub config: LaunchConfig,
    /// The canonical root.
    pub root: PathBuf,
    pub values: Values,
    /// The program a pre-launch cargo build made, for
    /// [`Program::CargoArtifact`].
    pub program: Option<PathBuf>,
    pub specs: Vec<AdapterSpec>,
    /// Where bare commands are looked up, if not on the login shell's `PATH`.
    pub login: Option<Arc<LoginPath>>,
    pub settings: DebugSettings,
    /// By source path: each file's enabled breakpoints.
    pub breakpoints: BTreeMap<String, Vec<SourceBreakpoint>>,
}

/// The session `resolve` starts: its adapter found, its variables expanded
/// and its launch arguments built (the adapter's defaults, the typed fields
/// by the adapter's own keys, then `adapter_args` as written).
///
/// # Errors
///
/// In words, for a toast: no adapter, an adapter not found, or a variable
/// that can't expand.
pub(crate) fn session_options(resolve: Resolve) -> Result<SessionOptions, String> {
    let Resolve {
        config,
        root,
        values,
        program,
        specs,
        login,
        settings,
        breakpoints,
    } = resolve;
    let expand = |text: &str| variables::expand(text, &values);
    let cwd = match &config.cwd {
        Cwd::At(cwd) => cwd.clone(),
        // Checked now, as only a start can expand it: it can't leave the
        // root.
        Cwd::Expand(written) => {
            under_root(&root, &root.join(expand(written)?)).ok_or_else(|| {
                tr!("workbench-debug-launch-cwd-outside", cwd = written.as_str()).to_string()
            })?
        }
    };
    let registry = registry(&specs, &settings, login);
    // Project-specific adapter resources (a root `.venv`, the toolchain
    // `rustc` runs in) are the workspace's, wherever the program runs.
    let context = AdapterContext::new(&root);
    let adapter = match &config.adapter {
        AdapterRef::Named {
            name: Some(name), ..
        } => {
            let context = match debuggee_language(&config, &root) {
                Some(language) => context.with_language(language),
                None => context,
            };
            registry.find(name, &context)
        }
        AdapterRef::Named {
            written,
            name: None,
        } => {
            return Err(tr!(
                "workbench-debug-launch-no-adapter-named",
                adapter = written.as_str()
            )
            .into());
        }
        AdapterRef::DefaultFor(language) => {
            registry.default_for(language, &context.with_language(language.as_str()))
        }
    };
    let Some(adapter) = adapter else {
        let language = match &config.adapter {
            AdapterRef::DefaultFor(language) => language.clone(),
            AdapterRef::Named { written, .. } => written.clone(),
        };
        return Err(tr!("workbench-debug-no-adapter-for", language = language).into());
    };
    if let Status::NotFound { hint } = &adapter.status {
        return Err(tr!(
            "workbench-debug-adapter-not-found",
            adapter = adapter.spec.name.as_str(),
            hint = hint.as_str()
        )
        .into());
    }
    let spec = adapter.spec;

    let program = match (&config.program, program) {
        (_, Some(built)) => Some(built.to_string_lossy().into_owned()),
        (Some(Program::Written(text)), None) => Some(expand(text)?),
        (Some(Program::CargoArtifact { bin }), None) => {
            return Err(tr!("workbench-debug-cargo-no-artifact", bin = bin.as_str()).into());
        }
        (None, None) => None,
    };
    let args = (config.args.iter())
        .map(|arg| expand(arg))
        .collect::<Result<Vec<_>, _>>()?;
    let mut env = Map::new();
    for (name, value) in &config.env {
        env.insert(name.clone(), Value::String(expand(value)?));
    }
    let mut adapter_args = Value::Object(config.adapter_args.clone());
    expand_strings(&mut adapter_args, &values)?;

    let mut arguments = spec.launch_defaults.clone();
    arguments.insert("name".into(), Value::String(config.name.clone()));
    if let Some(program) = program {
        arguments.insert("program".into(), Value::String(program));
    }
    let args = args.into_iter().map(Value::String).collect();
    arguments.insert("args".into(), Value::Array(args));
    let cwd_text = cwd.to_string_lossy().into_owned();
    arguments.insert("cwd".into(), Value::String(cwd_text));
    if !env.is_empty() {
        arguments.insert("env".into(), Value::Object(env));
    }
    arguments.insert("stopOnEntry".into(), Value::Bool(config.stop_on_entry));
    if let Some((key, value)) = config
        .console
        .and_then(|console| console_key(&spec, console))
    {
        arguments.insert(key.into(), value);
    }
    if !spec.init_commands.is_empty() {
        let mut commands = match arguments.remove("initCommands") {
            Some(Value::Array(commands)) => commands,
            _ => Vec::new(),
        };
        commands.extend(spec.init_commands.iter().cloned().map(Value::String));
        arguments.insert("initCommands".into(), Value::Array(commands));
    }
    if let Value::Object(adapter_args) = adapter_args {
        arguments.extend(adapter_args);
    }

    Ok(SessionOptions {
        adapter: spec,
        cwd,
        request: StartRequest::Launch,
        arguments,
        breakpoints,
        exception_filters: None,
        log: if settings.log_payloads {
            LogMode::Payloads
        } else {
            LogMode::Metadata
        },
        expansions: expansions(&config, &values),
        stack_proves_stop: false,
        timeouts: super::timeouts(&settings),
    })
}

/// The language `config` debugs, as far as it says or the project shows: its
/// `adapter_args.sourceLanguages`' first (codelldb's key), a Python
/// program's, else Rust in a Cargo project. Rust's gets an LLDB adapter the
/// toolchain's formatters.
fn debuggee_language(config: &LaunchConfig, root: &Path) -> Option<&'static str> {
    let said = (config.adapter_args.get("sourceLanguages"))
        .and_then(Value::as_array)
        .and_then(|languages| languages.first())
        .and_then(Value::as_str);
    match said {
        Some("rust") => return Some("rust"),
        Some("python") => return Some("python"),
        Some("c") => return Some("c"),
        Some("cpp") => return Some("cpp"),
        Some(_) => return None,
        None => {}
    }
    if let Some(Program::Written(program)) = &config.program
        && Path::new(program)
            .extension()
            .is_some_and(|ext| ext == "py")
    {
        return Some("python");
    }
    root.join(verbatim!("Cargo.toml"))
        .is_file()
        .then_some("rust")
}

/// `path` with its `.` and `..` worked out by its words alone, if that is
/// `root` or under it.
fn under_root(root: &Path, path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !resolved.pop() {
                    return None;
                }
            }
            other => resolved.push(other),
        }
    }
    resolved.starts_with(root).then_some(resolved)
}

/// The key `spec`'s adapter reads for where the program's output goes, and
/// its value for `console`: none for an adapter with no such key (delve's
/// output goes to the console).
fn console_key(spec: &AdapterSpec, console: Console) -> Option<(&'static str, Value)> {
    let integrated = console == Console::Integrated;
    match spec.name.as_str() {
        "debugpy" => Some((
            "console",
            Value::String(if integrated {
                verbatim!("integratedTerminal").into()
            } else {
                verbatim!("internalConsole").into()
            }),
        )),
        "lldb-dap" => Some(("runInTerminal", Value::Bool(integrated))),
        "codelldb" => Some((
            "terminal",
            Value::String(if integrated {
                verbatim!("integrated").into()
            } else {
                verbatim!("console").into()
            }),
        )),
        _ => None,
    }
}

/// Expands the variables in every string in `value`.
fn expand_strings(value: &mut Value, values: &Values) -> Result<(), String> {
    match value {
        Value::String(text) => *text = variables::expand(text, values)?,
        Value::Array(items) => {
            for item in items {
                expand_strings(item, values)?;
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                expand_strings(item, values)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Every `${env:NAME}` in `config`'s strings, with what it expands to, for
/// the protocol log's redaction.
fn expansions(config: &LaunchConfig, values: &Values) -> Vec<EnvExpansion> {
    let mut texts: Vec<String> = Vec::new();
    if let Some(Program::Written(program)) = &config.program {
        texts.push(program.clone());
    }
    texts.extend(config.args.iter().cloned());
    texts.extend(config.env.values().cloned());
    if let Cwd::Expand(cwd) = &config.cwd {
        texts.push(cwd.clone());
    }
    collect_strings(&Value::Object(config.adapter_args.clone()), &mut texts);
    let mut names: Vec<String> = Vec::new();
    for text in &texts {
        let mut rest = text.as_str();
        while let Some(start) = rest.find(verbatim!("${env:")) {
            let after = &rest[start + 6..];
            let Some(end) = after.find('}') else { break };
            let name = &after[..end];
            if !names.iter().any(|known| known == name) {
                names.push(name.to_owned());
            }
            rest = &after[end + 1..];
        }
    }
    (names.into_iter())
        .map(|name| {
            let value = values.env.get(&name).cloned().unwrap_or_default();
            EnvExpansion::new(name, value)
        })
        .filter(|expansion| !expansion.value.is_empty())
        .collect()
}

fn collect_strings(value: &Value, into: &mut Vec<String>) {
    match value {
        Value::String(text) => into.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| collect_strings(item, into)),
        Value::Object(map) => map.values().for_each(|item| collect_strings(item, into)),
        _ => {}
    }
}

/// Whether `line` of a build's output may be cargo's `compiler-artifact`
/// message, which [`cargo_artifact`] reads: the lines a pre-launch cargo
/// build's read keeps.
pub(crate) fn is_artifact_message(line: &str) -> bool {
    line.trim_start().starts_with('{') && line.contains(verbatim!("\"compiler-artifact\""))
}

/// The executable cargo's `compiler-artifact` message in `output` names for
/// the binary target `bin`: the last such message, as a rebuild prints it
/// again. Lines that aren't JSON (the rendered diagnostics, the terminal's
/// own) are skipped.
pub(crate) fn cargo_artifact(output: &str, bin: &str) -> Option<PathBuf> {
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
        let Some(target) = message.get("target") else {
            continue;
        };
        let is_bin = (target.get("kind").and_then(Value::as_array))
            .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")));
        if !is_bin || target.get("name").and_then(Value::as_str) != Some(bin) {
            continue;
        }
        if let Some(executable) = message.get("executable").and_then(Value::as_str) {
            found = Some(PathBuf::from(executable));
        }
    }
    found
}

/// Where Yama's ptrace scope is read, on Linux.
pub(crate) const PTRACE_SCOPE_FILE: &str = "/proc/sys/kernel/yama/ptrace_scope";

/// Why a launch failed, when Tenmoku can say more than the adapter did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Linux refused to let the adapter trace the program: Yama's
    /// `kernel.yama.ptrace_scope`, with its value if it could be read.
    Ptrace(Option<String>),
    /// macOS refused to let the adapter attach: a hardened-runtime program
    /// without `com.apple.security.get-task-allow`.
    Signing,
}

impl Refusal {
    /// The toast's message.
    pub(crate) fn message(&self) -> String {
        match self {
            Refusal::Ptrace(Some(scope)) => {
                tr!("workbench-debug-ptrace-refused", scope = scope.as_str()).into()
            }
            Refusal::Ptrace(None) => tr!("workbench-debug-ptrace-refused-unknown").into(),
            Refusal::Signing => tr!("workbench-debug-signing-refused").into(),
        }
    }
}

/// Whether `message`, a failed launch's, is a ptrace refusal on Linux or a
/// signing refusal on macOS, by `adapter` (a native adapter's permission
/// error counts too). `scope` reads the ptrace scope, only when it's asked
/// for.
pub(crate) fn refusal(
    message: &str,
    adapter: &AdapterSpec,
    os: HostOs,
    scope: impl FnOnce() -> Option<String>,
) -> Option<Refusal> {
    let lower = message.to_lowercase();
    let native = ["lldb-dap", "codelldb", "delve"].contains(&adapter.name.as_str())
        || (adapter.languages.iter()).any(|language| ["rust", "c", "cpp"].contains(&&**language));
    let permission = lower.contains(verbatim!("operation not permitted"))
        || lower.contains(verbatim!("permission denied"));
    match os {
        HostOs::Linux if lower.contains("ptrace") || (native && permission) => {
            let scope = scope().map(|scope| scope.trim().to_owned());
            Some(Refusal::Ptrace(scope.filter(|scope| !scope.is_empty())))
        }
        HostOs::MacOs
            if lower.contains(verbatim!("get-task-allow"))
                || lower.contains(verbatim!("hardened runtime"))
                || (native && lower.contains(verbatim!("not allowed to attach"))) =>
        {
            Some(Refusal::Signing)
        }
        _ => None,
    }
}

/// The ptrace scope as the system has it.
pub(crate) fn read_ptrace_scope() -> Option<String> {
    std::fs::read_to_string(Path::new(PTRACE_SCOPE_FILE)).ok()
}
