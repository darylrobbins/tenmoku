//! The `[debug]` settings (docs/decisions/r1b9dw3g0g-debugger.md §3, card
//! D3): the user's debug adapters, the timeouts a session's requests get,
//! and whether the protocol log records payloads. A project file can only
//! set the timeouts ([`narrow_debug`]): a folder can't choose what runs, or
//! turn on a log that may hold secrets.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use toml::de::DeTable;

use crate::{
    diagnostic::Severity,
    layers::Layer,
    settings::{key_offset, project_warning},
};

/// Settings for the debugger.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DebugSettings {
    /// Debug adapters by name: changes to the built-in lldb-dap, codelldb,
    /// debugpy and delve, or adapters of your own, which need a `command`.
    /// A project file can't set them.
    pub adapters: BTreeMap<String, DebugAdapterSettings>,
    /// How long a debug adapter may take, in milliseconds. Applies to
    /// sessions started after it changes.
    pub timeouts: DebugTimeouts,
    /// Record whole protocol messages in the debug log, not only what kind
    /// they are. They may hold secrets, such as a password in a variable. A
    /// project file can't set it.
    pub log_payloads: bool,
}

/// One adapter's settings. Each key left unset keeps the built-in value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DebugAdapterSettings {
    /// The program: a bare name is looked up on the login shell's `PATH`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Set on top of Tenmoku's environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    /// Folders a bare `command` is looked up in before the login shell's
    /// `PATH`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
    /// How Tenmoku talks to the adapter. With tcp, an argument holds
    /// `${port}` for the port it listens on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<DebugTransport>,
    /// The languages it debugs, by Tenmoku's names: rust, python, go and so
    /// on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<String>>,
    /// The `type` strings of launch.json configurations it answers to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<String>>,
}

/// How Tenmoku talks to an adapter of the user's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DebugTransport {
    /// Over its standard input and output.
    Stdio,
    /// Over a connection to 127.0.0.1, on the port Tenmoku puts in its
    /// arguments for `${port}`.
    Tcp,
}

/// A debug session's timeouts, in milliseconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DebugTimeouts {
    /// A request's answer, such as a step's or a variable's.
    #[schemars(range(min = 1))]
    pub request: u64,
    /// Starting the program: the answer to launch or attach.
    #[schemars(range(min = 1))]
    pub launch: u64,
    /// Connecting to an adapter that listens on a port.
    #[schemars(range(min = 1))]
    pub connect: u64,
}

impl Default for DebugTimeouts {
    fn default() -> Self {
        Self {
            request: 10_000,
            launch: 30_000,
            connect: 5_000,
        }
    }
}

/// The warning for what a project file sets under `debug` that it may not,
/// in en-US.
pub const PROJECT_DEBUG: &str = kiln_l10n::verbatim!(
    "A project's settings can only set `debug.timeouts`: a folder can't choose a debug adapter's command, arguments or environment, or turn on debug.log_payloads"
);

/// Narrows a project layer's `debug` table, as `narrow_lsp` narrows `lsp`:
/// keeps `timeouts` and drops the rest with a [`PROJECT_DEBUG`] warning at
/// each key: every field of every `adapters.<name>` (the `command`,
/// `args`, `env` and `path` that choose what runs, and the rest with
/// them), `log_payloads`, and anything else.
pub(crate) fn narrow_debug(layer: &mut Layer) {
    let mut kept = toml::Table::new();
    let mut dropped: Vec<Vec<String>> = Vec::new();
    let debug = match layer.table.remove("debug") {
        None => return,
        Some(toml::Value::Table(debug)) => debug,
        Some(_) => {
            dropped.push(Vec::new());
            toml::Table::new()
        }
    };
    for (key, value) in debug {
        match (key.as_str(), value) {
            ("timeouts", value) => {
                kept.insert(key, value);
            }
            ("adapters", toml::Value::Table(adapters)) => {
                for (name, adapter) in adapters {
                    match adapter {
                        toml::Value::Table(fields) if !fields.is_empty() => {
                            dropped.extend(
                                fields
                                    .into_iter()
                                    .map(|(field, _)| vec![key.clone(), name.clone(), field]),
                            );
                        }
                        _ => dropped.push(vec![key.clone(), name]),
                    }
                }
            }
            _ => dropped.push(vec![key]),
        }
    }
    if !kept.is_empty() {
        layer.table.insert("debug".into(), toml::Value::Table(kept));
    }
    let document = DeTable::parse(&layer.source).ok();
    for path in dropped {
        let offset = document
            .as_ref()
            .map_or(0, |document| key_offset(document.get_ref(), "debug", &path));
        let key = std::iter::once("debug")
            .chain(path.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(".");
        let warning = layer.diagnostic_at(
            Severity::Warning,
            Some(key),
            offset,
            project_warning(PROJECT_DEBUG),
        );
        layer.diagnostics.push(warning);
    }
}
