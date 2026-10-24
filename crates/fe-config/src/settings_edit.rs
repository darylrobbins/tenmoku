//! Writing one setting, from the Settings editor, into the user's
//! `settings.toml` or the project's, and resetting one to its default by
//! removing its key: kiln-core's [`write_settings`] and
//! [`reset_settings`], which keep the rest of the file as it was.
//!
//! A value is checked as loading the file would check it, against
//! [`Settings`] and [`settings_schema`], before anything is written, and a
//! project's file can't be given a key its layer ignores
//! ([`SettingsModel::workspace_allowed`]).

use std::{fmt, io, path::Path};

use kiln_core::write::{reset_settings, write_settings};
use toml::Table;

use crate::{
    SettingEntry, SettingKind, SettingValue, Settings, SettingsModel, USER_SETTINGS_TEMPLATE,
    layers::Layer, project_settings_path, settings_schema, user_settings_path,
};

/// Which settings file a change goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingScope {
    /// The user's `settings.toml`.
    User,
    /// The project's `.fastedit/settings.toml`.
    Workspace,
}

/// Why a setting wasn't written.
#[derive(Debug)]
pub enum SettingEditError {
    /// A project's file can't set this key: its layer ignores it.
    NotAllowedInWorkspace,
    /// The value isn't one the setting takes, with why.
    Invalid(String),
    /// The file couldn't be read or written.
    Io(io::Error),
}

impl fmt::Display for SettingEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAllowedInWorkspace => {
                f.write_str("a project's settings can't set this; set it in your own settings.toml")
            }
            Self::Invalid(message) => f.write_str(message),
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SettingEditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for SettingEditError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Sets `key` (`["editor", "font_size"]`) to `value` in `scope`'s settings
/// file: the user's ([`user_settings_path`]), created from
/// [`USER_SETTINGS_TEMPLATE`] if it's missing, or `root`'s project file
/// ([`project_settings_path`]), created empty. Nothing is written for a
/// value the setting doesn't take, or a key a project can't set.
pub fn set_setting(
    scope: SettingScope,
    root: &Path,
    key: &[&str],
    value: SettingValue<'_>,
) -> Result<(), SettingEditError> {
    let model = SettingsModel::new(None, None);
    allowed(&model, scope, key)?;
    validate(&model, key, value)?;
    let (path, template) = file(scope, root)?;
    write_settings(&path, &[(key, Some(value))], template)?;
    Ok(())
}

/// Resets `key` to its default by removing it from `scope`'s settings
/// file, as [`set_setting`] picks it, with each table that leaves empty;
/// one that still holds another key or a comment stays. A key that isn't
/// there, or a file that's missing, writes nothing. A project's file can't
/// be changed at a key it can't set. A key that can't be removed without
/// dropping a comment beside it (a dotted key in an inline table, say) is
/// [`SettingEditError::Invalid`], saying to edit the file by hand, and the
/// file is left as it was.
pub fn reset_setting(
    scope: SettingScope,
    root: &Path,
    key: &[&str],
) -> Result<(), SettingEditError> {
    allowed(&SettingsModel::new(None, None), scope, key)?;
    let (path, template) = file(scope, root)?;
    reset_settings(&path, &[key], template).map_err(|error| match error.kind() {
        // A dotted key in braces with a comment beside it, say.
        io::ErrorKind::Unsupported => SettingEditError::Invalid(format!(
            "{}: can't be removed without losing a comment; edit settings.toml by hand",
            key.join(".")
        )),
        _ => SettingEditError::Io(error),
    })
}

/// `scope`'s settings file, and the template a missing one starts from.
fn file(scope: SettingScope, root: &Path) -> io::Result<(std::path::PathBuf, &'static str)> {
    match scope {
        SettingScope::User => user_settings_path()
            .map(|path| (path, USER_SETTINGS_TEMPLATE))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "there's no config directory")),
        SettingScope::Workspace => Ok((project_settings_path(root), "")),
    }
}

/// Whether `scope`'s file may set `key`.
fn allowed(
    model: &SettingsModel,
    scope: SettingScope,
    key: &[&str],
) -> Result<(), SettingEditError> {
    let entry = SettingEntry {
        key: key.iter().map(|&part| part.to_owned()).collect(),
        group: String::new(),
        kind: SettingKind::Other,
        default: serde_json::Value::Null,
        description: String::new(),
    };
    match scope {
        SettingScope::Workspace if !model.workspace_allowed(&entry) => {
            Err(SettingEditError::NotAllowedInWorkspace)
        }
        _ => Ok(()),
    }
}

/// Whether `key` takes `value`, checked as a file holding only it would
/// be: deserialized as [`Settings`], then against [`settings_schema`]'s
/// types, bounds and, for a setting that's one of some strings, its
/// variants.
fn validate(
    model: &SettingsModel,
    key: &[&str],
    value: SettingValue<'_>,
) -> Result<(), SettingEditError> {
    let invalid = |message: String| {
        Err(SettingEditError::Invalid(format!(
            "{}: {message}",
            key.join(".")
        )))
    };
    let toml_value = match value {
        SettingValue::Bool(value) => toml::Value::Boolean(value),
        SettingValue::Str(text) => toml::Value::String(text.to_owned()),
        SettingValue::Int(value) => toml::Value::Integer(value),
        SettingValue::Float(value) if !value.is_finite() => {
            return invalid(format!("{value} isn't a number"));
        }
        SettingValue::Float(value) => toml::Value::Float(value),
        SettingValue::StrList(items) => toml::Value::Array(
            items
                .iter()
                .map(|&item| toml::Value::String(item.to_owned()))
                .collect(),
        ),
    };
    let (name, tables) = key.split_last().expect("a setting has a name");
    let mut table = Table::from_iter([(name.to_string(), toml_value)]);
    for part in tables.iter().rev() {
        table = Table::from_iter([(part.to_string(), toml::Value::Table(table))]);
    }
    if let Err(error) = toml::Value::Table(table.clone()).try_into::<Settings>() {
        return invalid(error.message().trim_end().to_owned());
    }
    let mut layer = Layer::default();
    layer.table = table;
    if let Some(error) = layer
        .check_schema(&settings_schema())
        .into_iter()
        .find(|diagnostic| diagnostic.is_error())
    {
        return invalid(error.message);
    }
    let entry = model
        .entries()
        .iter()
        .find(|entry| entry.key.iter().map(String::as_str).eq(key.iter().copied()));
    if let (Some(entry), SettingValue::Str(text)) = (entry, value)
        && let SettingKind::Enum(variants) = &entry.kind
        && !variants.iter().any(|variant| variant.value == text)
    {
        let names: Vec<String> = variants
            .iter()
            .map(|variant| format!("\"{}\"", variant.value))
            .collect();
        return invalid(format!("must be one of {}", names.join(", ")));
    }
    Ok(())
}
