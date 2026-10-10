// SPDX-License-Identifier: LicenseRef-Proprietary

//! Reading a theme file: what it sets, and what's wrong with it.
//!
//! ```toml
//! kiln_theme = 1
//! name = "Paper"
//! appearance = "light"
//!
//! [accent]
//! base = "#3b6fd4"
//!
//! [list]
//! row_selected = "#3b6fd433"
//! ```
//!
//! Unlike a settings file, a theme with a mistake still loads: a mistake is
//! about one key, which is reported as a warning and left out, as if the
//! file didn't set it. Only a file that can't be read or isn't TOML is
//! rejected.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use toml::{Table, Value};

use super::{Appearance, Color, TOKEN_GROUPS, TOKEN_KEYS};
use crate::{
    diagnostic::{Diagnostic, Severity},
    layers::{Layer, load_layer, parse_layer},
};

/// The theme file format this build reads, as `kiln_theme = 1` declares.
/// A file without the key is read as this version.
pub const THEME_VERSION: i64 = 1;

/// The table of the flat chrome colours that theme files had before
/// tokens, still read so those files load.
pub const ALIAS_TABLE: &str = "ui";

/// The keys of the [`ALIAS_TABLE`] and the token each sets.
pub const ALIASES: &[(&str, &str)] = &[
    ("background", "surface.background"),
    ("panel_background", "surface.panel"),
    ("border", "border.default"),
    ("text", "text.primary"),
    ("muted_text", "text.muted"),
    ("error_text", "status.error"),
    ("row_hover", "list.row_hover"),
    ("row_selected", "list.row_selected"),
    ("row_selected_inactive", "list.row_selected_inactive"),
    ("row_focus_border", "list.row_focus_border"),
    ("icon", "list.icon"),
    ("folder_icon", "list.folder_icon"),
    ("chevron", "list.chevron"),
    ("indent_guide", "list.indent_guide"),
    ("tab_active", "tab.active"),
    ("tab_active_text", "tab.active_text"),
    ("tab_inactive", "tab.inactive"),
    ("dirty_dot", "state.dirty"),
    ("drop_target", "state.drop_target"),
    ("accent", "accent.base"),
];

/// What one theme file says, before it's laid over a base.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeFile {
    pub path: PathBuf,
    pub name: Option<String>,
    pub appearance: Option<Appearance>,
    /// The tokens the file sets, by key (`"list.row_selected"`), the
    /// [`ALIAS_TABLE`]'s included; a token set both ways takes the token's
    /// own key.
    pub tokens: BTreeMap<&'static str, Color>,
    /// The app's own tables, by name, kept as written for the app to read:
    /// say, an editor's colours. Only names the reader was told of are
    /// kept; any other table is an unknown key.
    pub extensions: BTreeMap<String, Table>,
}

impl ThemeFile {
    /// Reads the theme file at `path`, keeping the tables named in
    /// `extensions`. It's `None`, with an error, if the file is missing,
    /// unreadable or not TOML; otherwise every problem is a warning about
    /// one key, which is left out.
    pub fn load(path: &Path, extensions: &[&str]) -> (Option<ThemeFile>, Vec<Diagnostic>) {
        let layer = load_layer(path);
        // A missing file loads as an empty layer, like an empty file.
        if layer.source.is_empty() && layer.diagnostics.is_empty() && !path.exists() {
            let error = layer.diagnostic_at(Severity::Error, None, 0, "no such file".into());
            return (None, vec![error]);
        }
        Self::from_layer(&layer, extensions)
    }

    /// Parses `source` as the theme file at `path`, as [`ThemeFile::load`]
    /// does.
    pub fn parse(
        path: &Path,
        source: &str,
        extensions: &[&str],
    ) -> (Option<ThemeFile>, Vec<Diagnostic>) {
        Self::from_layer(&parse_layer(path, source.into()), extensions)
    }

    /// Reads a parsed layer, as [`ThemeFile::load`] does.
    pub fn from_layer(layer: &Layer, extensions: &[&str]) -> (Option<ThemeFile>, Vec<Diagnostic>) {
        let mut diagnostics = layer.diagnostics.clone();
        if layer.has_errors() {
            return (None, diagnostics);
        }
        let mut reader = Reader {
            layer,
            diagnostics: &mut diagnostics,
            path: Vec::new(),
        };
        let file = reader.file(extensions);
        // In file order, not the table's sorted-key order.
        diagnostics.sort_by_key(|diagnostic| (diagnostic.line, diagnostic.column));
        (Some(file), diagnostics)
    }
}

/// Walks a layer's table, collecting what it sets and what's wrong.
struct Reader<'a> {
    layer: &'a Layer,
    diagnostics: &'a mut Vec<Diagnostic>,
    /// The key being read.
    path: Vec<String>,
}

impl Reader<'_> {
    fn file(&mut self, extensions: &[&str]) -> ThemeFile {
        let mut file = ThemeFile {
            path: self.layer.path.clone(),
            ..ThemeFile::default()
        };
        let mut aliased = BTreeMap::new();
        for (key, value) in &self.layer.table {
            self.path.push(key.clone());
            match key.as_str() {
                "kiln_theme" => self.version(value),
                "name" => match value.as_str() {
                    Some(name) if !name.trim().is_empty() => file.name = Some(name.to_owned()),
                    _ => self.warn_value("expected a name: a string that isn't empty"),
                },
                "appearance" => match value.as_str() {
                    Some("dark") => file.appearance = Some(Appearance::Dark),
                    Some("light") => file.appearance = Some(Appearance::Light),
                    _ => self.warn_value("expected \"dark\" or \"light\""),
                },
                ALIAS_TABLE => self.colors(value, &mut aliased, |key| {
                    ALIASES
                        .iter()
                        .find(|(alias, _)| *alias == key)
                        .map(|&(_, token)| token)
                }),
                group if TOKEN_GROUPS.contains(&group) => {
                    let mut set = BTreeMap::new();
                    self.colors(value, &mut set, |key| {
                        let token = format!("{group}.{key}");
                        TOKEN_KEYS.iter().copied().find(|known| *known == token)
                    });
                    file.tokens.extend(set);
                }
                name if extensions.contains(&name) => match value {
                    Value::Table(table) => {
                        file.extensions.insert(name.to_owned(), table.clone());
                    }
                    _ => self.wrong_type("a table", value),
                },
                _ => self.unknown(),
            }
            self.path.pop();
        }
        for (token, color) in aliased {
            file.tokens.entry(token).or_insert(color);
        }
        file
    }

    fn version(&mut self, value: &Value) {
        match value.as_integer() {
            Some(THEME_VERSION) => {}
            Some(version) if version > THEME_VERSION => {
                let message = format!(
                    "written for version {version}, but this reads version {THEME_VERSION}: \
                     keys it doesn't know are ignored"
                );
                self.diagnostics.push(self.layer.value_diagnostic(
                    Severity::Warning,
                    &self.path,
                    message,
                ));
            }
            _ => self.warn_value(&format!("expected {THEME_VERSION}")),
        }
    }

    /// Reads a table of colours into `set`, by the token `token` names for
    /// each key.
    fn colors(
        &mut self,
        value: &Value,
        set: &mut BTreeMap<&'static str, Color>,
        token: impl Fn(&str) -> Option<&'static str>,
    ) {
        let Value::Table(table) = value else {
            self.wrong_type("a table", value);
            return;
        };
        for (key, value) in table {
            self.path.push(key.clone());
            match token(key) {
                None => self.unknown(),
                Some(token) => match value {
                    Value::String(text) => match text.parse() {
                        Ok(color) => {
                            set.insert(token, color);
                        }
                        Err(message) => self.warn_value(&message),
                    },
                    _ => self.wrong_type("a colour string", value),
                },
            }
            self.path.pop();
        }
    }

    fn warn_value(&mut self, message: &str) {
        let message = format!("{message}, ignored");
        let diagnostic = self
            .layer
            .value_diagnostic(Severity::Warning, &self.path, message);
        self.diagnostics.push(diagnostic);
    }

    fn wrong_type(&mut self, expected: &str, value: &Value) {
        self.warn_value(&format!("expected {expected}, found {}", value.type_str()));
    }

    fn unknown(&mut self) {
        let diagnostic =
            self.layer
                .key_diagnostic(Severity::Warning, &self.path, "unknown key, ignored".into());
        self.diagnostics.push(diagnostic);
    }
}
