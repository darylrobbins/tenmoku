// SPDX-License-Identifier: LicenseRef-Proprietary

//! An app's settings, typed: its own struct, layered from its defaults, the
//! user's settings file and a project's, kept at its last good version
//! while a file has errors.

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value as Json;
use toml::Table;

use crate::{
    diagnostic::{Diagnostic, Severity},
    layers::{Layer, load_layer, merge},
};

/// What a settings type needs: written to a table for its defaults, read
/// back from the merged layers, and compared so that a reload that changes
/// nothing says so. Mark the struct (and each nested one) `#[serde(default)]`,
/// so a file may set only some of its keys.
pub trait SettingsType: Serialize + DeserializeOwned + Default + PartialEq + 'static {}

impl<S: Serialize + DeserializeOwned + Default + PartialEq + 'static> SettingsType for S {}

/// What a reload changed.
pub struct SettingsEvent<S> {
    /// The effective settings now.
    pub settings: Arc<S>,
    /// The effective settings before the reload.
    pub previous: Arc<S>,
    /// Every layer's diagnostics now.
    pub diagnostics: Vec<Diagnostic>,
}

impl<S> Clone for SettingsEvent<S> {
    fn clone(&self) -> Self {
        Self {
            settings: self.settings.clone(),
            previous: self.previous.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }
}

impl<S: fmt::Debug> fmt::Debug for SettingsEvent<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsEvent")
            .field("settings", &self.settings)
            .field("previous", &self.previous)
            .field("diagnostics", &self.diagnostics)
            .finish()
    }
}

/// The effective settings of type `S`: `S::default()`, then the user's
/// file, then the project's, merged key by key.
///
/// A file with an error (a syntax error, or a value `S` can't take, or one
/// the schema rejects) keeps its layer's last good table, which is empty
/// until the file has loaded cleanly once, so a typo never resets a setting
/// to its default. Warnings don't reject a file. Without a schema
/// ([`SettingsStore::with_schema`]), only serde checks a file, and keys `S`
/// doesn't have are ignored silently; with one, they're warnings, as are
/// ranges and lengths the schema sets (as schemars writes them).
pub struct SettingsStore<S> {
    /// The user layer, then the project layer, each if given.
    layers: Vec<LayerState>,
    schema: Option<Arc<Json>>,
    settings: Arc<S>,
    diagnostics: Vec<Diagnostic>,
}

struct LayerState {
    path: PathBuf,
    /// Whether it's the user's file, rather than a project's.
    user: bool,
    /// The table last loaded without errors.
    good: Table,
    diagnostics: Vec<Diagnostic>,
}

impl<S: SettingsType> SettingsStore<S> {
    /// Loads the user's and the project's settings files, either of which
    /// may be absent or missing, checked by serde alone.
    pub fn load(user: Option<PathBuf>, project: Option<PathBuf>) -> Self {
        Self::load_checked(user, project, None)
    }

    /// Loads them as [`SettingsStore::load`] does, also checking each file
    /// against `schema`, a JSON schema of `S` (`schemars::schema_for!(S)`).
    pub fn with_schema(user: Option<PathBuf>, project: Option<PathBuf>, schema: Json) -> Self {
        Self::load_checked(user, project, Some(Arc::new(schema)))
    }

    fn load_checked(
        user: Option<PathBuf>,
        project: Option<PathBuf>,
        schema: Option<Arc<Json>>,
    ) -> Self {
        let mut layers: Vec<_> = [(user, true), (project, false)]
            .into_iter()
            .filter_map(|(path, user)| {
                Some(LayerState {
                    path: path?,
                    user,
                    good: Table::new(),
                    diagnostics: Vec::new(),
                })
            })
            .collect();
        for layer in &mut layers {
            read::<S>(layer, schema.as_deref());
        }
        let (settings, diagnostics) = effective::<S>(&layers, schema.as_deref());
        Self {
            layers,
            schema,
            settings: Arc::new(settings),
            diagnostics,
        }
    }

    pub fn settings(&self) -> &Arc<S> {
        &self.settings
    }

    /// Every layer's diagnostics, the user file's first.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The settings files, for a [`crate::ConfigWatcher`]: the user's, then
    /// the project's.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.layers.iter().map(|layer| layer.path.clone()).collect()
    }

    /// The user's settings file, if it was given.
    pub fn user_path(&self) -> Option<&Path> {
        let user = self.layers.iter().find(|layer| layer.user)?;
        Some(&user.path)
    }

    /// Re-reads the layers whose files are in `changed`. Returns what
    /// changed if the effective settings or the diagnostics did, and `None`
    /// if neither did.
    pub fn reload(&mut self, changed: &[PathBuf]) -> Option<SettingsEvent<S>> {
        let mut read_any = false;
        for layer in &mut self.layers {
            if changed.contains(&layer.path) {
                read::<S>(layer, self.schema.as_deref());
                read_any = true;
            }
        }
        if !read_any {
            return None;
        }
        let (settings, diagnostics) = effective::<S>(&self.layers, self.schema.as_deref());
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

/// Re-reads `layer`'s file, keeping its last good table if it has errors.
fn read<S: SettingsType>(layer: &mut LayerState, schema: Option<&Json>) {
    let (table, diagnostics) = validate::<S>(&layer.path, schema);
    if let Some(table) = table {
        layer.good = table;
    }
    layer.diagnostics = diagnostics;
}

/// Loads the settings file at `path`: its table if it has no errors, and
/// its diagnostics.
fn validate<S: SettingsType>(
    path: &Path,
    schema: Option<&Json>,
) -> (Option<Table>, Vec<Diagnostic>) {
    let mut layer = load_layer(path);
    if !layer.has_errors() {
        // On the source, for the error's location.
        if let Err(error) = toml::from_str::<S>(&layer.source) {
            let diagnostic = layer.diagnostic_for(Severity::Error, &error);
            layer.diagnostics.push(diagnostic);
        } else if let Some(schema) = schema {
            let more = layer.check_schema(schema);
            layer.diagnostics.extend(more);
        }
    }
    let table = (!layer.has_errors()).then_some(layer.table);
    (table, layer.diagnostics)
}

/// The layers' good tables merged over the defaults, and their diagnostics.
fn effective<S: SettingsType>(
    layers: &[LayerState],
    schema: Option<&Json>,
) -> (S, Vec<Diagnostic>) {
    let defaults = Table::try_from(S::default());
    let mut table = defaults.unwrap_or_else(|error| {
        log::error!("the default settings don't serialize to a table: {error}");
        Table::new()
    });
    for layer in layers {
        merge(&mut table, &layer.good);
    }
    // Each good table passed on its own, so their merge should too.
    let mut merged = Layer::default();
    merged.table = table;
    let errors: Vec<_> = schema
        .map(|schema| merged.check_schema(schema))
        .unwrap_or_default()
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
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "))
    };
    let settings = settings.unwrap_or_else(|error| {
        log::error!("the merged settings are invalid, so the defaults apply: {error}");
        S::default()
    });
    let diagnostics = layers
        .iter()
        .flat_map(|layer| layer.diagnostics.iter().cloned())
        .collect();
    (settings, diagnostics)
}
