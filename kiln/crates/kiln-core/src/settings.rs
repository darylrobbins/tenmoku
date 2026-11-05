// SPDX-License-Identifier: Apache-2.0

//! An app's settings, typed: its own struct, layered from its defaults, the
//! user's settings file and a project's, kept at its last good version
//! while a file has errors. A project's file may be narrowed to what the
//! app lets a project set ([`ProjectFilter`]). Several types may share one
//! file, each its own table of it ([`SettingsOptions::section`]).

use std::{
    fmt,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{
    Deserializer, Serialize,
    de::{DeserializeOwned, DeserializeSeed, IgnoredAny, MapAccess, Visitor},
};
use serde_json::{Map, Value as Json, json};
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

/// What a project's settings file may set, narrowed by the app: a
/// project file comes with the folder, so an app may not trust it with
/// everything the user's file sets, such as programs to run.
///
/// The filter governs one key (`lsp`, or a dotted one such as
/// `tools.servers`). `narrow` is given each project file's [`Layer`], once
/// it has parsed, and drops from its table whatever under that key the
/// project may not set, adding a diagnostic for each (usually a warning,
/// at the key, with [`Layer::diagnostic_at`]). An error serde finds in the
/// file under that key is then checked on the narrowed table instead,
/// since the key it's in may be gone: if the narrowed table is good, the
/// file is; if not, the error is reported at the start of the file. The
/// user's file is never narrowed.
#[derive(Clone)]
pub struct ProjectFilter {
    key: String,
    narrow: Arc<dyn Fn(&mut Layer) + Send + Sync>,
}

impl ProjectFilter {
    pub fn new(
        key: impl Into<String>,
        narrow: impl Fn(&mut Layer) + Send + Sync + 'static,
    ) -> Self {
        Self {
            key: key.into(),
            narrow: Arc::new(narrow),
        }
    }

    /// Whether `key` is the filter's key or under it.
    fn governs(&self, key: &str) -> bool {
        key.strip_prefix(self.key.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    }
}

impl fmt::Debug for ProjectFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProjectFilter")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

type SchemaFor = Arc<dyn Fn(&Table) -> Arc<Json> + Send + Sync>;

/// How a [`SettingsStore`] checks its files, beyond serde: a JSON schema,
/// what a project's file may set, and the one table of them it reads, if
/// it reads only one. [`SettingsStore::load_with`] takes them.
#[derive(Clone, Default)]
pub struct SettingsOptions {
    schema: Option<SchemaFor>,
    project_filters: Vec<ProjectFilter>,
    layer_filters: Vec<ProjectFilter>,
    section: Option<Section>,
}

/// The table of a shared file a store reads.
#[derive(Clone, Debug)]
struct Section {
    name: String,
    /// Every section of the file, this one among them, the first
    /// reporting what is no section's.
    all: Arc<[String]>,
}

impl Section {
    /// Whether this section's store reports what's wrong with the file
    /// outside every section: a syntax error, or a key no section has.
    fn reports_file(&self) -> bool {
        self.all.first().is_none_or(|first| *first == self.name)
    }

    /// Whether `key`, dotted, is this section's table or in it.
    fn holds(&self, key: &str) -> bool {
        key.strip_prefix(self.name.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    }

    /// The section's table of `file`: empty if it has none.
    fn table_of(&self, file: &Table) -> Table {
        match file.get(&self.name) {
            Some(toml::Value::Table(table)) => table.clone(),
            _ => Table::new(),
        }
    }

    /// `schema`, of the section's table, as one of the file: each other
    /// section a key it may have, checked by that section's store.
    fn file_schema(&self, schema: &Json) -> Json {
        let mut schema = schema.clone();
        let defs = schema
            .as_object_mut()
            .and_then(|schema| schema.remove("$defs"));
        let mut properties = Map::new();
        for name in self.all.iter() {
            properties.insert(name.clone(), json!({}));
        }
        properties.insert(self.name.clone(), schema);
        let mut file = json!({ "type": "object", "properties": properties });
        if let Some(defs) = defs {
            file["$defs"] = defs;
        }
        file
    }
}

impl SettingsOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks each file, and the layers merged, against `schema`, a JSON
    /// schema of `S` (`schemars::schema_for!(S)`): see [`SettingsStore`].
    pub fn schema(self, schema: Json) -> Self {
        let schema = Arc::new(schema);
        self.schema_for(move |_| schema.clone())
    }

    /// Checks each file, and the layers merged, against the schema
    /// `schema` gives for its table: for a schema that depends on what the
    /// file sets, such as one listing a map's keys as properties so that
    /// each entry is checked like a table.
    pub fn schema_for(
        mut self,
        schema: impl Fn(&Table) -> Arc<Json> + Send + Sync + 'static,
    ) -> Self {
        self.schema = Some(Arc::new(schema));
        self
    }

    /// Narrows what a project's file may set: see [`ProjectFilter`]. Each
    /// call adds one, run in order. Its key is the file's, a section's
    /// name first (`notes.lsp`).
    pub fn project_filter(mut self, filter: ProjectFilter) -> Self {
        self.project_filters.push(filter);
        self
    }

    /// Drops what no file may set badly, as [`SettingsOptions::project_filter`]
    /// does for a project's file, but for the user's file too, and before a
    /// project's own filters: an entry it drops (with a warning) rejects
    /// nothing, so the rest of the file still applies.
    pub fn layer_filter(mut self, filter: ProjectFilter) -> Self {
        self.layer_filters.push(filter);
        self
    }

    /// Reads only the table `name` of each file (`[notes]`), a file that
    /// the other sections in `all` share, one store each: `S` is that
    /// table, its defaults where the file has none. The schema is the
    /// table's, and the other sections' tables are keys it knows, so
    /// that only a key in no section, or one the section doesn't have,
    /// is unknown. Each store reports what's in its own table, and the
    /// first of `all` what's outside every section too: a syntax error,
    /// or a key no section has. An error anywhere in a file still keeps
    /// every section at its last good table.
    pub fn section<T: Into<String>>(
        mut self,
        name: impl Into<String>,
        all: impl IntoIterator<Item = T>,
    ) -> Self {
        let name = name.into();
        let mut all: Vec<String> = all.into_iter().map(Into::into).collect();
        if !all.contains(&name) {
            all.push(name.clone());
        }
        self.section = Some(Section {
            name,
            all: all.into(),
        });
        self
    }

    fn schema_of(&self, table: &Table) -> Option<Arc<Json>> {
        self.schema.as_ref().map(|schema| schema(table))
    }
}

impl fmt::Debug for SettingsOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsOptions")
            .field("schema", &self.schema.is_some())
            .field("project_filters", &self.project_filters)
            .field("section", &self.section)
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
/// ranges and lengths the schema sets (as schemars writes them). A project
/// file may be narrowed first ([`ProjectFilter`]).
pub struct SettingsStore<S> {
    /// The user layer, then the project layer, each if given.
    layers: Vec<LayerState>,
    options: SettingsOptions,
    settings: Arc<S>,
    diagnostics: Vec<Diagnostic>,
}

struct LayerState {
    path: PathBuf,
    /// Whether it's the user's file, rather than a project's.
    user: bool,
    /// The table last loaded without errors.
    good: Table,
    /// The file's text that `good` came from: empty until it has loaded
    /// cleanly once.
    good_source: String,
    diagnostics: Vec<Diagnostic>,
}

impl<S: SettingsType> SettingsStore<S> {
    /// Loads the user's and the project's settings files, either of which
    /// may be absent or missing, checked by serde alone.
    pub fn load(user: Option<PathBuf>, project: Option<PathBuf>) -> Self {
        Self::load_with(user, project, SettingsOptions::new())
    }

    /// Loads them as [`SettingsStore::load`] does, also checking each file
    /// against `schema`, a JSON schema of `S` (`schemars::schema_for!(S)`).
    pub fn with_schema(user: Option<PathBuf>, project: Option<PathBuf>, schema: Json) -> Self {
        Self::load_with(user, project, SettingsOptions::new().schema(schema))
    }

    /// Loads them as [`SettingsStore::load`] does, checked as `options`
    /// say: a schema, and what a project's file may set.
    pub fn load_with(
        user: Option<PathBuf>,
        project: Option<PathBuf>,
        options: SettingsOptions,
    ) -> Self {
        let mut layers: Vec<_> = [(user, true), (project, false)]
            .into_iter()
            .filter_map(|(path, user)| {
                Some(LayerState {
                    path: path?,
                    user,
                    good: Table::new(),
                    good_source: String::new(),
                    diagnostics: Vec::new(),
                })
            })
            .collect();
        for layer in &mut layers {
            read::<S>(layer, &options);
        }
        let (settings, diagnostics) = effective::<S>(&layers, &options);
        Self {
            layers,
            options,
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

    /// The text each layer's last good table came from, the user's first:
    /// what the store accepted, which may differ from the file now. Empty
    /// for a layer that has not loaded cleanly yet.
    pub fn accepted_sources(&self) -> Vec<&str> {
        (self.layers.iter())
            .map(|layer| layer.good_source.as_str())
            .collect()
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
                read::<S>(layer, &self.options);
                read_any = true;
            }
        }
        if !read_any {
            return None;
        }
        let (settings, diagnostics) = effective::<S>(&self.layers, &self.options);
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
fn read<S: SettingsType>(layer: &mut LayerState, options: &SettingsOptions) {
    let mut filters = options.layer_filters.clone();
    if !layer.user {
        filters.extend(options.project_filters.iter().cloned());
    }
    let (accepted, diagnostics) = validate::<S>(&layer.path, &filters, options);
    if let Some((table, source)) = accepted {
        layer.good = table;
        layer.good_source = source;
    }
    layer.diagnostics = diagnostics;
}

/// Loads the settings file at `path`: its table and text if it has no errors, and
/// its diagnostics. `filters` narrow it first, so a bad value in a key
/// they drop anyway rejects nothing.
fn validate<S: SettingsType>(
    path: &Path,
    filters: &[ProjectFilter],
    options: &SettingsOptions,
) -> (Option<(Table, String)>, Vec<Diagnostic>) {
    let section = options.section.as_ref();
    let mut layer = load_layer(path);
    // What loading found is the file's: a syntax error, or no reading it.
    let loaded = layer.diagnostics.len();
    if !layer.has_errors() {
        for filter in filters {
            (filter.narrow)(&mut layer);
        }
        // Checked on the source, for the error's location, unless the
        // error is in a key a filter governs: then the narrowed table
        // decides.
        if let Err(error) = from_source::<S>(&layer.source, section) {
            let diagnostic = layer.diagnostic_for(Severity::Error, &error);
            let narrowed = diagnostic
                .key
                .as_deref()
                .is_some_and(|key| filters.iter().any(|filter| filter.governs(key)));
            let table = match section {
                Some(section) => section.table_of(&layer.table),
                None => layer.table.clone(),
            };
            if !narrowed {
                layer.diagnostics.push(diagnostic);
            } else if let Err(error) = toml::Value::Table(table).try_into::<S>() {
                let message = error.message().trim_end().to_owned();
                let diagnostic = layer.diagnostic_at(Severity::Error, None, 0, message);
                layer.diagnostics.push(diagnostic);
            }
        }
        if !layer.has_errors() {
            let more = match section {
                Some(section) => options
                    .schema_of(&section.table_of(&layer.table))
                    .map(|schema| layer.check_schema(&section.file_schema(&schema))),
                None => options
                    .schema_of(&layer.table)
                    .map(|schema| layer.check_schema(&schema)),
            };
            layer.diagnostics.extend(more.unwrap_or_default());
        }
    }
    let rejected = layer.has_errors();
    let mut diagnostics = layer.diagnostics;
    if let Some(section) = section
        && !section.reports_file()
    {
        // Another section's store reports them.
        let mut ix = 0;
        diagnostics.retain(|diagnostic| {
            ix += 1;
            ix > loaded
                && diagnostic
                    .key
                    .as_deref()
                    .is_none_or(|key| section.holds(key))
        });
    }
    let table = match section {
        Some(section) => section.table_of(&layer.table),
        None => layer.table,
    };
    ((!rejected).then_some((table, layer.source)), diagnostics)
}

/// Deserializes `S` from the TOML `source`, or from its table `section`,
/// for an error's location in it.
fn from_source<S: DeserializeOwned>(
    source: &str,
    section: Option<&Section>,
) -> Result<(), toml::de::Error> {
    let Some(section) = section else {
        return toml::from_str::<S>(source).map(drop);
    };
    let seed = SectionSeed::<S> {
        name: &section.name,
        settings: PhantomData,
    };
    seed.deserialize(toml::de::Deserializer::parse(source)?)
}

/// Deserializes the table `name` of a file as an `S`, skipping the rest.
struct SectionSeed<'a, S> {
    name: &'a str,
    settings: PhantomData<S>,
}

impl<'de, S: DeserializeOwned> DeserializeSeed<'de> for SectionSeed<'_, S> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de, S: DeserializeOwned> Visitor<'de> for SectionSeed<'_, S> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a settings file with a table `{}`", self.name)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            if key == self.name {
                map.next_value::<S>()?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}

/// The layers' good tables merged over the defaults, and their diagnostics.
fn effective<S: SettingsType>(
    layers: &[LayerState],
    options: &SettingsOptions,
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
    let errors: Vec<_> = options
        .schema_of(&merged.table)
        .map(|schema| merged.check_schema(&schema))
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
