//! The editor's settings, layered from defaults, the user file and the
//! project file.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use toml::{
    Table,
    de::{DeTable, DeValue},
};

use crate::{
    diagnostic::{Diagnostic, Severity},
    layers::{Layer, load_layer, merge},
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
}

/// Settings for vim mode.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct VimSettings {
    /// Whether the editor starts in vim's normal mode, where keys run vim
    /// commands rather than type. View > Toggle Vim Mode flips it and saves
    /// it here.
    pub enabled: bool,
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
    #[default]
    TypescriptLanguageServer,
    /// typescript-go's `tsc --lsp`.
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
    SCHEMA.clone()
}

static SCHEMA: LazyLock<serde_json::Value> =
    LazyLock::new(|| sorted(schemars::schema_for!(Settings).to_value()));

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

/// What a reload changed.
#[derive(Clone, Debug)]
pub struct SettingsEvent {
    /// The effective settings now.
    pub settings: Arc<Settings>,
    /// The effective settings before the reload.
    pub previous: Arc<Settings>,
    /// Every layer's diagnostics now.
    pub diagnostics: Vec<Diagnostic>,
}

/// The effective settings: built-in defaults, then the user file, then the
/// project file, merged key by key.
///
/// A file with an error (syntax, type or range) keeps its layer's last good
/// table, which is empty until the file has loaded cleanly once, so a typo
/// never resets a setting to its default. Warnings, such as unknown keys,
/// don't reject a file.
pub struct SettingsStore {
    /// The user layer, then the project layer.
    layers: Vec<LayerState>,
    settings: Arc<Settings>,
    diagnostics: Vec<Diagnostic>,
}

struct LayerState {
    path: PathBuf,
    /// The project's layer, whose `lsp` keys may only narrow.
    project: bool,
    /// The table last loaded without errors.
    good: Table,
    diagnostics: Vec<Diagnostic>,
}

impl LayerState {
    fn new(path: PathBuf, project: bool) -> Self {
        let mut layer = Self {
            path,
            project,
            good: Table::new(),
            diagnostics: Vec::new(),
        };
        layer.read();
        layer
    }

    /// Re-reads the file, keeping the last good table if it has errors.
    fn read(&mut self) {
        let (table, diagnostics) = validate(&self.path, self.project);
        if let Some(table) = table {
            self.good = table;
        }
        self.diagnostics = diagnostics;
    }
}

/// Loads the settings file at `path`: its table if it has no errors, and
/// its diagnostics. A `project` file's `lsp` keys are narrowed first, so a
/// bad value in a key that's ignored anyway rejects nothing.
fn validate(path: &Path, project: bool) -> (Option<Table>, Vec<Diagnostic>) {
    let mut layer = load_layer(path);
    if !layer.has_errors() {
        if project {
            narrow_lsp(&mut layer);
        }
        // Checked on the source, for the error's location, unless the
        // error is in a key narrowing dropped: then the narrowed table
        // decides.
        if let Err(error) = toml::from_str::<Settings>(&layer.source) {
            let diagnostic = layer.diagnostic_for(Severity::Error, &error);
            let dropped = project
                && diagnostic
                    .key
                    .as_deref()
                    .is_some_and(|key| key == "lsp" || key.starts_with("lsp."));
            if !dropped {
                layer.diagnostics.push(diagnostic);
            } else if let Err(error) =
                toml::Value::Table(layer.table.clone()).try_into::<Settings>()
            {
                let message = error.message().trim_end().to_owned();
                let diagnostic = layer.diagnostic_at(Severity::Error, None, 0, message);
                layer.diagnostics.push(diagnostic);
            }
        }
        if !layer.has_errors() {
            let more = layer.check_schema(&schema_for(&layer.table));
            layer.diagnostics.extend(more);
        }
    }
    let table = (!layer.has_errors()).then_some(layer.table);
    (table, layer.diagnostics)
}

/// [`SCHEMA`], with each server `table` names under `lsp.servers` listed
/// as a property, so its keys get checked like any other table's: a schema
/// map (`additionalProperties`) isn't descended into.
fn schema_for(table: &Table) -> std::borrow::Cow<'static, serde_json::Value> {
    let names: Vec<&String> = match table.get("lsp").and_then(|lsp| lsp.get("servers")) {
        Some(toml::Value::Table(servers)) => servers.keys().collect(),
        _ => Vec::new(),
    };
    if names.is_empty() {
        return std::borrow::Cow::Borrowed(&SCHEMA);
    }
    let mut schema = SCHEMA.clone();
    let server = serde_json::json!({ "$ref": "#/$defs/LspServerSettings" });
    let properties = names
        .into_iter()
        .map(|name| (name.clone(), server.clone()))
        .collect();
    schema["$defs"]["LspSettings"]["properties"]["servers"]["properties"] =
        serde_json::Value::Object(properties);
    std::borrow::Cow::Owned(schema)
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

impl SettingsStore {
    /// Loads the user and project settings files, either of which may be
    /// absent or missing.
    pub fn load(user: Option<PathBuf>, project: Option<PathBuf>) -> Self {
        let layers: Vec<_> = [(user, false), (project, true)]
            .into_iter()
            .filter_map(|(path, project)| Some(LayerState::new(path?, project)))
            .collect();
        let (settings, diagnostics) = effective(&layers);
        Self {
            layers,
            settings: Arc::new(settings),
            diagnostics,
        }
    }

    pub fn settings(&self) -> &Arc<Settings> {
        &self.settings
    }

    /// Every layer's diagnostics, the user file's first.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The settings files, for a [`crate::ConfigWatcher`].
    pub fn paths(&self) -> Vec<PathBuf> {
        self.layers.iter().map(|layer| layer.path.clone()).collect()
    }

    /// Re-reads the layers whose files are in `changed`. Returns what changed
    /// if the effective settings or the diagnostics did, and `None` if
    /// neither did.
    pub fn reload(&mut self, changed: &[PathBuf]) -> Option<SettingsEvent> {
        let mut read = false;
        for layer in &mut self.layers {
            if changed.contains(&layer.path) {
                layer.read();
                read = true;
            }
        }
        if !read {
            return None;
        }
        let (settings, diagnostics) = effective(&self.layers);
        if settings == *self.settings && diagnostics == self.diagnostics {
            return None;
        }
        let previous = std::mem::replace(&mut self.settings, Arc::new(settings));
        self.diagnostics = diagnostics;
        Some(SettingsEvent {
            settings: self.settings.clone(),
            previous,
            diagnostics: self.diagnostics.clone(),
        })
    }
}

/// The layers' good tables merged over the defaults, and their diagnostics.
fn effective(layers: &[LayerState]) -> (Settings, Vec<Diagnostic>) {
    let mut table = Table::try_from(Settings::default()).expect("the defaults serialize");
    for layer in layers {
        merge(&mut table, &layer.good);
    }
    // Each good table passed on its own, so their merge should too.
    let mut merged = Layer::default();
    merged.table = table;
    let errors: Vec<_> = merged
        .check_schema(&SCHEMA)
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect();
    let settings = if errors.is_empty() {
        toml::Value::Table(merged.table)
            .try_into()
            .map_err(|error| error.to_string())
    } else {
        Err(errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("; "))
    };
    let settings = settings.unwrap_or_else(|error| {
        log::error!("the merged settings are invalid, so the defaults apply: {error}");
        Settings::default()
    });
    let diagnostics = layers
        .iter()
        .flat_map(|layer| layer.diagnostics.iter().cloned())
        .collect();
    (settings, diagnostics)
}
