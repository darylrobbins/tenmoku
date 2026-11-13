//! The Settings editor's model, without a view: every setting
//! [`settings_schema`] declares, one [`SettingEntry`] row per leaf, with its
//! user and workspace values, searched and grouped as VS Code's Settings
//! editor lists them.
//!
//! The schema is the only list of settings, so a setting added to
//! [`Settings`](crate::Settings) appears here with no change. Tables and
//! arrays of tables the editor can't edit in place (`lsp.servers`,
//! `vim.remap`) are one [`SettingKind::Other`] row each, edited in
//! `settings.toml`. Strings whose values only the host knows, the themes
//! and fonts installed, or that a list picks from, the Claude models, are
//! [`SettingKind::Choice`], by key ([`choice_source`]), so the schema stays
//! a string's. Nothing here writes a file.

use kiln_core::theme::Appearance;
use serde_json::{Map, Value};
use toml::Table;

use crate::l10n::{KeyKind, setting_key, variant_attribute};
use crate::settings_schema;

/// One setting: a leaf of the schema.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingEntry {
    /// Its keys, as in `["editor", "minimap", "enabled"]`.
    pub key: Vec<String>,
    /// The title of the group it's in, as in `Editor > Minimap`.
    pub group: String,
    /// What it holds, which says how it's edited.
    pub kind: SettingKind,
    /// Its built-in value; null for one that's unset by default.
    pub default: Value,
    /// The schema's description of it.
    pub description: String,
}

impl SettingEntry {
    /// Its dotted key, as in `editor.minimap.enabled`.
    pub fn key_path(&self) -> String {
        self.key.join(".")
    }
}

/// What a setting holds.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingKind {
    Bool,
    /// One of these strings.
    Enum(Vec<EnumVariant>),
    /// A whole number, within the schema's bounds.
    Int {
        min: Option<i64>,
        max: Option<i64>,
    },
    /// A number, within the schema's bounds.
    Float {
        min: Option<f64>,
        max: Option<f64>,
    },
    /// A string, at least `min_length` characters long if set.
    Text {
        min_length: Option<u64>,
    },
    /// A list of strings.
    TextList,
    /// A string, one of a list the host gives at runtime.
    Choice(ChoiceSource),
    /// A table, an array of tables or anything else the editor can't edit:
    /// it links to `settings.toml`.
    Other,
}

/// The list a [`SettingKind::Choice`]'s value is one of, which the host
/// resolves: the themes installed now, say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceSource {
    /// The bundled and installed themes of `appearance`, by ID; or with
    /// none, the bundled ones, by title.
    Themes { appearance: Option<Appearance> },
    /// The Claude models the app knows ([`chat_models::KNOWN`]), though
    /// the setting takes any id.
    ///
    /// [`chat_models::KNOWN`]: crate::chat_models::KNOWN
    ChatModels,
    /// The font families installed.
    Fonts,
}

/// One of an [`SettingKind::Enum`]'s values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumVariant {
    pub value: String,
    /// The schema's description of it; empty if it has none.
    pub description: String,
}

/// A table of the schema, with the settings in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingGroup {
    /// Its keys, as in `["editor", "minimap"]`.
    pub key: Vec<String>,
    /// As in `Editor > Minimap`.
    pub title: String,
    /// The schema's description of the table.
    pub description: String,
    /// How many rows are in it, not counting its nested groups'.
    pub count: usize,
}

/// Where a setting's effective value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueSource {
    Default,
    /// The user's `settings.toml`.
    User,
    /// The project's `.tenmoku/settings.toml`.
    Workspace,
}

/// A setting's value and the layer it comes from.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveValue {
    pub value: Value,
    pub source: ValueSource,
}

/// Every setting with its values in the user's and the project's settings
/// files.
#[derive(Clone, Debug)]
pub struct SettingsModel {
    entries: Vec<SettingEntry>,
    /// Each entry's schema, `$ref` followed, which fills in the defaults of
    /// a table or array of tables' items.
    schemas: Vec<Value>,
    defs: Value,
    groups: Vec<SettingGroup>,
    user: Table,
    /// The project's file as written.
    project: Table,
    /// The project's file with the keys a project can't set dropped, as
    /// [`SettingsStore`](crate::SettingsStore) drops them.
    applied: Table,
    errors: Vec<String>,
}

impl SettingsModel {
    /// The settings with the user's and the project's settings files'
    /// text, either of which may be missing. A file that doesn't parse adds
    /// to [`errors`](Self::errors) and sets nothing.
    pub fn new(user: Option<&str>, project: Option<&str>) -> Self {
        let mut errors = Vec::new();
        let mut parse = |user: bool, text: Option<&str>| match text.map(str::parse::<Table>) {
            None => Table::new(),
            Some(Ok(table)) => table,
            Some(Err(error)) => {
                let error = error.to_string();
                errors.push(
                    match user {
                        true => kiln_l10n::tr!("config-settings-user-unparsed", error = error),
                        false => {
                            kiln_l10n::tr!("config-settings-workspace-unparsed", error = error)
                        }
                    }
                    .to_string(),
                );
                Table::new()
            }
        };
        let user = parse(true, user);
        let project = parse(false, project);
        let applied = applied_project(&project);
        let schema = settings_schema();
        let mut walk = rows(&schema);
        localize(&mut walk);
        let Walk {
            defs,
            entries,
            schemas,
            groups,
            ..
        } = walk;
        Self {
            entries,
            schemas,
            defs,
            groups,
            user,
            project,
            applied,
            errors,
        }
    }

    /// Every setting, in schema order.
    pub fn entries(&self) -> &[SettingEntry] {
        &self.entries
    }

    /// Every table, in schema order, a table before those nested in it.
    pub fn groups(&self) -> &[SettingGroup] {
        &self.groups
    }

    /// Why each file that didn't parse didn't.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// `entry`'s value in the user's file, if it sets it.
    pub fn user_value(&self, entry: &SettingEntry) -> Option<Value> {
        lookup(&self.user, &entry.key)
    }

    /// `entry`'s value in the project's file, if it sets it, whether or not
    /// a project may ([`workspace_allowed`](Self::workspace_allowed)).
    pub fn workspace_value(&self, entry: &SettingEntry) -> Option<Value> {
        lookup(&self.project, &entry.key)
    }

    /// `entry`'s effective value: the project's, then the user's, then the
    /// default. A table's is the three merged key by key, and the tables in
    /// it, or in an array of tables, get their keys' defaults.
    pub fn value(&self, entry: &SettingEntry) -> EffectiveValue {
        let layers = [
            (ValueSource::User, lookup(&self.user, &entry.key)),
            (ValueSource::Workspace, lookup(&self.applied, &entry.key)),
        ];
        let mut effective = EffectiveValue {
            value: entry.default.clone(),
            source: ValueSource::Default,
        };
        for (source, value) in layers {
            let Some(value) = value else { continue };
            effective.value = match (effective.value, value) {
                (Value::Object(mut base), Value::Object(over)) => {
                    merge_json(&mut base, over);
                    Value::Object(base)
                }
                (_, value) => value,
            };
            effective.source = source;
        }
        if let Some(index) = self.entries.iter().position(|row| row.key == entry.key) {
            fill_defaults(&self.defs, &self.schemas[index], &mut effective.value);
        }
        effective
    }

    /// Whether the user's file or the project's sets `entry`, even to its
    /// default, and even where a project may not
    /// ([`workspace_allowed`](Self::workspace_allowed)).
    pub fn is_modified(&self, entry: &SettingEntry) -> bool {
        lookup(&self.user, &entry.key).is_some() || lookup(&self.project, &entry.key).is_some()
    }

    /// Whether a project's file may set `entry`: not [`PROJECT_CHAT`],
    /// [`PROJECT_TERMINAL_SHELL`] or [`PROJECT_VIM_USER_ONLY`]'s keys, not
    /// the theme's, and of `lsp` only `enabled` and `servers`, from which
    /// only `enabled = false` applies (outside a trusted folder).
    ///
    /// [`PROJECT_CHAT`]: crate::PROJECT_CHAT
    /// [`PROJECT_TERMINAL_SHELL`]: crate::PROJECT_TERMINAL_SHELL
    /// [`PROJECT_VIM_USER_ONLY`]: crate::PROJECT_VIM_USER_ONLY
    pub fn workspace_allowed(&self, entry: &SettingEntry) -> bool {
        let key: Vec<&str> = entry.key.iter().map(String::as_str).collect();
        !matches!(
            key.as_slice(),
            ["chat", ..]
                | ["theme", ..]
                | ["terminal", "shell" | "shell_args"]
                | ["vim", "shell" | "remap"]
                | ["workbench", "icon_theme" | "locale"]
        ) && match key.as_slice() {
            // Only what `narrow_agents` keeps: class levels, commands_ask
            // and commands_deny, which only tighten the user's.
            ["agents", rest @ ..] => matches!(
                rest,
                [
                    "permissions",
                    "read"
                        | "search"
                        | "propose_edit"
                        | "run_command"
                        | "fetch"
                        | "other"
                        | "commands_ask"
                        | "commands_deny"
                ]
            ),
            ["lsp", rest @ ..] => matches!(rest, ["enabled"] | ["servers", ..]),
            _ => true,
        }
    }

    /// The settings `query` finds, in schema order: those whose dotted key,
    /// group title or description holds every word of it, ignoring case.
    /// `@modified` keeps only [modified](Self::is_modified) ones, `@user`
    /// those the user's file sets and `@workspace` those the project's
    /// does. An empty query finds them all.
    pub fn search(&self, query: &str) -> Vec<&SettingEntry> {
        let mut terms = Vec::new();
        let (mut modified, mut user, mut workspace) = (false, false, false);
        for word in query.split_whitespace() {
            let word = word.to_lowercase();
            match word.as_str() {
                "@modified" => modified = true,
                "@user" => user = true,
                "@workspace" => workspace = true,
                _ => terms.push(word),
            }
        }
        self.entries
            .iter()
            .filter(|entry| !modified || self.is_modified(entry))
            .filter(|entry| !user || lookup(&self.user, &entry.key).is_some())
            .filter(|entry| !workspace || lookup(&self.project, &entry.key).is_some())
            .filter(|entry| {
                if terms.is_empty() {
                    return true;
                }
                let text = format!(
                    "{}\n{}\n{}",
                    entry.key_path(),
                    entry.group,
                    entry.description
                )
                .to_lowercase();
                terms.iter().all(|term| text.contains(term.as_str()))
            })
            .collect()
    }
}

/// The value at `key` in `table`, as JSON.
fn lookup(table: &Table, key: &[String]) -> Option<Value> {
    let (last, tables) = key.split_last()?;
    let mut table = table;
    for name in tables {
        table = table.get(name)?.as_table()?;
    }
    serde_json::to_value(table.get(last)?).ok()
}

/// `over` laid on `base` key by key.
fn merge_json(base: &mut Map<String, Value>, over: Map<String, Value>) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(Value::Object(base)), Value::Object(over)) => merge_json(base, over),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// `value` with the defaults `schema` gives the keys of its tables, and of
/// the tables in its maps and arrays, that it leaves out, as deserializing
/// [`Settings`](crate::Settings) fills them in.
fn fill_defaults(defs: &Value, schema: &Value, value: &mut Value) {
    let schema = resolve(defs, schema);
    match value {
        Value::Object(object) => {
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                for (name, property) in properties {
                    match object.get_mut(name) {
                        Some(value) => fill_defaults(defs, property, value),
                        None => {
                            if let Some(default) = property.get("default") {
                                object.insert(name.clone(), default.clone());
                            }
                        }
                    }
                }
            }
            if let Some(items) = schema
                .get("additionalProperties")
                .filter(|items| items.is_object())
            {
                for (name, value) in object.iter_mut() {
                    if !schema
                        .get("properties")
                        .is_some_and(|properties| properties.get(name).is_some())
                    {
                        fill_defaults(defs, items, value);
                    }
                }
            }
        }
        Value::Array(values) => {
            if let Some(items) = schema.get("items") {
                for value in values {
                    fill_defaults(defs, items, value);
                }
            }
        }
        _ => {}
    }
}

/// `schema` with a `$ref` to `defs` followed.
fn resolve<'a>(defs: &'a Value, schema: &'a Value) -> &'a Value {
    let target = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|path| path.strip_prefix("#/$defs/"))
        .and_then(|name| defs.get(name));
    match target {
        Some(target) => resolve(defs, target),
        None => schema,
    }
}

/// `project` with what a project file can't set dropped, as the settings
/// store's project filters drop it, its `agents.permissions` moved to
/// `agents.project_permissions`.
fn applied_project(project: &Table) -> Table {
    let mut table = project.clone();
    let agents = applied_agents(project);
    table.remove("agents");
    if let Some(agents) = agents {
        table.insert("agents".into(), agents);
    }
    table.remove("chat");
    table.remove("theme");
    for (name, keys) in [
        ("terminal", &["shell", "shell_args"][..]),
        ("vim", &["shell", "remap"]),
        ("workbench", &["icon_theme", "locale"]),
    ] {
        if let Some(toml::Value::Table(inner)) = table.get_mut(name) {
            for key in keys {
                inner.remove(*key);
            }
        }
    }
    if let Some(lsp) = table.remove("lsp") {
        let off = toml::Value::Boolean(false);
        let mut kept = Table::new();
        if let toml::Value::Table(lsp) = lsp {
            if lsp.get("enabled") == Some(&off) {
                kept.insert("enabled".into(), off.clone());
            }
            if let Some(toml::Value::Table(servers)) = lsp.get("servers") {
                let servers: Table = servers
                    .iter()
                    .filter(|(_, server)| server.get("enabled") == Some(&off))
                    .map(|(name, _)| {
                        let server = Table::from_iter([("enabled".to_owned(), off.clone())]);
                        (name.clone(), toml::Value::Table(server))
                    })
                    .collect();
                if !servers.is_empty() {
                    kept.insert("servers".into(), toml::Value::Table(servers));
                }
            }
        }
        if !kept.is_empty() {
            table.insert("lsp".into(), toml::Value::Table(kept));
        }
    }
    table
}

/// The descriptions of `walk`'s rows, of their values and of its groups in
/// the active locale, by their keys ([`setting_key`]): a value's is its
/// row's message's attribute ([`variant_attribute`]). The schema's, the
/// en-US text, is the fallback.
fn localize(walk: &mut Walk) {
    let args = kiln_l10n::Args::new();
    for entry in &mut walk.entries {
        let key = setting_key(KeyKind::Description, &entry.key_path());
        entry.description = localized(&key, &entry.description);
        if let SettingKind::Enum(variants) = &mut entry.kind {
            for variant in variants.iter_mut() {
                if variant.description.is_empty() {
                    continue;
                }
                let name = variant_attribute(&variant.value);
                if let Some(text) = kiln_l10n::attribute(&key, &name, &args) {
                    variant.description = text.to_string();
                }
            }
        }
    }
    for group in &mut walk.groups {
        let key = setting_key(KeyKind::Description, &group.key.join("."));
        group.description = localized(&key, &group.description);
    }
}

/// `key`'s message, or `fallback` without one; none for no `fallback`.
fn localized(key: &str, fallback: &str) -> String {
    if fallback.is_empty() {
        return String::new();
    }
    kiln_l10n::tr_or(key, fallback).to_string()
}

/// The rows and groups of `schema`, in its order, with the schema's
/// descriptions.
pub(crate) fn rows(schema: &Value) -> Walk {
    let mut walk = Walk {
        defs: schema.get("$defs").cloned().unwrap_or(Value::Null),
        entries: Vec::new(),
        schemas: Vec::new(),
        groups: Vec::new(),
        #[cfg(test)]
        order: Vec::new(),
    };
    walk.table(schema, &[], &Value::Null);
    for group in &mut walk.groups {
        group.count = walk
            .entries
            .iter()
            .filter(|entry| entry.key[..entry.key.len() - 1] == group.key[..])
            .count();
    }
    walk
}

pub(crate) struct Walk {
    defs: Value,
    pub(crate) entries: Vec<SettingEntry>,
    /// Each entry's schema, `$ref` followed.
    schemas: Vec<Value>,
    pub(crate) groups: Vec<SettingGroup>,
    /// The groups and entries, in the order the walk met them.
    #[cfg(test)]
    pub(crate) order: Vec<Row>,
}

/// One of a [`Walk`]'s groups or entries, by its index.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum Row {
    Group(usize),
    Entry(usize),
}

impl Walk {
    fn resolve<'a>(&'a self, schema: &'a Value) -> &'a Value {
        resolve(&self.defs, schema)
    }

    /// The rows of the table at `key`, whose default is `defaults`.
    fn table(&mut self, schema: &Value, key: &[String], defaults: &Value) {
        let group = group_title(key);
        let Some(properties) = self
            .resolve(schema)
            .get("properties")
            .and_then(Value::as_object)
        else {
            return;
        };
        for (name, property) in properties.clone() {
            let mut key = key.to_vec();
            key.push(name.clone());
            let resolved = self.resolve(&property).clone();
            let description = description(&property)
                .or_else(|| description(&resolved))
                .unwrap_or_default();
            let default = property
                .get("default")
                .or_else(|| defaults.get(&name))
                .cloned()
                .unwrap_or(Value::Null);
            if resolved.get("properties").is_some_and(Value::is_object) {
                #[cfg(test)]
                self.order.push(Row::Group(self.groups.len()));
                self.groups.push(SettingGroup {
                    title: group_title(&key),
                    key: key.clone(),
                    description,
                    count: 0,
                });
                self.table(&resolved, &key, &default);
            } else if key.len() == 1 {
                // A map of no table (`formatters`) is edited in settings.toml:
                // the editor lists a top-level row before every heading.
            } else {
                let kind = match choice_source(&key) {
                    Some(source) => SettingKind::Choice(source),
                    None => self.kind(&resolved),
                };
                #[cfg(test)]
                self.order.push(Row::Entry(self.entries.len()));
                self.entries.push(SettingEntry {
                    key,
                    group: group.clone(),
                    kind,
                    default,
                    description,
                });
                self.schemas.push(resolved);
            }
        }
    }

    fn kind(&self, schema: &Value) -> SettingKind {
        if let Some(variants) = self.variants(schema) {
            return SettingKind::Enum(variants);
        }
        match types(schema).as_slice() {
            ["boolean"] => SettingKind::Bool,
            ["integer"] => SettingKind::Int {
                min: schema.get("minimum").and_then(as_i64),
                max: schema.get("maximum").and_then(as_i64),
            },
            ["number"] => SettingKind::Float {
                min: schema.get("minimum").and_then(Value::as_f64),
                max: schema.get("maximum").and_then(Value::as_f64),
            },
            ["string"] => SettingKind::Text {
                min_length: schema.get("minLength").and_then(Value::as_u64),
            },
            ["array"] => match schema.get("items").map(|items| self.resolve(items)) {
                Some(items) if types(items) == ["string"] && self.variants(items).is_none() => {
                    SettingKind::TextList
                }
                _ => SettingKind::Other,
            },
            _ => SettingKind::Other,
        }
    }

    /// The strings `schema` is one of, if it's only ever one of some
    /// strings (or null).
    fn variants(&self, schema: &Value) -> Option<Vec<EnumVariant>> {
        let schema = self.resolve(schema);
        if let Some(values) = schema.get("enum").and_then(Value::as_array) {
            return values
                .iter()
                .filter(|value| !value.is_null())
                .map(|value| {
                    Some(EnumVariant {
                        value: value.as_str()?.to_owned(),
                        description: String::new(),
                    })
                })
                .collect();
        }
        if let Some(value) = schema.get("const") {
            return Some(vec![EnumVariant {
                value: value.as_str()?.to_owned(),
                description: description(schema).unwrap_or_default(),
            }]);
        }
        let options = schema
            .get("oneOf")
            .or_else(|| schema.get("anyOf"))
            .and_then(Value::as_array)?;
        let mut variants = Vec::new();
        for option in options {
            if types(self.resolve(option)) == ["null"] {
                continue;
            }
            variants.extend(self.variants(option)?);
        }
        (!variants.is_empty()).then_some(variants)
    }
}

/// The list the setting at `key` is one of, if only the host knows it: a
/// table here, not a schema keyword, so `settings.schema.json` stays as it
/// is.
fn choice_source(key: &[String]) -> Option<ChoiceSource> {
    let key: Vec<&str> = key.iter().map(String::as_str).collect();
    let appearance = match key.as_slice() {
        ["theme", "dark"] => Some(Appearance::Dark),
        ["theme", "light"] => Some(Appearance::Light),
        ["theme", "name"] => None,
        ["chat", "model"] => return Some(ChoiceSource::ChatModels),
        [.., "font_family"] => return Some(ChoiceSource::Fonts),
        _ => return None,
    };
    Some(ChoiceSource::Themes { appearance })
}

/// `schema`'s `type`s other than null.
fn types(schema: &Value) -> Vec<&str> {
    let types = match schema.get("type") {
        Some(Value::String(name)) => vec![name.as_str()],
        Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    match types.as_slice() {
        ["null"] => types,
        _ => types.into_iter().filter(|name| *name != "null").collect(),
    }
}

fn description(schema: &Value) -> Option<String> {
    schema
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|value| value as i64))
}

/// The title of the group at `key`, as in `Editor > Minimap`, in the
/// active locale: each table's name by its key, `settings-group-<name>`.
fn group_title(key: &[String]) -> String {
    key.iter()
        .map(|name| kiln_l10n::tr_or(&setting_key(KeyKind::Group, name), &title(name)).to_string())
        .collect::<Vec<_>>()
        .join(" > ")
}

/// `name` as a title in en-US: `status_bar` is `Status Bar`, and `lsp` is
/// `LSP`.
pub(crate) fn title(name: &str) -> String {
    name.split(['_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| match word {
            "lsp" | "ai" | "ui" => word.to_uppercase(),
            _ => {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().chain(chars).collect())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// What `narrow_agents` keeps of a project's `agents`: its valid class
/// levels, `commands_ask` and `commands_deny`, moved to
/// `agents.project_permissions`.
fn applied_agents(project: &Table) -> Option<toml::Value> {
    let permissions = project
        .get("agents")?
        .as_table()?
        .get("permissions")?
        .as_table()?;
    let mut kept = Table::new();
    for (name, value) in permissions {
        let ok = match (name.as_str(), value) {
            (
                "read" | "search" | "propose_edit" | "run_command" | "fetch" | "other",
                toml::Value::String(level),
            ) => matches!(level.as_str(), "allow" | "ask" | "deny"),
            ("commands_ask" | "commands_deny", toml::Value::Array(items)) => {
                items.iter().all(toml::Value::is_str)
            }
            _ => false,
        };
        if ok {
            kept.insert(name.clone(), value.clone());
        }
    }
    if kept.is_empty() {
        return None;
    }
    let mut agents = Table::new();
    agents.insert("project_permissions".into(), toml::Value::Table(kept));
    Some(toml::Value::Table(agents))
}
