//! The `[agents]` settings (docs/decisions/50t8wt9we2-ai-agents.md §7): the
//! tool loop's limits, the permission levels and command lists, and the
//! Claude Code adapter's programs. A project file can only tighten them
//! ([`narrow_agents`]).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use toml::de::DeTable;

use crate::{
    diagnostic::Severity,
    layers::Layer,
    settings::{key_offset, project_warning},
};

/// Settings for AI agents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AgentSettings {
    /// The API chat offers Claude tools it runs for you, in trusted folders
    /// only.
    pub tool_loop: bool,
    /// The most requests one turn makes.
    #[schemars(range(min = 1, max = 200))]
    pub max_rounds: u32,
    /// The most tokens a turn's task may use, 0 for no budget, else at
    /// least 20000.
    #[schemars(extend("anyOf" = [{"const": 0}, {"minimum": 20000}]))]
    #[serde(deserialize_with = "budget")]
    pub task_budget: u32,
    /// How long one turn may run, in seconds.
    #[schemars(range(min = 30, max = 7200))]
    pub turn_time_limit_secs: u32,
    /// How long one command an agent runs may take, in seconds.
    #[schemars(range(min = 1, max = 3600))]
    pub command_timeout_secs: u32,
    /// What an agent may do without asking, by kind of action.
    pub permissions: AgentPermissions,
    /// Permissions a project file asked for: each can only tighten what
    /// `permissions` allows, so the effective level is the stricter of the
    /// two. A project file's `agents.permissions` is moved here.
    pub project_permissions: ProjectPermissions,
    /// The Claude Code agent. A project file can't set any of it.
    pub claude_code: ClaudeCodeSettings,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            tool_loop: true,
            max_rounds: 25,
            task_budget: 0,
            turn_time_limit_secs: 600,
            command_timeout_secs: 120,
            permissions: AgentPermissions::default(),
            project_permissions: ProjectPermissions::default(),
            claude_code: ClaudeCodeSettings::default(),
        }
    }
}

impl AgentSettings {
    /// The permissions in effect: per class the stricter of the user's level
    /// and the project's, and each command list the union of both. A
    /// project can't add to `commands_allow`.
    pub fn effective_permissions(&self) -> AgentPermissions {
        let user = &self.permissions;
        let project = &self.project_permissions;
        let level = |user: AgentLevel, project: Option<AgentLevel>| {
            project.map_or(user, |project| user.max(project))
        };
        let union = |a: &[String], b: &[String]| {
            let mut all = a.to_vec();
            for item in b {
                if !all.contains(item) {
                    all.push(item.clone());
                }
            }
            all
        };
        AgentPermissions {
            read: level(user.read, project.read),
            search: level(user.search, project.search),
            propose_edit: level(user.propose_edit, project.propose_edit),
            run_command: level(user.run_command, project.run_command),
            fetch: level(user.fetch, project.fetch),
            other: level(user.other, project.other),
            commands_allow: user.commands_allow.clone(),
            commands_ask: union(&user.commands_ask, &project.commands_ask),
            commands_deny: union(&user.commands_deny, &project.commands_deny),
        }
    }
}

/// What happens when an agent wants to do something of a kind. Later is
/// stricter.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentLevel {
    /// Do it without asking.
    Allow,
    /// Ask first.
    Ask,
    /// Never.
    Deny,
}

impl AgentLevel {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "allow" => Some(Self::Allow),
            "ask" => Some(Self::Ask),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// `agents.permissions`: a level for each kind of action, and command
/// patterns that override a level for `run_command`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AgentPermissions {
    /// Reading files in the folder.
    pub read: AgentLevel,
    /// Searching the folder.
    pub search: AgentLevel,
    /// Proposing an edit, which you review before it applies.
    pub propose_edit: AgentLevel,
    /// Running a command.
    pub run_command: AgentLevel,
    /// Fetching from the network.
    pub fetch: AgentLevel,
    /// Any other action.
    pub other: AgentLevel,
    /// Command patterns that run without asking.
    pub commands_allow: Vec<String>,
    /// Command patterns that always ask.
    pub commands_ask: Vec<String>,
    /// Command patterns that never run.
    pub commands_deny: Vec<String>,
}

impl Default for AgentPermissions {
    fn default() -> Self {
        Self {
            read: AgentLevel::Allow,
            search: AgentLevel::Allow,
            propose_edit: AgentLevel::Allow,
            run_command: AgentLevel::Ask,
            fetch: AgentLevel::Ask,
            other: AgentLevel::Ask,
            commands_allow: Vec::new(),
            commands_ask: Vec::new(),
            commands_deny: Vec::new(),
        }
    }
}

/// `agents.project_permissions`: what a project file asks for. A level here
/// only takes effect if it is stricter than the user's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ProjectPermissions {
    /// Reading files in the folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<AgentLevel>,
    /// Searching the folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<AgentLevel>,
    /// Proposing an edit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub propose_edit: Option<AgentLevel>,
    /// Running a command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_command: Option<AgentLevel>,
    /// Fetching from the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch: Option<AgentLevel>,
    /// Any other action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<AgentLevel>,
    /// Command patterns that always ask, added to yours.
    pub commands_ask: Vec<String>,
    /// Command patterns that never run, added to yours.
    pub commands_deny: Vec<String>,
}

/// `agents.claude_code`: the programs Tenmoku runs for the Claude Code agent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ClaudeCodeSettings {
    /// The path of the claude-agent-acp adapter. Empty finds it on your
    /// login shell's PATH.
    pub adapter: String,
    /// The path of the claude program. Empty finds it on your login shell's
    /// PATH.
    pub claude: String,
    /// Extra paths Claude Code's sandbox may write.
    pub sandbox_allow_write: Vec<String>,
    /// Let Claude Code run the MCP servers its own settings configure.
    pub mcp_servers: bool,
}

/// The warning for what a project file sets under `agents` that it may not,
/// in en-US.
pub const PROJECT_AGENTS: &str = kiln_l10n::verbatim!(
    "A project's settings can only tighten `agents.permissions` (levels, commands_ask, commands_deny); everything else under `agents` is ignored"
);

const LEVEL_KEYS: [&str; 6] = [
    "read",
    "search",
    "propose_edit",
    "run_command",
    "fetch",
    "other",
];

/// Narrows a project layer's `agents` table: keeps `permissions`' class
/// levels, `commands_ask` and `commands_deny`, moved to
/// `agents.project_permissions` (layers merge last-wins, so under
/// `permissions` they could loosen the user's), and drops the rest with a
/// [`PROJECT_AGENTS`] warning at each key: `commands_allow`, the limits,
/// `tool_loop` and all of `claude_code`.
pub(crate) fn narrow_agents(layer: &mut Layer) {
    let mut kept = toml::Table::new();
    let mut dropped: Vec<Vec<String>> = Vec::new();
    let agents = match layer.table.remove("agents") {
        None => return,
        Some(toml::Value::Table(agents)) => agents,
        Some(_) => {
            dropped.push(Vec::new());
            toml::Table::new()
        }
    };
    for (key, value) in agents {
        match (key.as_str(), value) {
            ("permissions", toml::Value::Table(permissions)) => {
                for (name, value) in permissions {
                    let ok = match (name.as_str(), &value) {
                        (name, toml::Value::String(level)) if LEVEL_KEYS.contains(&name) => {
                            AgentLevel::parse(level).is_some()
                        }
                        ("commands_ask" | "commands_deny", toml::Value::Array(items)) => {
                            items.iter().all(toml::Value::is_str)
                        }
                        _ => false,
                    };
                    if ok {
                        kept.insert(name, value);
                    } else {
                        dropped.push(vec![key.clone(), name]);
                    }
                }
            }
            // (`project_permissions` is gone: `drop_project_permissions`
            // removed it from every file before this runs.)
            (_, toml::Value::Table(table)) => {
                dropped.extend(table.into_iter().map(|(name, _)| vec![key.clone(), name]));
            }
            _ => dropped.push(vec![key]),
        }
    }
    if !kept.is_empty() {
        let mut agents = toml::Table::new();
        agents.insert("project_permissions".into(), toml::Value::Table(kept));
        layer
            .table
            .insert("agents".into(), toml::Value::Table(agents));
    }
    let document = DeTable::parse(&layer.source).ok();
    for path in dropped {
        let offset = document.as_ref().map_or(0, |document| {
            key_offset(document.get_ref(), "agents", &path)
        });
        let key = std::iter::once("agents")
            .chain(path.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(".");
        let warning = layer.diagnostic_at(
            Severity::Warning,
            Some(key),
            offset,
            project_warning(PROJECT_AGENTS),
        );
        layer.diagnostics.push(warning);
    }
}

/// `agents.task_budget`: 0, or at least 20000.
fn budget<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let tokens = u32::deserialize(deserializer)?;
    if tokens != 0 && tokens < 20_000 {
        return Err(serde::de::Error::custom(
            kiln_l10n::tr!("config-agents-task-budget").to_string(),
        ));
    }
    Ok(tokens)
}

/// The warning for an `agents.project_permissions` set in any file, in
/// en-US.
pub const PROJECT_PERMISSIONS_SET: &str = kiln_l10n::verbatim!(
    "agents.project_permissions can't be set in a file: a project's `agents.permissions` fill it, and you tighten through `agents.permissions`"
);

/// Drops `agents.project_permissions` from any layer, with a
/// [`PROJECT_PERMISSIONS_SET`] warning: only [`narrow_agents`] fills it, from
/// a project's own `agents.permissions`, so a file's value can't loosen or
/// erase what another layer set.
pub(crate) fn drop_project_permissions(layer: &mut Layer) {
    let Some(toml::Value::Table(agents)) = layer.table.get_mut("agents") else {
        return;
    };
    if agents.remove("project_permissions").is_none() {
        return;
    }
    let offset = DeTable::parse(&layer.source).ok().map_or(0, |document| {
        key_offset(
            document.get_ref(),
            "agents",
            &["project_permissions".into()],
        )
    });
    let warning = layer.diagnostic_at(
        Severity::Warning,
        Some("agents.project_permissions".into()),
        offset,
        project_warning(PROJECT_PERMISSIONS_SET),
    );
    layer.diagnostics.push(warning);
}
