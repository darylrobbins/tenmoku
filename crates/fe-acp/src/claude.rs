//! Claude Agent: the security profile, the version floor, the states the
//! panel shows, and [`ClaudeAgent`], the session methods that keep the
//! profile current (decision record §§ 3 and 4).
//!
//! **The profile.** [`profile`] builds `_meta.claudeCode.options` from the
//! agent settings and the lane's git directories. It is sent whole on
//! `session/new`, `session/load`, `session/resume` and the load after a
//! respawn, and the adapter builds each query afresh from it. When the
//! settings change, the next prompt is preceded by a `session/load` with the
//! new profile, so a prompt is never sent under an outdated one.
//!
//! **The check.** Before a session starts and before every prompt, the
//! inherited Claude Code settings are checked (`claude_settings`), and the
//! repository's git directories are resolved with git itself for the
//! sandbox to deny. Either failing refuses, with § 4's "Sandbox loosened".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use agent_client_protocol::ErrorCode;
use agent_client_protocol::v1::schema::{
    ContentBlock, InitializeResponse, LoadSessionResponse, Meta, NewSessionResponse,
    PromptResponse, ResumeSessionResponse, SessionId,
};
use fe_config::agent_settings::{AgentLevel, AgentSettings};
use futures::channel::oneshot;
use serde_json::{Value, json};

use crate::claude_settings::{Environment, Loosened, SettingsCheck};
use crate::launch::AgentProcess;
use crate::{AcpClient, AuthKind, Error};

/// The oldest adapter fe-acp runs: the version the record read (§ 2).
pub const MIN_ADAPTER_VERSION: (u64, u64, u64) = (0, 106, 0);

/// What a Claude Code tool does, by the permission class it falls under
/// (§ 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    /// `read`.
    Read,
    /// `search`.
    Search,
    /// `propose_edit`.
    ProposeEdit,
    /// `run_command`.
    RunCommand,
    /// `fetch`.
    Fetch,
    /// No class: the tool writes nothing and runs nothing itself, and a
    /// subagent's calls use the other tools, under the same rules.
    None,
}

/// The built-in tools Claude Agent may use, with their class: the
/// profile's `tools` (§ 3). Every other built-in tool is unavailable.
/// Checked against the SDK's `sdk-tools.d.ts` at each tested adapter
/// version.
pub const CLAUDE_TOOLS: [(&str, ToolClass); 14] = [
    ("Read", ToolClass::Read),
    ("Glob", ToolClass::Search),
    ("Grep", ToolClass::Search),
    ("Edit", ToolClass::ProposeEdit),
    ("Write", ToolClass::ProposeEdit),
    ("NotebookEdit", ToolClass::ProposeEdit),
    ("Bash", ToolClass::RunCommand),
    ("WebFetch", ToolClass::Fetch),
    ("WebSearch", ToolClass::Fetch),
    ("TodoWrite", ToolClass::None),
    ("Agent", ToolClass::None),
    ("EnterPlanMode", ToolClass::None),
    ("ExitPlanMode", ToolClass::None),
    ("AskUserQuestion", ToolClass::None),
];

/// The tools every profile forces to ask, whatever their class's level:
/// edits and commands always reach `session/request_permission`.
pub const ALWAYS_ASK: [&str; 4] = ["Edit", "Write", "NotebookEdit", "Bash"];

/// The ask rule that makes every MCP tool call ask (Claude Code's tool-name
/// wildcard).
pub const MCP_ASK_RULE: &str = "mcp__*";

/// File names the chat's deny list refuses (`chat_context::refusal`): as
/// patterns under a root.
const DENY_LIST: [&str; 5] = ["**/.env*", "**/id_*", "**/*.pem", "**/*.key", "**/.git/**"];

/// One of § 4's states, shown in place of the composer's send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    /// The adapter isn't found.
    AdapterNotInstalled,
    /// `claude` isn't found.
    ClaudeNotInstalled,
    /// `authRequired` on `session/new` or a prompt, or an
    /// `_auth/status_update` of kind `none`.
    NotLoggedIn,
    /// The adapter's `agentInfo.version` is below [`MIN_ADAPTER_VERSION`],
    /// or it gave none.
    AdapterTooOld {
        /// The version it gave.
        version: Option<String>,
    },
    /// Claude Code's sandbox can't run here (`failIfUnavailable`).
    SandboxUnavailable {
        /// The adapter's message.
        details: String,
    },
    /// The inherited-settings check failed, or git couldn't resolve the
    /// directories to deny.
    SandboxLoosened(Loosened),
    /// The root has no git repository.
    NotAGitRepository,
    /// The adapter exited, or the connection to it ended.
    Disconnected {
        /// The last lines it wrote to stderr.
        stderr: Vec<String>,
    },
    /// Any other failure: the error, for the details.
    Failed {
        /// What failed.
        details: String,
    },
}

impl AgentState {
    /// The state an error from the adapter stands for. `Disconnected`
    /// carries no stderr here; [`ClaudeAgent`] fills it in.
    #[must_use]
    pub fn from_error(error: &Error) -> Self {
        if error.code == ErrorCode::AUTH_REQUIRED {
            return Self::NotLoggedIn;
        }
        if crate::client::is_disconnect(error) {
            return Self::Disconnected { stderr: Vec::new() };
        }
        let details = error_text(error);
        if is_sandbox_unavailable(&details) {
            return Self::SandboxUnavailable { details };
        }
        Self::Failed { details }
    }
}

/// An error's message and data, as one string.
fn error_text(error: &Error) -> String {
    match &error.data {
        Some(Value::String(data)) => format!("{}: {data}", error.message),
        Some(data) => format!("{}: {data}", error.message),
        None => error.message.clone(),
    }
}

/// Whether the adapter's error says the sandbox can't run: Claude Code's
/// `failIfUnavailable` refusal.
fn is_sandbox_unavailable(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("sandbox")
        && [
            "unavailable",
            "not available",
            "bubblewrap",
            "bwrap",
            "failifunavailable",
        ]
        .iter()
        .any(|word| text.contains(word))
}

/// Checks the adapter's `initialize` answer against the version floor.
///
/// # Errors
///
/// [`AgentState::AdapterTooOld`] when `agentInfo.version` is below
/// [`MIN_ADAPTER_VERSION`], can't be read, or is missing.
pub fn check_version(response: &InitializeResponse) -> Result<(), AgentState> {
    let version = response
        .agent_info
        .as_ref()
        .map(|info| info.version.clone());
    let parsed = version.as_deref().and_then(parse_version);
    match parsed {
        Some(parsed) if parsed >= MIN_ADAPTER_VERSION => Ok(()),
        _ => Err(AgentState::AdapterTooOld { version }),
    }
}

/// `major.minor.patch` from a version such as `0.106.0` or `v0.107.1-beta.2`.
fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// A Claude Agent lane: the opened root and the lane's worktree, the
/// session's `cwd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lane {
    /// The opened root.
    pub root: PathBuf,
    /// The lane's worktree.
    pub worktree: PathBuf,
}

/// A lane's git directories, resolved with git itself (§ 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneDirs {
    /// The opened root.
    pub root: PathBuf,
    /// The lane's worktree.
    pub worktree: PathBuf,
    /// The repository's common dir (`--git-common-dir`).
    pub common_dir: PathBuf,
    /// The worktree's own admin dir (`<common>/worktrees/<n>`).
    pub worktree_admin_dir: PathBuf,
    /// The root's own admin dir: the common dir for a main checkout, its
    /// own admin dir when the root is itself a linked worktree.
    pub root_admin_dir: PathBuf,
}

impl LaneDirs {
    /// Resolves `lane`'s directories with `git`:
    ///
    /// - `git -C <worktree> rev-parse --path-format=absolute
    ///   --git-common-dir --git-dir`;
    /// - `git -C <root> rev-parse --path-format=absolute --git-dir`.
    ///
    /// # Errors
    ///
    /// [`AgentState::NotAGitRepository`] when the root isn't in one, and
    /// [`AgentState::SandboxLoosened`] with [`Loosened::Git`] for any other
    /// failure: git missing, `rev-parse` failing, or a directory that
    /// doesn't exist.
    pub async fn resolve(git: &Path, lane: &Lane) -> Result<Self, AgentState> {
        let root =
            rev_parse(git, &lane.root, &["--git-dir"])
                .await
                .map_err(|failure| match failure {
                    RevParse::NotARepository(_) => AgentState::NotAGitRepository,
                    RevParse::Failed(reason) => git_failure(reason),
                })?;
        let worktree = rev_parse(git, &lane.worktree, &["--git-common-dir", "--git-dir"])
            .await
            .map_err(|failure| match failure {
                RevParse::NotARepository(reason) | RevParse::Failed(reason) => git_failure(reason),
            })?;
        let [root_admin_dir] = <[PathBuf; 1]>::try_from(root)
            .map_err(|_| git_failure("unexpected rev-parse output for the root"))?;
        let [common_dir, worktree_admin_dir] = <[PathBuf; 2]>::try_from(worktree)
            .map_err(|_| git_failure("unexpected rev-parse output for the worktree"))?;
        let dirs = Self {
            root: lane.root.clone(),
            worktree: lane.worktree.clone(),
            common_dir,
            worktree_admin_dir,
            root_admin_dir,
        };
        // The profile's deny rules and `denyWrite` name these directories,
        // and glob syntax in them would make those patterns, which could
        // miss the directory meant.
        for dir in [
            &dirs.root,
            &dirs.worktree,
            &dirs.common_dir,
            &dirs.worktree_admin_dir,
            &dirs.root_admin_dir,
        ] {
            if dir.to_str().is_none() {
                return Err(git_failure(format!("{} isn't a UTF-8 path", dir.display())));
            }
            let text = path_text(dir);
            if text.contains(GLOB_SYNTAX) {
                return Err(git_failure(format!(
                    "{text} has glob characters, which Claude Code's sandbox and permission rules read as a pattern"
                )));
            }
        }
        Ok(dirs)
    }

    /// The main checkout, the parent of the common dir, whose
    /// `.claude/settings.local.json` Claude Code reads for a linked
    /// worktree's session (F8).
    #[must_use]
    pub fn main_checkout(&self) -> PathBuf {
        self.common_dir
            .parent()
            .map_or_else(|| self.common_dir.clone(), Path::to_path_buf)
    }

    /// The sandbox's `filesystem.denyWrite`: the common dir and both admin
    /// dirs, deduplicated, in that order.
    #[must_use]
    pub fn deny_write(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for dir in [
            &self.common_dir,
            &self.worktree_admin_dir,
            &self.root_admin_dir,
        ] {
            if !dirs.contains(dir) {
                dirs.push(dir.clone());
            }
        }
        dirs
    }
}

fn git_failure(reason: impl Into<String>) -> AgentState {
    AgentState::SandboxLoosened(Loosened::Git {
        reason: reason.into(),
    })
}

/// Why `rev-parse` failed.
enum RevParse {
    /// The directory isn't in a git repository.
    NotARepository(String),
    /// Anything else.
    Failed(String),
}

/// `git -C <dir> rev-parse --path-format=absolute <args>`: one existing
/// directory per line, made canonical (on Windows without the verbatim
/// `\\?\` prefix where the path doesn't need it, as git and Claude Code
/// write paths).
async fn rev_parse(git: &Path, dir: &Path, args: &[&str]) -> Result<Vec<PathBuf>, RevParse> {
    let mut command = async_process::Command::new(git);
    command
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--path-format=absolute"])
        .args(args)
        .stdin(std::process::Stdio::null());
    // The answer must be about `dir`, never a repository the environment
    // points at.
    for variable in [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ] {
        command.env_remove(variable);
    }
    let output = command
        .output()
        .await
        .map_err(|err| RevParse::Failed(format!("running {} failed: {err}", git.display())))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let reason = format!(
            "git rev-parse in {} failed ({}): {stderr}",
            dir.display(),
            output.status
        );
        return Err(if stderr.contains("not a git repository") {
            RevParse::NotARepository(reason)
        } else {
            RevParse::Failed(reason)
        });
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| RevParse::Failed("git rev-parse printed a path that isn't UTF-8".into()))?;
    stdout
        .lines()
        .map(|line| {
            let path = PathBuf::from(line);
            dunce::canonicalize(&path)
                .map_err(|err| RevParse::Failed(format!("{} doesn't exist: {err}", path.display())))
        })
        .collect()
}

/// The security profile of § 3, as the `_meta` of `session/new`,
/// `session/load` and `session/resume`: `{"claudeCode": {"options": …}}`.
///
/// It holds:
///
/// - mode `default`, and `allowDangerouslySkipPermissions: false`;
/// - `tools`: [`CLAUDE_TOOLS`] less each class at `deny`;
/// - `settings.permissions.ask`: [`ALWAYS_ASK`], each tool of a class at
///   `ask`, and [`MCP_ASK_RULE`] when MCP servers run and `other` is `ask`;
/// - `settings.permissions.deny`: the chat's deny list, as `Read(…)` and
///   `Edit(…)` rules anchored with `//` at the root and the worktree;
/// - `strictMcpConfig`: true unless `mcp_servers` is on and `other` isn't
///   `deny`;
/// - `sandbox`: enabled, failing if unavailable, never unsandboxed, writes
///   allowed in `sandbox_allow_write`'s absolute paths outside the root and
///   the git directories, and denied in the git directories.
///
/// The same settings and directories always give the same bytes.
#[must_use]
pub fn profile(settings: &AgentSettings, dirs: &LaneDirs) -> Meta {
    let permissions = settings.effective_permissions();
    let level = |class: ToolClass| match class {
        ToolClass::Read => Some(permissions.read),
        ToolClass::Search => Some(permissions.search),
        ToolClass::ProposeEdit => Some(permissions.propose_edit),
        ToolClass::RunCommand => Some(permissions.run_command),
        ToolClass::Fetch => Some(permissions.fetch),
        ToolClass::None => None,
    };
    let tools: Vec<&str> = CLAUDE_TOOLS
        .iter()
        .filter(|(_, class)| level(*class) != Some(AgentLevel::Deny))
        .map(|(name, _)| *name)
        .collect();
    let mut ask: Vec<String> = ALWAYS_ASK.iter().map(|tool| (*tool).to_owned()).collect();
    for (name, class) in CLAUDE_TOOLS {
        if level(class) == Some(AgentLevel::Ask) && !ask.iter().any(|tool| tool == name) {
            ask.push(name.to_owned());
        }
    }
    let mcp = settings.claude_code.mcp_servers && permissions.other != AgentLevel::Deny;
    if mcp && permissions.other == AgentLevel::Ask {
        ask.push(MCP_ASK_RULE.to_owned());
    }
    let mut deny = Vec::new();
    let mut anchors = vec![&dirs.root];
    if dirs.worktree != dirs.root {
        anchors.push(&dirs.worktree);
    }
    for tool in ["Read", "Edit"] {
        for anchor in &anchors {
            for pattern in DENY_LIST {
                deny.push(format!(
                    "{tool}(/{}/{pattern})",
                    path_text(anchor).trim_end_matches('/')
                ));
            }
        }
    }
    let deny_write: Vec<String> = dirs.deny_write().iter().map(|dir| path_text(dir)).collect();
    let allow_write = allowed_writes(&settings.claude_code.sandbox_allow_write, dirs);
    let options = json!({
        "permissionMode": "default",
        "allowDangerouslySkipPermissions": false,
        "tools": tools,
        "strictMcpConfig": !mcp,
        "settings": {
            "permissions": {
                "ask": ask,
                "deny": deny,
            },
        },
        "sandbox": {
            "enabled": true,
            "failIfUnavailable": true,
            "autoAllowBashIfSandboxed": false,
            "allowUnsandboxedCommands": false,
            "filesystem": {
                "allowWrite": allow_write,
                "denyWrite": deny_write,
            },
        },
    });
    let mut meta = Meta::new();
    meta.insert("claudeCode".into(), json!({ "options": options }));
    meta
}

/// A path as the profile writes it.
#[cfg(not(windows))]
fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A path as the profile writes it: on Windows in the POSIX form Claude
/// Code matches its permission rules against, `C:\Users\me` as
/// `/c/Users/me` (a verbatim `\\?\C:` prefix the same way), so `//` anchors
/// a rule at the drive's root as it does at `/` elsewhere. A UNC share is
/// `//server/share`. Separators are `/` whichever way the path was joined.
#[cfg(windows)]
fn path_text(path: &Path) -> String {
    use std::path::{Component, Prefix};
    let mut text = String::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                    text.push('/');
                    text.push(char::from(drive.to_ascii_lowercase()));
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    text.push_str("//");
                    text.push_str(&server.to_string_lossy());
                    text.push('/');
                    text.push_str(&share.to_string_lossy());
                }
                Prefix::Verbatim(name) | Prefix::DeviceNS(name) => {
                    text.push_str("//");
                    text.push_str(&name.to_string_lossy());
                }
            },
            Component::RootDir => {}
            Component::CurDir => text.push_str("/."),
            Component::ParentDir => text.push_str("/.."),
            Component::Normal(name) => {
                text.push('/');
                text.push_str(&name.to_string_lossy());
            }
        }
    }
    if text.is_empty() {
        text.push('/');
    }
    text
}

/// Characters the sandbox's path syntax gives a meaning (glob patterns).
#[cfg(not(windows))]
const GLOB_SYNTAX: [char; 7] = ['*', '?', '[', ']', '{', '}', '\\'];
/// Characters the sandbox's path syntax gives a meaning (glob patterns).
/// On Windows `\` is the path separator, written as `/` in the profile
/// ([`path_text`]).
#[cfg(windows)]
const GLOB_SYNTAX: [char; 6] = ['*', '?', '[', ']', '{', '}'];

/// The `sandbox_allow_write` entries the profile passes, resolved: absolute
/// paths whose real location (`..` and symlinks resolved, as far as they
/// exist) is neither inside nor above the root, the worktree or the git
/// directories. The rest are dropped with a warning: a path reaching the
/// root would let commands write the user's tree outside review. A path
/// that can't be resolved (a dangling symlink, a loop, a component that
/// can't be read, or a path that isn't UTF-8) is dropped too.
fn allowed_writes(paths: &[String], dirs: &LaneDirs) -> Vec<String> {
    let mut forbidden: Vec<PathBuf> = [&dirs.root, &dirs.worktree]
        .into_iter()
        .map(|dir| resolve(dir).unwrap_or_else(|| dir.clone()))
        .collect();
    forbidden.extend(dirs.deny_write());
    paths
        .iter()
        .filter_map(|path| {
            // The sandbox reads glob syntax in these on macOS
            // (`/Users/me/proj*/**`), which a literal overlap check can't
            // bound, so none passes.
            let resolved = (!path.contains(GLOB_SYNTAX))
                .then(|| resolve(Path::new(path.as_str())))
                .flatten()
                .filter(|real| {
                    !forbidden
                        .iter()
                        .any(|dir| real.starts_with(dir) || dir.starts_with(real))
                })
                .and_then(|real| real.to_str().is_some().then(|| path_text(&real)))
                // A link can lead to a folder whose name has glob syntax,
                // which the sandbox would read as a pattern: what's passed is
                // checked, not only what was written.
                .filter(|real| !real.contains(GLOB_SYNTAX));
            if resolved.is_none() {
                log::warn!(
                    "fe-acp: agents.claude_code.sandbox_allow_write: {path} isn't an absolute path outside the project and its git directories, so it isn't passed to Claude Code"
                );
            }
            resolved
        })
        .collect()
}

/// Where the absolute `path` really is: resolved component by component,
/// as the kernel walks it. After each component, the path so far is made
/// canonical (symlinks followed) when it exists; a component that doesn't
/// exist is kept as written, and a `..` after it goes back lexically (it
/// can't be a link), to a place that may exist again and is resolved again
/// from there. `None` for a relative path, a dangling symlink, or a
/// component that can't be resolved.
fn resolve(path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    if !path.is_absolute() {
        return None;
    }
    let mut real = PathBuf::new();
    for component in path.components() {
        match component {
            // A drive or share alone isn't a place to canonicalize (`C:` is
            // that drive's current directory).
            Component::Prefix(_) => {
                real.push(component.as_os_str());
                continue;
            }
            Component::RootDir => real.push(component.as_os_str()),
            Component::CurDir => continue,
            Component::ParentDir => {
                real.pop();
            }
            Component::Normal(name) => real.push(name),
        }
        match dunce::canonicalize(&real) {
            Ok(canonical) => real = canonical,
            // Nothing there, not even a dangling link: kept as written.
            Err(err)
                if err.kind() == std::io::ErrorKind::NotFound
                    && real.symlink_metadata().is_err() => {}
            Err(_) => return None,
        }
    }
    Some(real)
}

/// An open session: its lane, its check, and the profile it was last
/// loaded with.
struct Session {
    dirs: LaneDirs,
    check: SettingsCheck,
    /// The profile the adapter is known to run the session under: the one
    /// its last answered `session/new`, `session/load` or `session/resume`
    /// carried. `None` while that's unknown: before the first answer, or
    /// after a load that failed or whose connection ended.
    applied: Option<Meta>,
    /// A load or resume was sent and hasn't been answered.
    reconfiguring: bool,
    /// Prompts and loads waiting for it.
    waiters: Vec<oneshot::Sender<()>>,
    refused: Option<Loosened>,
}

impl Session {
    /// A future that ends when the reconfiguration in flight is settled.
    fn wait(&mut self) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.waiters.push(tx);
        rx
    }
}

/// What a prompt does next, decided under the lock.
enum Step {
    Wait(oneshot::Receiver<()>),
    Reload(PathBuf, Meta),
    Failed(Error),
}

/// A load or resume in flight. Settled when its response is received, or,
/// if it never is, when it's dropped.
struct Reconfiguring {
    /// Weak, so a load that's never answered doesn't keep the agent (its
    /// client, connection and process) alive after every handle is gone.
    inner: Weak<Inner>,
    session_id: SessionId,
    meta: Option<Meta>,
}

impl Reconfiguring {
    fn new(agent: &ClaudeAgent, session_id: &SessionId, meta: Meta) -> Self {
        Self {
            inner: Arc::downgrade(&agent.inner),
            session_id: session_id.clone(),
            meta: Some(meta),
        }
    }

    /// The response callback that settles it.
    fn callback<T>(self) -> impl FnOnce(&Result<T, Error>) + Send + 'static {
        move |response| self.settle(response.is_ok())
    }

    fn settle(mut self, applied: bool) {
        self.finish(applied);
    }

    fn finish(&mut self, applied: bool) {
        let Some(meta) = self.meta.take() else {
            return;
        };
        // The agent is gone: nobody is left to wait on it.
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let mut sessions = lock(&inner.sessions);
        if let Some(session) = sessions.get_mut(&self.session_id) {
            session.reconfiguring = false;
            session.applied = applied.then_some(meta);
            for waiter in session.waiters.drain(..) {
                let _ = waiter.send(());
            }
        }
    }
}

impl Drop for Reconfiguring {
    fn drop(&mut self) {
        // Never answered: the profile the adapter runs is unknown.
        self.finish(false);
    }
}

/// Claude Agent over one adapter connection: the session methods with the
/// security profile and the inherited-settings check (§ 4's lifecycle).
///
/// Clones share the connection and the sessions.
#[derive(Clone)]
pub struct ClaudeAgent {
    inner: Arc<Inner>,
}

struct Inner {
    client: AcpClient,
    process: Option<AgentProcess>,
    environment: Environment,
    git: PathBuf,
    settings: Mutex<AgentSettings>,
    sessions: Mutex<HashMap<SessionId, Session>>,
}

impl std::fmt::Debug for ClaudeAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeAgent").finish_non_exhaustive()
    }
}

/// How long to wait for the adapter's exit, and the end of its stderr,
/// after its connection ends.
const EXIT_WAIT: Duration = Duration::from_secs(2);

impl ClaudeAgent {
    /// Claude Agent over `client`, with Claude Code's settings files found
    /// on `environment`, git run as `git`, and the profile built from
    /// `settings`. `process` is the adapter's process, when fe-acp launched
    /// it, for the stderr a "Disconnected" state shows.
    #[must_use]
    pub fn new(
        client: AcpClient,
        process: Option<AgentProcess>,
        environment: Environment,
        git: impl Into<PathBuf>,
        settings: AgentSettings,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                client,
                process,
                environment,
                git: git.into(),
                settings: Mutex::new(settings),
                sessions: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The client.
    #[must_use]
    pub fn client(&self) -> &AcpClient {
        &self.inner.client
    }

    /// Sends `initialize` and checks the adapter's version.
    ///
    /// # Errors
    ///
    /// The state for the failure: [`AgentState::AdapterTooOld`] below the
    /// floor.
    pub async fn initialize(&self) -> Result<InitializeResponse, AgentState> {
        let response = self.inner.client.initialize().await;
        let response = self.result(response).await?;
        check_version(&response)?;
        Ok(response)
    }

    /// The agent settings changed: the profile each session gets is rebuilt
    /// from `settings`, and a session whose profile changes is re-loaded
    /// before its next prompt.
    pub fn set_settings(&self, settings: AgentSettings) {
        // Under the sessions' lock, so a prompt being sent sees the change
        // or was sent before it.
        let _sessions = lock(&self.inner.sessions);
        *lock(&self.inner.settings) = settings;
    }

    /// "Not logged in" when the adapter's latest `_auth/status_update` said
    /// so.
    #[must_use]
    pub fn auth_state(&self) -> Option<AgentState> {
        (self.inner.client.auth_kind() == Some(AuthKind::None)).then_some(AgentState::NotLoggedIn)
    }

    /// The profile a session in `dirs` gets now.
    #[must_use]
    pub fn profile(&self, dirs: &LaneDirs) -> Meta {
        profile(&lock(&self.inner.settings), dirs)
    }

    /// Resolves `lane`'s git directories and checks the inherited settings
    /// for it.
    ///
    /// # Errors
    ///
    /// The state that refuses the session.
    pub async fn prepare(&self, lane: &Lane) -> Result<(LaneDirs, SettingsCheck), AgentState> {
        let dirs = LaneDirs::resolve(&self.inner.git, lane).await?;
        let check = SettingsCheck::new(
            self.inner.environment.clone(),
            &dirs.worktree,
            &dirs.root,
            dirs.main_checkout(),
        );
        check.check().map_err(AgentState::SandboxLoosened)?;
        Ok((dirs, check))
    }

    /// Starts a session in `lane`'s worktree with the whole profile
    /// (`session/new`).
    ///
    /// # Errors
    ///
    /// The state that refuses it.
    pub async fn new_session(&self, lane: &Lane) -> Result<NewSessionResponse, AgentState> {
        let (dirs, check) = self.prepare(lane).await?;
        let meta = self.profile(&dirs);
        let response = self
            .inner
            .client
            .new_session(&dirs.worktree, Some(meta.clone()))
            .await;
        let response = self.result(response).await?;
        lock(&self.inner.sessions).insert(
            response.session_id.clone(),
            Session {
                dirs,
                check,
                applied: Some(meta),
                reconfiguring: false,
                waiters: Vec::new(),
                refused: None,
            },
        );
        Ok(response)
    }

    /// Reopens a saved session in `lane`'s worktree with the whole current
    /// profile, built fresh (`session/load`): on reopening a chat, and
    /// after a crash or respawn. The adapter replays the history.
    ///
    /// # Errors
    ///
    /// The state that refuses it.
    pub async fn load_session(
        &self,
        session_id: impl Into<SessionId>,
        lane: &Lane,
    ) -> Result<LoadSessionResponse, AgentState> {
        let session_id = session_id.into();
        let (dirs, check) = self.prepare(lane).await?;
        let (worktree, meta, settle) = self.reconfigure(&session_id, dirs, check).await?;
        let started =
            (self.inner.client).start_load_session(session_id, worktree, Some(meta), settle);
        self.finish_reconfiguring(started).await
    }

    /// Resumes a saved session in `lane`'s worktree with the whole current
    /// profile (`session/resume`): no history is replayed.
    ///
    /// # Errors
    ///
    /// The state that refuses it.
    pub async fn resume_session(
        &self,
        session_id: impl Into<SessionId>,
        lane: &Lane,
    ) -> Result<ResumeSessionResponse, AgentState> {
        let session_id = session_id.into();
        let (dirs, check) = self.prepare(lane).await?;
        let (worktree, meta, settle) = self.reconfigure(&session_id, dirs, check).await?;
        let started =
            (self.inner.client).start_resume_session(session_id, worktree, Some(meta), settle);
        self.finish_reconfiguring(started).await
    }

    /// Opens the session in `dirs` (or takes it over, when it is open) for a
    /// load or resume: waits for any reconfiguration in flight, checks the
    /// inherited settings again after each wait (refusing as a prompt
    /// would), then marks one in flight with the current profile. Returns the worktree, the
    /// profile, and the callback that settles it when the response is
    /// received.
    async fn reconfigure<T>(
        &self,
        session_id: &SessionId,
        dirs: LaneDirs,
        check: SettingsCheck,
    ) -> Result<
        (
            PathBuf,
            Meta,
            impl FnOnce(&Result<T, Error>) + Send + 'static,
        ),
        AgentState,
    > {
        let connected = self.inner.client.connected().await;
        self.result(connected).await?;
        loop {
            let waiting = {
                let mut sessions = lock(&self.inner.sessions);
                let session = sessions
                    .entry(session_id.clone())
                    .or_insert_with(|| Session {
                        dirs: dirs.clone(),
                        check: check.clone(),
                        applied: None,
                        reconfiguring: false,
                        waiters: Vec::new(),
                        refused: None,
                    });
                if session.reconfiguring {
                    session.wait()
                } else {
                    // Checked again now, under the lock: a widening change
                    // made while this waited refuses it, as it would a
                    // prompt.
                    if let Err(loosened) = check.check() {
                        session.refused = Some(loosened.clone());
                        return Err(AgentState::SandboxLoosened(loosened));
                    }
                    session.dirs = dirs.clone();
                    session.check = check.clone();
                    session.reconfiguring = true;
                    let meta = self.profile(&session.dirs);
                    let worktree = session.dirs.worktree.clone();
                    drop(sessions);
                    let settle = Reconfiguring::new(self, session_id, meta.clone()).callback();
                    return Ok((worktree, meta, settle));
                }
            };
            let _ = waiting.await;
        }
    }

    async fn finish_reconfiguring<T>(
        &self,
        started: Result<impl Future<Output = Result<T, Error>>, Error>,
    ) -> Result<T, AgentState> {
        match started {
            Ok(response) => {
                let response = response.await;
                self.result(response).await
            }
            Err(error) => self.result(Err(error)).await,
        }
    }

    /// Sends a prompt (§ 4's "Prompt" stage): first the inherited-settings
    /// check; then, if the profile the adapter is known to run the session
    /// under isn't the current one, a `session/load` with the current one
    /// (its replay arrives on the stream); then the prompt, waiting for the
    /// turn to end.
    ///
    /// **Admission.** The check, the comparison with the applied profile,
    /// and sending the prompt happen together under the sessions' lock,
    /// which [`set_settings`](Self::set_settings) and the watcher callback
    /// ([`settings_changed`](Self::settings_changed)) also take. So a
    /// change either comes first, and the prompt is refused or re-loaded
    /// for, or comes after the prompt was sent, and the watcher cancels its
    /// turn.
    ///
    /// **Reconfiguration.** While a load or resume of the session is in
    /// flight, nothing is admitted: the prompt waits for its response and
    /// starts over. A load is settled when its response is received, on the
    /// connection's dispatch loop, whatever became of the caller that sent
    /// it, and only an answered load makes its profile the applied one. A
    /// failed load, or one whose connection ended, leaves none applied, so
    /// the next prompt loads again.
    ///
    /// # Errors
    ///
    /// The state that refused the prompt, or that the turn failed with.
    pub async fn prompt(
        &self,
        session_id: impl Into<SessionId>,
        prompt: Vec<ContentBlock>,
    ) -> Result<PromptResponse, AgentState> {
        let session_id = session_id.into();
        let connected = self.inner.client.connected().await;
        self.result(connected).await?;
        let response = loop {
            let step = {
                let mut sessions = lock(&self.inner.sessions);
                let session = sessions
                    .get_mut(&session_id)
                    .ok_or_else(|| AgentState::Failed {
                        details: format!("no open session {session_id}"),
                    })?;
                session.refused = session.check.check().err();
                if let Some(loosened) = &session.refused {
                    return Err(AgentState::SandboxLoosened(loosened.clone()));
                }
                if session.reconfiguring {
                    Step::Wait(session.wait())
                } else {
                    let meta = self.profile(&session.dirs);
                    if session.applied.as_ref() == Some(&meta) {
                        // Sent here, under the lock: the turn is registered
                        // before a watcher can look for it.
                        let sent =
                            (self.inner.client).start_prompt(session_id.clone(), prompt.clone());
                        match sent {
                            Ok(response) => break response,
                            Err(error) => Step::Failed(error),
                        }
                    } else {
                        session.reconfiguring = true;
                        Step::Reload(session.dirs.worktree.clone(), meta)
                    }
                }
            };
            match step {
                Step::Wait(waiting) => {
                    let _ = waiting.await;
                }
                Step::Failed(error) => return self.result(Err(error)).await,
                Step::Reload(worktree, meta) => {
                    let settle = Reconfiguring::new(self, &session_id, meta.clone()).callback();
                    let started = (self.inner.client).start_load_session(
                        session_id.clone(),
                        worktree,
                        Some(meta),
                        settle,
                    );
                    self.finish_reconfiguring(started).await?;
                }
            }
        };
        let response = response.await;
        self.result(response).await
    }

    /// Stops the session's running turn (`session/cancel`); its pending
    /// permission requests are answered `Cancelled`.
    ///
    /// # Errors
    ///
    /// When the notification can't be sent.
    pub async fn cancel(&self, session_id: impl Into<SessionId>) -> Result<(), Error> {
        self.inner.client.cancel(session_id).await
    }

    /// How many prompts, loads and resumes of the session wait for its
    /// reconfiguration in flight. For tests.
    #[doc(hidden)]
    #[must_use]
    pub fn queued(&self, session_id: &SessionId) -> usize {
        lock(&self.inner.sessions)
            .get(session_id)
            .map_or(0, |session| session.waiters.len())
    }

    /// Why the session refuses prompts now, if it does: the last check
    /// before a prompt or on a settings change failed.
    #[must_use]
    pub fn refused(&self, session_id: &SessionId) -> Option<Loosened> {
        lock(&self.inner.sessions)
            .get(session_id)
            .and_then(|session| session.refused.clone())
    }

    /// The sessions open, by id.
    #[must_use]
    pub fn sessions(&self) -> Vec<SessionId> {
        let mut ids: Vec<SessionId> = lock(&self.inner.sessions).keys().cloned().collect();
        ids.sort_by(|a, b| a.0.cmp(&b.0));
        ids
    }

    /// The paths a settings watcher must watch for every open session.
    #[must_use]
    pub fn watched(&self) -> Vec<PathBuf> {
        let mut watched: Vec<PathBuf> = Vec::new();
        for session in lock(&self.inner.sessions).values() {
            for path in session.check.watched() {
                if !watched.contains(&path) {
                    watched.push(path);
                }
            }
        }
        watched
    }

    /// Re-checks every open session under the sessions' lock: records which
    /// now refuse, and returns them with the ones that have a running turn
    /// to cancel. A prompt sent before this took the lock has its turn
    /// registered; one sent after sees the refusal.
    pub(crate) fn recheck_sessions(&self) -> (Vec<(SessionId, AgentState)>, Vec<SessionId>) {
        let mut sessions = lock(&self.inner.sessions);
        let mut refused = Vec::new();
        let mut cancel = Vec::new();
        for (session_id, session) in sessions.iter_mut() {
            session.refused = session.check.check().err();
            if let Some(loosened) = &session.refused {
                refused.push((
                    session_id.clone(),
                    AgentState::SandboxLoosened(loosened.clone()),
                ));
                // The protocol's own tracking: a turn whose caller dropped
                // its future still runs until its response arrives.
                if self.inner.client.turn_running(session_id) {
                    cancel.push(session_id.clone());
                }
            }
        }
        refused.sort_by(|a, b| a.0.0.cmp(&b.0.0));
        (refused, cancel)
    }

    /// Ends the sessions and the adapter (§ 4's "Shutdown"): `session/close`
    /// for each session, then stdin closed, and the process group killed
    /// once the adapter exits or after [`crate::launch::SHUTDOWN_GRACE`].
    pub async fn shutdown(&self) {
        let sessions = self.sessions();
        match &self.inner.process {
            Some(process) => process.shutdown(&self.inner.client, &sessions).await,
            None => {
                for session_id in sessions {
                    let _ = self.inner.client.close_session(session_id).await;
                }
                self.inner.client.shutdown();
            }
        }
        lock(&self.inner.sessions).clear();
    }

    /// `result`, its error as a state: a lost connection is
    /// [`AgentState::Disconnected`] with the adapter's last stderr lines,
    /// once it has exited.
    async fn result<T>(&self, result: Result<T, Error>) -> Result<T, AgentState> {
        let error = match result {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let state = AgentState::from_error(&error);
        let Some(process) = &self.inner.process else {
            return Err(state);
        };
        let exited = match state {
            AgentState::Disconnected { .. } => process.exited(EXIT_WAIT).await.is_some(),
            _ => process.try_exited().is_some(),
        };
        if exited || matches!(state, AgentState::Disconnected { .. }) {
            // A process still running keeps its stderr open: take what it
            // has written.
            if exited {
                process.stderr_finished(EXIT_WAIT).await;
            }
            return Err(AgentState::Disconnected {
                stderr: process.stderr_tail(),
            });
        }
        Err(state)
    }
}

/// Locks, ignoring poisoning: every update is a single field write.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
