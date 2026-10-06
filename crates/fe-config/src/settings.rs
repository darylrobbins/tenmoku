//! The editor's settings, layered from defaults, the user file and the
//! project file.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use toml::Table;

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
}

/// Settings for the text editor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EditorSettings {
    /// The font family. Unset, the first installed of JetBrains Mono, SF Mono
    /// and Menlo.
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
    /// The table last loaded without errors.
    good: Table,
    diagnostics: Vec<Diagnostic>,
}

impl LayerState {
    fn new(path: PathBuf) -> Self {
        let mut layer = Self {
            path,
            good: Table::new(),
            diagnostics: Vec::new(),
        };
        layer.read();
        layer
    }

    /// Re-reads the file, keeping the last good table if it has errors.
    fn read(&mut self) {
        let (table, diagnostics) = validate(&self.path);
        if let Some(table) = table {
            self.good = table;
        }
        self.diagnostics = diagnostics;
    }
}

/// Loads the settings file at `path`: its table if it has no errors, and
/// its diagnostics.
fn validate(path: &Path) -> (Option<Table>, Vec<Diagnostic>) {
    let mut layer = load_layer(path);
    if !layer.has_errors() {
        match toml::from_str::<Settings>(&layer.source) {
            Ok(_) => {
                let more = layer.check_schema(&SCHEMA);
                layer.diagnostics.extend(more);
            }
            Err(error) => {
                let diagnostic = layer.diagnostic_for(Severity::Error, &error);
                layer.diagnostics.push(diagnostic);
            }
        }
    }
    let table = (!layer.has_errors()).then_some(layer.table);
    (table, layer.diagnostics)
}

impl SettingsStore {
    /// Loads the user and project settings files, either of which may be
    /// absent or missing.
    pub fn load(user: Option<PathBuf>, project: Option<PathBuf>) -> Self {
        let layers: Vec<_> = [user, project]
            .into_iter()
            .flatten()
            .map(LayerState::new)
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
