//! The inherited Claude Code settings check (decision record § 3,
//! "Inherited settings that affect confinement are checked, and fail
//! closed").
//!
//! Claude Code merges every settings file it reads, and their lists merge
//! rather than replace. So the user's own files could widen what the
//! security profile confines. Before a session starts, before every prompt,
//! and whenever a watched file changes, fe-acp reads every source of the
//! table below and refuses anything it can't prove only tightens:
//!
//! | Source | File |
//! |---|---|
//! | Managed file | `<managed dir>/managed-settings.json` |
//! | Managed drop-ins | `<managed dir>/managed-settings.d/*.json`, every file |
//! | Managed preferences (macOS) | the [`MANAGED_PREFERENCES_DOMAIN`] property list, machine-wide or per user: never inspected, so its presence refuses |
//! | User | `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json` |
//! | Project, in the worktree | `.claude/settings.json` |
//! | Local, in the worktree | `.claude/settings.local.json` |
//! | Local, in the main checkout | `<main checkout>/.claude/settings.local.json`, the main checkout being the parent of the git common dir (F8) |
//! | Project, in the opened root | `<root>/.claude/settings.json`, checked ahead because Update lane copies it into the worktree |
//!
//! From each file only `sandbox`, `permissions.allow` and
//! `permissions.additionalDirectories` are kept, plus the managed sources'
//! `allowManagedPermissionRulesOnly` (F6). Everything else, `env` and
//! `apiKeyHelper` included, is dropped as the file is parsed, and never kept
//! or logged.
//!
//! The constants here (the directories, the preferences domain, the rule
//! kinds that grant paths or domains) are taken from Claude Code's
//! managed-settings and sandboxing documentation, and are checked against it
//! at each tested adapter version (§ 12's smoke run, step 14).

use std::fmt;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::claude::{AgentState, ClaudeAgent};

/// Claude Code's managed settings directory on Linux.
pub const MANAGED_DIR_LINUX: &str = "/etc/claude-code";

/// Claude Code's managed settings directory on macOS.
pub const MANAGED_DIR_MACOS: &str = "/Library/Application Support/ClaudeCode";

/// Where macOS keeps managed preferences from configuration profiles:
/// machine-wide in this directory, per user in a directory under it.
pub const MANAGED_PREFERENCES_MACOS: &str = "/Library/Managed Preferences";

/// Claude Code's managed-preferences domain: its property list is
/// `<domain>.plist`.
pub const MANAGED_PREFERENCES_DOMAIN: &str = "com.anthropic.claudecode";

/// The managed settings file in a managed directory.
pub const MANAGED_FILE: &str = "managed-settings.json";

/// The managed drop-in directory in a managed directory.
pub const MANAGED_DROP_INS: &str = "managed-settings.d";

/// The managed-only setting that makes Claude Code ignore every permission
/// rule but the managed ones, the profile's forced asks and deny rules
/// included (F6).
pub const MANAGED_RULES_ONLY: &str = "allowManagedPermissionRulesOnly";

/// The tools whose allow rules grant the sandbox write access to their
/// paths.
pub const EDIT_RULE_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// The tools whose allow rules (`<tool>(domain:…)`) grant the sandbox's
/// network allowlist their domain.
pub const NETWORK_RULE_TOOLS: [&str; 1] = ["WebFetch"];

/// Where Claude Code's settings files are, outside the project: the managed
/// directories, the managed-preferences locations, and the user's config
/// directory. [`Environment::system`] is the real machine; tests use a fake
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// Directories holding [`MANAGED_FILE`] and [`MANAGED_DROP_INS`].
    pub managed_dirs: Vec<PathBuf>,
    /// Directories where a [`MANAGED_PREFERENCES_DOMAIN`] property list, in
    /// the directory or in a directory under it, refuses.
    pub managed_preferences: Vec<PathBuf>,
    /// The user's Claude Code config directory, holding `settings.json`.
    pub user_config_dir: Option<PathBuf>,
    /// Why this platform's policy can't be inspected at all, when Claude
    /// Code reads it from a place fe-acp has no reader for (the Windows
    /// registry).
    pub uninspectable_platform: Option<String>,
}

impl Environment {
    /// This machine's: the platform's managed locations, and
    /// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
    #[must_use]
    pub fn system() -> Self {
        let user_config_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|home| !home.is_empty())
                    .map(|home| PathBuf::from(home).join(".claude"))
            });
        let mut environment = Self {
            managed_dirs: Vec::new(),
            managed_preferences: Vec::new(),
            user_config_dir,
            uninspectable_platform: None,
        };
        if cfg!(target_os = "macos") {
            environment.managed_dirs.push(MANAGED_DIR_MACOS.into());
            environment
                .managed_preferences
                .push(MANAGED_PREFERENCES_MACOS.into());
        } else if cfg!(unix) {
            environment.managed_dirs.push(MANAGED_DIR_LINUX.into());
        } else {
            environment.uninspectable_platform = Some(
                "Claude Code reads its policy on this platform from a place Tenmoku can't inspect"
                    .into(),
            );
        }
        environment
    }
}

/// Which row of the source table a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// `managed-settings.json`.
    ManagedFile,
    /// A file in `managed-settings.d`.
    ManagedDropIn,
    /// A managed-preferences property list: never inspected.
    ManagedPreferences,
    /// The user's `settings.json`.
    User,
    /// The worktree's `.claude/settings.json`.
    WorktreeProject,
    /// The worktree's `.claude/settings.local.json`.
    WorktreeLocal,
    /// The main checkout's `.claude/settings.local.json`.
    MainCheckoutLocal,
    /// The opened root's `.claude/settings.json`, checked ahead of Update
    /// lane.
    RootProject,
}

impl SourceKind {
    /// A managed source: where [`MANAGED_RULES_ONLY`] counts.
    #[must_use]
    pub fn is_managed(self) -> bool {
        matches!(
            self,
            Self::ManagedFile | Self::ManagedDropIn | Self::ManagedPreferences
        )
    }

    /// A project or local file, whose relative `Edit(…)` allow rules grant
    /// only paths under the project, which the sandbox already allows.
    #[must_use]
    pub fn is_project(self) -> bool {
        matches!(
            self,
            Self::WorktreeProject
                | Self::WorktreeLocal
                | Self::MainCheckoutLocal
                | Self::RootProject
        )
    }
}

/// One settings file of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Its row.
    pub kind: SourceKind,
    /// Where it is (it may not exist).
    pub path: PathBuf,
}

/// Why Claude Agent won't start, or won't send a prompt: § 4's "Sandbox
/// loosened" state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loosened {
    /// `key` in `file` widens what the profile confines.
    Widening {
        /// The settings file.
        file: PathBuf,
        /// The key, and for an allow rule the rule:
        /// `sandbox.excludedCommands`, `permissions.allow: Edit(//etc/**)`.
        key: String,
    },
    /// A managed source sets [`MANAGED_RULES_ONLY`], so Claude Code would
    /// ignore the profile's forced asks and deny rules (F6).
    ManagedRulesOnly {
        /// The managed file or drop-in.
        file: PathBuf,
    },
    /// The file isn't JSON, or isn't an object.
    Unparsable {
        /// The settings file.
        file: PathBuf,
        /// What the parser said.
        error: String,
    },
    /// The file exists but can't be read.
    Unreadable {
        /// The settings file.
        file: PathBuf,
        /// Why.
        error: String,
    },
    /// Claude Code has managed preferences (a configuration profile) that
    /// Tenmoku can't inspect.
    Uninspectable {
        /// The property list found.
        path: PathBuf,
    },
    /// This platform's policy can't be inspected at all.
    UninspectablePlatform {
        /// Why.
        reason: String,
    },
    /// Git couldn't resolve the directories the sandbox must deny.
    Git {
        /// What failed.
        reason: String,
    },
}

impl fmt::Display for Loosened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Widening { file, key } => write!(
                f,
                "Your Claude Code settings loosen its sandbox (`{key}` in `{}`), so Claude Agent won't run. Remove it, or run Claude Code outside Tenmoku.",
                file.display()
            ),
            Self::ManagedRulesOnly { file } => write!(
                f,
                "Your managed Claude Code settings set `{MANAGED_RULES_ONLY}` (in `{}`), which would make Claude Code ignore Tenmoku's permission rules, so Claude Agent won't run.",
                file.display()
            ),
            Self::Unparsable { file, error } => write!(
                f,
                "A Claude Code settings file doesn't parse (`{}`: {error}), so Claude Agent won't run.",
                file.display()
            ),
            Self::Unreadable { file, error } => write!(
                f,
                "A Claude Code settings file can't be read (`{}`: {error}), so Claude Agent won't run.",
                file.display()
            ),
            Self::Uninspectable { path } => write!(
                f,
                "Claude Code has managed preferences (a configuration profile) that Tenmoku can't inspect (`{}`), so Claude Agent won't run.",
                path.display()
            ),
            Self::UninspectablePlatform { reason } => {
                write!(f, "{reason}, so Claude Agent won't run.")
            }
            Self::Git { reason } => write!(
                f,
                "Git couldn't resolve the repository's directories for Claude Code's sandbox to deny ({reason}), so Claude Agent won't run."
            ),
        }
    }
}

/// What fe-acp keeps of one settings file: the three keys it reads, and
/// [`MANAGED_RULES_ONLY`]. The rest of the file is dropped as it is parsed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Kept {
    /// `sandbox`.
    pub sandbox: Option<Value>,
    /// `permissions.allow`.
    pub allow: Option<Value>,
    /// `permissions.additionalDirectories`.
    pub additional_directories: Option<Value>,
    /// `allowManagedPermissionRulesOnly`, at the top level or under
    /// `permissions`.
    pub managed_rules_only: Option<Value>,
    /// `permissions` was there but isn't an object.
    pub permissions_not_an_object: bool,
}

/// Reads the settings file at `path`, keeping only what [`Kept`] holds.
/// `Ok(None)` when there's no file.
///
/// # Errors
///
/// [`Loosened::Unreadable`] when it exists but can't be read (a directory,
/// say), and [`Loosened::Unparsable`] when it isn't a JSON object.
pub fn read(path: &Path) -> Result<Option<Kept>, Loosened> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(Loosened::Unreadable {
                file: path.to_owned(),
                error: err.to_string(),
            });
        }
    };
    let unparsable = |error: String| Loosened::Unparsable {
        file: path.to_owned(),
        error,
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|err| unparsable(err.to_string()))?;
    let Value::Object(mut file) = value else {
        return Err(unparsable("not a JSON object".into()));
    };
    let mut kept = Kept {
        sandbox: file.remove("sandbox"),
        managed_rules_only: file.remove(MANAGED_RULES_ONLY),
        ..Kept::default()
    };
    match file.remove("permissions") {
        None => {}
        Some(Value::Object(mut permissions)) => {
            kept.allow = permissions.remove("allow");
            kept.additional_directories = permissions.remove("additionalDirectories");
            // Either place counts: keep the one that's set.
            if let Some(nested) = permissions.remove(MANAGED_RULES_ONLY)
                && kept
                    .managed_rules_only
                    .as_ref()
                    .is_none_or(|top| !is_set(top))
            {
                kept.managed_rules_only = Some(nested);
            }
        }
        Some(_) => kept.permissions_not_an_object = true,
    }
    // `file` (with `env`, `apiKeyHelper` and the rest) is dropped here.
    Ok(Some(kept))
}

/// Checks what was kept of the file `source`.
///
/// # Errors
///
/// The first key that doesn't only tighten, in the order the record lists
/// them.
pub fn check_kept(source: &Source, kept: &Kept) -> Result<(), Loosened> {
    let widening = |key: String| Loosened::Widening {
        file: source.path.clone(),
        key,
    };
    if source.kind.is_managed() && kept.managed_rules_only.as_ref().is_some_and(is_set) {
        return Err(Loosened::ManagedRulesOnly {
            file: source.path.clone(),
        });
    }
    if let Some(sandbox) = &kept.sandbox {
        check_sandbox(sandbox).map_err(widening)?;
    }
    if kept.permissions_not_an_object {
        return Err(widening("permissions".into()));
    }
    match &kept.additional_directories {
        None => {}
        Some(Value::Array(directories)) if directories.is_empty() => {}
        Some(_) => return Err(widening("permissions.additionalDirectories".into())),
    }
    match &kept.allow {
        None => {}
        Some(Value::Array(rules)) => {
            for rule in rules {
                let Value::String(rule) = rule else {
                    return Err(widening(format!("permissions.allow: {rule}")));
                };
                if !allow_rule_tightens(rule, source.kind.is_project()) {
                    return Err(widening(format!("permissions.allow: {rule}")));
                }
            }
        }
        Some(_) => return Err(widening("permissions.allow".into())),
    }
    Ok(())
}

/// Whether a [`MANAGED_RULES_ONLY`] value may turn it on: anything but
/// `false` or `null`.
fn is_set(value: &Value) -> bool {
    value != &Value::Bool(false) && !value.is_null()
}

/// Checks a `sandbox` value: `Err` names the first key that isn't one of
/// the values that can only tighten.
fn check_sandbox(sandbox: &Value) -> Result<(), String> {
    let Value::Object(sandbox) = sandbox else {
        return Err("sandbox".into());
    };
    for (key, value) in sandbox {
        let ok = match key.as_str() {
            "enabled" | "failIfUnavailable" => value == &Value::Bool(true),
            "allowUnsandboxedCommands" | "autoAllowBashIfSandboxed" => value == &Value::Bool(false),
            "filesystem" => {
                check_object(value, "sandbox.filesystem", |key, value| match key {
                    "denyWrite" | "denyRead" => strings(value).is_some(),
                    _ => false,
                })?;
                true
            }
            "network" => {
                check_object(value, "sandbox.network", |key, value| match key {
                    "deniedDomains" => strings(value).is_some(),
                    "strictAllowlist" | "allowManagedDomainsOnly" => value == &Value::Bool(true),
                    "allowedDomains" => strings(value)
                        .is_some_and(|domains| domains.iter().all(|d| !could_be_loopback(d))),
                    _ => false,
                })?;
                true
            }
            _ => false,
        };
        if !ok {
            return Err(format!("sandbox.{key}"));
        }
    }
    Ok(())
}

/// Checks each key of the object `value` at `path` with `ok`.
fn check_object(
    value: &Value,
    path: &str,
    ok: impl Fn(&str, &Value) -> bool,
) -> Result<(), String> {
    let Value::Object(object) = value else {
        return Err(path.to_owned());
    };
    match object.iter().find(|(key, value)| !ok(key, value)) {
        Some((key, _)) => Err(format!("{path}.{key}")),
        None => Ok(()),
    }
}

/// The strings of an array of strings.
fn strings(value: &Value) -> Option<Vec<&str>> {
    value.as_array()?.iter().map(Value::as_str).collect()
}

/// Whether the `permissions.allow` rule `rule` grants the sandbox neither a
/// path nor a domain beyond what the profile allows. `project` is whether it
/// comes from a project or local file.
#[must_use]
pub fn allow_rule_tightens(rule: &str, project: bool) -> bool {
    let (tool, specifier) = match rule.split_once('(') {
        Some((tool, rest)) => (tool, rest.strip_suffix(')')),
        None => (rule, Some("")),
    };
    let tool = tool.trim();
    let named = |tools: &[&str]| tools.iter().any(|t| t.eq_ignore_ascii_case(tool));
    // A wildcard tool name (other than an MCP server's) could name one of
    // the tools below.
    if tool.contains('*') && !tool.starts_with("mcp__") {
        return false;
    }
    if named(&EDIT_RULE_TOOLS) {
        return match specifier {
            Some(path) if !path.is_empty() => project && is_project_relative(path),
            _ => false,
        };
    }
    if named(&NETWORK_RULE_TOOLS) {
        return match specifier.and_then(|s| s.strip_prefix("domain:")) {
            Some(domain) => !could_be_loopback(domain),
            None => false,
        };
    }
    true
}

/// A path relative to the project: no leading `/`, `//`, `~`, `\` or `$`,
/// no drive, and no `..`.
fn is_project_relative(path: &str) -> bool {
    let path = path.trim();
    !path.is_empty()
        && !path.starts_with(['/', '~', '\\', '$'])
        && !path.contains(':')
        && Path::new(&path.replace('\\', "/"))
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

/// Whether the domain `domain` (from `allowedDomains` or a
/// `WebFetch(domain:…)` rule) is, or could match, a loopback name or
/// address: `localhost`, `*.localhost`, `127.*`, `::1`, the unspecified
/// address, or a wildcard that might match one of them.
///
/// It fails closed: only what can be shown to name something else passes.
/// A wildcard passes only as `*.<suffix>`, with no port, whose suffix is a
/// host name of letters, digits, `-` and `_` whose last label has a letter
/// and isn't `localhost`: such a pattern matches only names ending in that
/// suffix, never an address or `localhost`. Anything else with a `*` in it
/// (`*`, `127.*`, `foo*.localhost`, `*.example.com:*`) is refused, as is
/// anything that isn't a plain host, an address, or `host:port`.
#[must_use]
pub fn could_be_loopback(domain: &str) -> bool {
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() || domain.contains("://") || domain.contains('/') {
        return true;
    }
    if domain.contains('*') {
        return !domain
            .strip_prefix("*.")
            .is_some_and(|suffix| is_plain_name(suffix.trim_end_matches('.')));
    }
    let host = host_of(&domain);
    let host = host.trim_end_matches('.');
    if host.is_empty() || host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if let Ok(ip) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        return match ip {
            IpAddr::V4(ip) => ip.is_loopback() || ip.is_unspecified(),
            IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    // IPv4-mapped and IPv4-compatible forms.
                    || ip
                        .to_ipv4()
                        .is_some_and(|ip| ip.is_loopback() || ip.is_unspecified())
            }
        };
    }
    // Numeric hosts in the forms `inet_aton` reads (`127.1`, `2130706433`,
    // `0x7f.1`): loopback unless they read as some other address.
    if host.split('.').all(is_numeric_label) {
        return inet_aton(host).is_none_or(|ip| ip.is_loopback() || ip.is_unspecified());
    }
    // Anything but a plain host name can't be shown to be safe.
    !is_host_name(host)
}

/// A host name: dot-separated labels of letters, digits, `-` and `_`.
fn is_host_name(host: &str) -> bool {
    host.split('.').all(|label| {
        !label.is_empty()
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })
}

/// A host name no loopback name or address can end in: its last label
/// isn't a number in any form `inet_aton` reads (so no matching name is a
/// numeric address), and it isn't `localhost` or under it.
fn is_plain_name(suffix: &str) -> bool {
    is_host_name(suffix)
        && suffix
            .rsplit('.')
            .next()
            .is_some_and(|last| !is_numeric_label(last))
        && suffix != "localhost"
        && !suffix.ends_with(".localhost")
}

/// A label `inet_aton` reads as a number: decimal (or octal), or `0x` and
/// hexadecimal digits.
fn is_numeric_label(label: &str) -> bool {
    !label.is_empty()
        && (label.bytes().all(|b| b.is_ascii_digit())
            || label
                .strip_prefix("0x")
                .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit())))
}

/// The host of `host[:port]` or `[v6]:port`.
fn host_of(domain: &str) -> &str {
    if let Some(rest) = domain.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    match domain.split_once(':') {
        // One colon: a port. More: an IPv6 address.
        Some((host, port)) if !port.contains(':') => host,
        _ => domain,
    }
}

/// `inet_aton`'s reading of a numeric IPv4 address: one to four parts,
/// each decimal, octal (leading `0`) or hexadecimal (`0x`).
fn inet_aton(host: &str) -> Option<Ipv4Addr> {
    let parts: Vec<u64> = host
        .split('.')
        .map(|part| {
            if let Some(hex) = part.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).ok()
            } else if part.len() > 1 && part.starts_with('0') {
                u64::from_str_radix(&part[1..], 8).ok()
            } else {
                part.parse().ok()
            }
        })
        .collect::<Option<_>>()?;
    let (last, head) = parts.split_last()?;
    if head.len() > 3 || head.iter().any(|&part| part > 255) {
        return None;
    }
    let width = 8 * (4 - head.len() as u32);
    if *last >= 1u64 << width {
        return None;
    }
    let mut address = 0u64;
    for (i, part) in head.iter().enumerate() {
        address |= part << (24 - 8 * i);
    }
    Some(Ipv4Addr::from(u32::try_from(address | last).ok()?))
}

/// The check for one lane: the sources of the table for its worktree, the
/// opened root and the main checkout, on an [`Environment`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsCheck {
    environment: Environment,
    worktree: PathBuf,
    root: PathBuf,
    main_checkout: PathBuf,
}

impl SettingsCheck {
    /// The check for a session whose `cwd` is `worktree`, in the opened
    /// `root`, whose repository's main checkout is `main_checkout` (the
    /// parent of `git rev-parse --git-common-dir`).
    #[must_use]
    pub fn new(
        environment: Environment,
        worktree: impl Into<PathBuf>,
        root: impl Into<PathBuf>,
        main_checkout: impl Into<PathBuf>,
    ) -> Self {
        Self {
            environment,
            worktree: worktree.into(),
            root: root.into(),
            main_checkout: main_checkout.into(),
        }
    }

    /// The environment it reads.
    #[must_use]
    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    /// Every source of the table, in its order: each drop-in now in a
    /// `managed-settings.d` (sorted by name), and each managed-preferences
    /// property list that exists. Files that don't exist are listed too,
    /// except drop-ins and property lists.
    ///
    /// # Errors
    ///
    /// [`Loosened::Unreadable`] naming the path, when a directory that
    /// exists can't be listed, or an entry or a property list's place can't
    /// be inspected: what isn't seen can't be shown to be absent.
    pub fn sources(&self) -> Result<Vec<Source>, Loosened> {
        let source = |kind, path: PathBuf| Source { kind, path };
        let mut sources = Vec::new();
        for dir in &self.environment.managed_dirs {
            sources.push(source(SourceKind::ManagedFile, dir.join(MANAGED_FILE)));
            for path in drop_ins(&dir.join(MANAGED_DROP_INS))? {
                sources.push(source(SourceKind::ManagedDropIn, path));
            }
        }
        for dir in &self.environment.managed_preferences {
            for path in managed_preferences(dir)? {
                sources.push(source(SourceKind::ManagedPreferences, path));
            }
        }
        if let Some(dir) = &self.environment.user_config_dir {
            sources.push(source(SourceKind::User, dir.join("settings.json")));
        }
        sources.extend(self.project_sources());
        Ok(sources)
    }

    /// The four project and local files of the table.
    fn project_sources(&self) -> [Source; 4] {
        let source = |kind, path: PathBuf| Source { kind, path };
        let claude = |dir: &Path, file: &str| dir.join(".claude").join(file);
        [
            source(
                SourceKind::WorktreeProject,
                claude(&self.worktree, "settings.json"),
            ),
            source(
                SourceKind::WorktreeLocal,
                claude(&self.worktree, "settings.local.json"),
            ),
            source(
                SourceKind::MainCheckoutLocal,
                claude(&self.main_checkout, "settings.local.json"),
            ),
            source(SourceKind::RootProject, claude(&self.root, "settings.json")),
        ]
    }

    /// Runs the check over every source.
    ///
    /// # Errors
    ///
    /// The first source, in table order, that refuses, and why.
    pub fn check(&self) -> Result<(), Loosened> {
        if let Some(reason) = &self.environment.uninspectable_platform {
            return Err(Loosened::UninspectablePlatform {
                reason: reason.clone(),
            });
        }
        for source in self.sources()? {
            if source.kind == SourceKind::ManagedPreferences {
                return Err(Loosened::Uninspectable { path: source.path });
            }
            if let Some(kept) = read(&source.path)? {
                check_kept(&source, &kept)?;
            }
        }
        Ok(())
    }

    /// What a watcher must watch while a session is open: every file of the
    /// table, each `managed-settings.d` directory (for files added, changed
    /// or removed), and each managed-preferences directory and the
    /// directories under it (where a property list appearing is a change
    /// that fails). Any of them may not exist yet.
    #[must_use]
    pub fn watched(&self) -> Vec<PathBuf> {
        let mut watched = Vec::new();
        for dir in &self.environment.managed_dirs {
            watched.push(dir.join(MANAGED_FILE));
            watched.push(dir.join(MANAGED_DROP_INS));
        }
        for dir in &self.environment.managed_preferences {
            watched.push(dir.clone());
            // One that can't be listed fails the check itself.
            watched.extend(subdirectories(dir).unwrap_or_default());
        }
        if let Some(dir) = &self.environment.user_config_dir {
            watched.push(dir.join("settings.json"));
        }
        watched.extend(self.project_sources().map(|source| source.path));
        watched
    }
}

/// `path` couldn't be inspected.
fn unreadable(path: &Path, err: &std::io::Error) -> Loosened {
    Loosened::Unreadable {
        file: path.to_owned(),
        error: err.to_string(),
    }
}

/// The paths of `dir`'s entries, sorted; none when `dir` doesn't exist.
fn entries(dir: &Path) -> Result<Vec<PathBuf>, Loosened> {
    let listing = match std::fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(unreadable(dir, &err)),
    };
    let mut paths = Vec::new();
    for entry in listing {
        paths.push(entry.map_err(|err| unreadable(dir, &err))?.path());
    }
    paths.sort();
    Ok(paths)
}

/// The `*.json` files in a `managed-settings.d`, sorted by name.
fn drop_ins(dir: &Path) -> Result<Vec<PathBuf>, Loosened> {
    Ok(entries(dir)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect())
}

/// The directories directly under `dir` (links to directories included).
fn subdirectories(dir: &Path) -> Result<Vec<PathBuf>, Loosened> {
    let mut dirs = Vec::new();
    for path in entries(dir)? {
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_dir() => dirs.push(path),
            Ok(_) => {}
            // A dangling link leads nowhere.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(unreadable(&path, &err)),
        }
    }
    Ok(dirs)
}

/// The [`MANAGED_PREFERENCES_DOMAIN`] property lists in `dir` and in each
/// directory under it.
fn managed_preferences(dir: &Path) -> Result<Vec<PathBuf>, Loosened> {
    let file = format!("{MANAGED_PREFERENCES_DOMAIN}.plist");
    let mut found = Vec::new();
    for dir in std::iter::once(dir.to_owned()).chain(subdirectories(dir)?) {
        let path = dir.join(&file);
        match path.symlink_metadata() {
            Ok(_) => found.push(path),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(unreadable(&path, &err)),
        }
    }
    Ok(found)
}

impl ClaudeAgent {
    /// The watcher callback: a file [`SettingsCheck::watched`] lists
    /// changed. Re-checks every open session's sources. Each session that
    /// now fails has its running turn cancelled (`session/cancel`, its
    /// pending permission requests answered `Cancelled`) and refuses
    /// further prompts until the check passes again, before a prompt or on
    /// a later change.
    ///
    /// Returns the sessions refused, with why.
    pub async fn settings_changed(&self) -> Vec<(crate::schema::SessionId, AgentState)> {
        let (refused, cancel) = self.recheck_sessions();
        for session_id in cancel {
            if let Err(err) = self.client().cancel(session_id.clone()).await {
                log::warn!("fe-acp: cancelling {session_id} after a settings change failed: {err}");
            }
        }
        refused
    }
}
