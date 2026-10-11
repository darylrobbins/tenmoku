//! The editor's settings, layered from defaults, the user file and the
//! project file, on kiln-core's typed [`kiln_core::SettingsStore`]: a
//! project file's `lsp` keys are narrowed and its `theme` dropped
//! ([`ProjectFilter`]), and each
//! file is checked against the schema with its servers listed.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use kiln_core::{ProjectFilter, SettingsOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use toml::{
    Table,
    de::{DeTable, DeValue},
};

use crate::{
    diagnostic::{Diagnostic, Severity},
    layers::Layer,
};

/// fastedit's settings, as written in `settings.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Settings {
    /// The text editor.
    pub editor: EditorSettings,
    /// Vim mode.
    #[serde(default)]
    pub vim: VimSettings,
    /// The status bar.
    #[serde(default)]
    pub status_bar: StatusBarSettings,
    /// Language servers. A project file may only turn them off.
    pub lsp: LspSettings,
    /// The colour theme.
    #[serde(default)]
    pub theme: ThemeSettings,
}

/// Settings for the colour theme. The theme is the app's, not a window's,
/// so a project file may not set them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ThemeSettings {
    /// dark, light, or system to follow the system's appearance. Your own
    /// colours go in theme.toml beside this file; its appearance says whether
    /// it replaces the dark or the light theme.
    pub mode: ThemeModeSetting,
}

/// `theme.mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ThemeModeSetting {
    /// The dark theme.
    #[default]
    Dark,
    /// The light theme.
    Light,
    /// The dark or light theme as the system's appearance is.
    System,
}

/// Settings for vim mode.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct VimSettings {
    /// Whether the editor starts in vim's normal mode, where keys run vim
    /// commands rather than type. View > Toggle Vim Mode flips it and saves
    /// it here.
    pub enabled: bool,
    /// Which register vim's unnamed register shares with the system
    /// clipboard: `"unnamedplus"` makes `yy` copy to it and `p` paste from
    /// it, as nvim's `clipboard=unnamedplus` does. Empty, only `"+` and `"*`
    /// reach it.
    pub clipboard: VimClipboard,
}

/// `vim.clipboard`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum VimClipboard {
    /// Only the `+` and `*` registers reach the system clipboard.
    #[default]
    #[serde(rename = "")]
    Off,
    /// Yanks, deletes and puts that name no register use the system clipboard.
    #[serde(rename = "unnamedplus")]
    Unnamedplus,
}

/// Settings for the status bar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct StatusBarSettings {
    /// Whether the status bar shows at the bottom of the window. View >
    /// Toggle Status Bar flips it and saves it here.
    pub visible: bool,
}

impl Default for StatusBarSettings {
    fn default() -> Self {
        Self { visible: true }
    }
}

/// Settings for the text editor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EditorSettings {
    /// The font family. Unset, the first installed of JetBrains Mono, SF Mono,
    /// Menlo, DejaVu Sans Mono, Ubuntu Mono, Liberation Mono and Noto Sans Mono.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub font_family: Option<String>,
    /// The font size, in points.
    #[schemars(range(min = 6.0, max = 72.0))]
    pub font_size: f64,
    /// The line height, as a multiple of the font size.
    #[schemars(range(min = 1.0, max = 3.0))]
    pub line_height: f64,
    /// How many columns a tab advances to the next multiple of.
    #[schemars(range(min = 1, max = 16))]
    pub tab_size: usize,
    /// Whether long lines wrap at the window's edge. `alt-z` toggles it per
    /// editor.
    pub soft_wrap: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 13.0,
            line_height: 1.6,
            tab_size: 4,
            soft_wrap: false,
        }
    }
}

/// Settings for language servers (docs/decisions/lsp-client.md §3.3).
///
/// A server's spec is executable configuration, so a project's settings
/// file may only turn servers off: `lsp.enabled = false` and
/// `lsp.servers.<name>.enabled = false` apply from it, and every other
/// `lsp` key there is ignored with a warning, until workspace trust exists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LspSettings {
    /// Whether language servers run at all.
    pub enabled: bool,
    /// The largest file, in bytes, synced to a language server. Applies to
    /// files opened after it changes.
    #[schemars(range(min = 0))]
    pub max_file_bytes: u64,
    /// Which built-in server TypeScript and JavaScript files get.
    pub typescript_server: TypescriptServer,
    /// How long requests may take, in milliseconds, by method.
    pub timeouts: LspTimeouts,
    /// Servers by name: changes to the built-in rust-analyzer, pyright,
    /// typescript-language-server and typescript-go, or servers of your
    /// own, which need a `command`.
    pub servers: BTreeMap<String, LspServerSettings>,
}

impl Default for LspSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_file_bytes: 50 * 1024 * 1024,
            typescript_server: TypescriptServer::default(),
            timeouts: LspTimeouts::default(),
            servers: BTreeMap::new(),
        }
    }
}

/// The built-in server for TypeScript and JavaScript.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TypescriptServer {
    /// typescript-language-server, over tsserver.
    TypescriptLanguageServer,
    /// typescript-go's `tsc --lsp`.
    #[default]
    TypescriptGo,
}

/// Request timeouts, in milliseconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LspTimeouts {
    /// Go to definition.
    #[schemars(range(min = 1))]
    pub definition: u64,
    /// Workspace symbols.
    #[schemars(range(min = 1))]
    pub workspace_symbol: u64,
    /// Find references.
    #[schemars(range(min = 1))]
    pub references: u64,
    #[schemars(range(min = 1))]
    pub hover: u64,
    #[schemars(range(min = 1))]
    pub completion: u64,
    /// Resolving a completion item.
    #[schemars(range(min = 1))]
    pub resolve: u64,
}

impl Default for LspTimeouts {
    fn default() -> Self {
        Self {
            definition: 5000,
            workspace_symbol: 5000,
            references: 30000,
            hover: 3000,
            completion: 3000,
            resolve: 2000,
        }
    }
}

/// One server's settings. Each key left unset keeps the built-in value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LspServerSettings {
    /// Whether the server runs.
    pub enabled: bool,
    /// The program: a bare name is looked up on the login shell's `PATH`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Set on top of fastedit's environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    /// The languages it serves, by fastedit's names: rust, python,
    /// javascript, typescript, tsx and so on. A file gets the first enabled
    /// server listing its language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<String>>,
    /// How fastedit tells the server is ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness: Option<LspReadiness>,
    /// Work-done progress token prefixes that don't hold back readiness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_ignores_progress: Option<Vec<String>>,
    /// Sent with `initialize`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialization_options: Option<serde_json::Value>,
    /// Answers the server's `workspace/configuration` requests, by section:
    /// `[lsp.servers.rust-analyzer.settings.rust-analyzer]` is the
    /// `rust-analyzer` section. Changing it doesn't restart the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Default for LspServerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            command: None,
            args: None,
            env: None,
            languages: None,
            readiness: None,
            ready_ignores_progress: None,
            initialization_options: None,
            settings: None,
        }
    }
}

/// How a server's readiness is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LspReadiness {
    /// rust-analyzer's `experimental/serverStatus`.
    ServerStatus,
    /// No work-done progress open.
    Progress,
}

/// The JSON schema of `settings.toml`, which `settings.schema.json` holds,
/// with its keys sorted.
pub fn settings_schema() -> serde_json::Value {
    serde_json::Value::clone(&SCHEMA)
}

static SCHEMA: LazyLock<Arc<serde_json::Value>> =
    LazyLock::new(|| Arc::new(sorted(schemars::schema_for!(Settings).to_value())));

/// `value` with every object's keys in order, whether or not another crate
/// turns on serde_json's `preserve_order`.
fn sorted(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut entries: Vec<_> = object.into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            entries
                .into_iter()
                .map(|(key, value)| (key, sorted(value)))
                .collect()
        }
        serde_json::Value::Array(values) => values.into_iter().map(sorted).collect(),
        value => value,
    }
}

/// What a reload changed: kiln's, for fastedit's [`Settings`].
pub type SettingsEvent = kiln_core::SettingsEvent<Settings>;

/// The effective settings: built-in defaults, then the user file, then the
/// project file, merged key by key.
///
/// A file with an error (syntax, type or range) keeps its layer's last good
/// table, which is empty until the file has loaded cleanly once, so a typo
/// never resets a setting to its default. Warnings, such as unknown keys,
/// don't reject a file. A project file's `lsp` keys may only turn servers
/// off ([`LspSettings`]).
///
/// kiln-core's typed store for [`Settings`], with fastedit's checks:
/// [`SettingsStore::into_inner`] gives it to kiln's config service.
pub struct SettingsStore(kiln_core::SettingsStore<Settings>);

impl SettingsStore {
    /// Loads the user and project settings files, either of which may be
    /// absent or missing.
    pub fn load(user: Option<PathBuf>, project: Option<PathBuf>) -> Self {
        Self(kiln_core::SettingsStore::load_with(
            user,
            project,
            options(),
        ))
    }

    pub fn settings(&self) -> &Arc<Settings> {
        self.0.settings()
    }

    /// Every layer's diagnostics, the user file's first.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.0.diagnostics()
    }

    /// The settings files, for a [`crate::ConfigWatcher`].
    pub fn paths(&self) -> Vec<PathBuf> {
        self.0.paths()
    }

    /// Re-reads the layers whose files are in `changed`. Returns what changed
    /// if the effective settings or the diagnostics did, and `None` if
    /// neither did.
    pub fn reload(&mut self, changed: &[PathBuf]) -> Option<SettingsEvent> {
        self.0.reload(changed)
    }

    /// kiln-core's store underneath, for kiln's config service.
    pub fn into_inner(self) -> kiln_core::SettingsStore<Settings> {
        self.0
    }
}

/// How fastedit's settings files are checked: against [`SCHEMA`] with each
/// server listed ([`schema_for`]), a project's `lsp` keys narrowed
/// ([`narrow_lsp`]) and its `theme` dropped ([`drop_theme`]).
fn options() -> SettingsOptions {
    SettingsOptions::new()
        .schema_for(schema_for)
        .project_filter(ProjectFilter::new("lsp", narrow_lsp))
        .project_filter(ProjectFilter::new("theme", drop_theme))
}

/// [`SCHEMA`], with each server `table` names under `lsp.servers` listed
/// as a property, so its keys get checked like any other table's: a schema
/// map (`additionalProperties`) isn't descended into.
fn schema_for(table: &Table) -> Arc<serde_json::Value> {
    let names: Vec<&String> = match table.get("lsp").and_then(|lsp| lsp.get("servers")) {
        Some(toml::Value::Table(servers)) => servers.keys().collect(),
        _ => Vec::new(),
    };
    if names.is_empty() {
        return SCHEMA.clone();
    }
    let mut schema = serde_json::Value::clone(&SCHEMA);
    let server = serde_json::json!({ "$ref": "#/$defs/LspServerSettings" });
    let properties = names
        .into_iter()
        .map(|name| (name.clone(), server.clone()))
        .collect();
    schema["$defs"]["LspSettings"]["properties"]["servers"]["properties"] =
        serde_json::Value::Object(properties);
    Arc::new(schema)
}

/// The warning for each `lsp` key a project file sets that it may not.
pub const UNTRUSTED_LSP: &str =
    "LSP settings in project files are ignored until workspace trust exists";

/// Drops every `lsp` key of a project layer's table except
/// `lsp.enabled = false` and `lsp.servers.<name>.enabled = false`, with
/// an [`UNTRUSTED_LSP`] warning at each, whatever its value.
fn narrow_lsp(layer: &mut Layer) {
    let off = toml::Value::Boolean(false);
    let mut kept = Table::new();
    let mut dropped: Vec<Vec<String>> = Vec::new();
    let lsp = match layer.table.remove("lsp") {
        None => return,
        Some(toml::Value::Table(lsp)) => lsp,
        Some(_) => {
            dropped.push(Vec::new());
            Table::new()
        }
    };
    for (key, value) in lsp {
        match (key.as_str(), value) {
            ("enabled", value) if value == off => {
                kept.insert(key, value);
            }
            ("servers", toml::Value::Table(servers)) => {
                let mut kept_servers = Table::new();
                for (name, server) in servers {
                    let toml::Value::Table(server) = server else {
                        dropped.push(vec![key.clone(), name]);
                        continue;
                    };
                    let mut kept_server = Table::new();
                    for (field, value) in server {
                        if field == "enabled" && value == off {
                            kept_server.insert(field, value);
                        } else {
                            dropped.push(vec![key.clone(), name.clone(), field]);
                        }
                    }
                    if !kept_server.is_empty() {
                        kept_servers.insert(name, toml::Value::Table(kept_server));
                    }
                }
                if !kept_servers.is_empty() {
                    kept.insert(key, toml::Value::Table(kept_servers));
                }
            }
            _ => dropped.push(vec![key]),
        }
    }
    if !kept.is_empty() {
        layer.table.insert("lsp".into(), toml::Value::Table(kept));
    }
    let document = DeTable::parse(&layer.source).ok();
    for path in dropped {
        let offset = document
            .as_ref()
            .map_or(0, |document| key_offset(document.get_ref(), "lsp", &path));
        let key = std::iter::once("lsp")
            .chain(path.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(".");
        let warning =
            layer.diagnostic_at(Severity::Warning, Some(key), offset, UNTRUSTED_LSP.into());
        layer.diagnostics.push(warning);
    }
}

/// The warning for each `theme` key a project file sets.
pub const PROJECT_THEME: &str =
    "theme settings in project files are ignored: the theme is the same in every window";

/// Drops a project layer's `theme` table, with a [`PROJECT_THEME`] warning
/// at each key it set (at `theme` itself if it isn't a table).
fn drop_theme(layer: &mut Layer) {
    let dropped: Vec<Vec<String>> = match layer.table.remove("theme") {
        None => return,
        Some(toml::Value::Table(theme)) => theme.into_iter().map(|(key, _)| vec![key]).collect(),
        Some(_) => vec![Vec::new()],
    };
    let document = DeTable::parse(&layer.source).ok();
    for path in dropped {
        let offset = document
            .as_ref()
            .map_or(0, |document| key_offset(document.get_ref(), "theme", &path));
        let key = std::iter::once("theme")
            .chain(path.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(".");
        let warning =
            layer.diagnostic_at(Severity::Warning, Some(key), offset, PROJECT_THEME.into());
        layer.diagnostics.push(warning);
    }
}

/// Where the key at `first` then `rest` is in the source, or where the
/// deepest of its tables found is.
fn key_offset(table: &DeTable<'_>, first: &str, rest: &[String]) -> usize {
    let mut table = table;
    let mut offset = 0;
    for key in std::iter::once(first).chain(rest.iter().map(String::as_str)) {
        let Some((found, value)) = table.iter().find(|(name, _)| name.get_ref() == key) else {
            break;
        };
        offset = found.span().start;
        match value.get_ref() {
            DeValue::Table(inner) => table = inner,
            _ => break,
        }
    }
    offset
}
