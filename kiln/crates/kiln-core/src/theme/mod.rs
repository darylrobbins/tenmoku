// SPDX-License-Identifier: Apache-2.0

//! Themes as data: semantic colour [`Tokens`] read from a TOML file
//! ([`ThemeFile`]) and laid over a bundled dark or light base
//! ([`ThemeData`]). Turning them into a toolkit's colours is kiln-ui's job,
//! so this stays GPUI-free.
//!
//! A file sets as few tokens as it likes. A token it leaves out keeps the
//! base's colour, unless it's derived ([`RULES`]) from a token the file
//! set: a theme that changes only `accent.base` moves the selection, the
//! focus ring and the drop marker with it. An app's own tables, such as an
//! editor's colours, ride along as [`ThemeData::extensions`].

mod color;
mod file;
mod tokens;

use std::{collections::BTreeMap, path::Path, sync::LazyLock};

pub use color::Color;
pub use file::{ALIAS_TABLE, ALIASES, THEME_VERSION, ThemeFile};
pub use tokens::{
    Accent, Agent, Border, List, RULES, Rule, SOFT, Scrollbar, State, Status, Surface,
    TOKEN_GROUPS, TOKEN_KEYS, Tab, Text, Tokens, Vcs, resolve, rule,
};
use toml::Table;

/// Whether a theme is light text on dark or the other way round, which
/// picks the base it's laid over.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}

/// The bundled dark base's source.
pub const DARK_SOURCE: &str = include_str!("../../themes/dark.toml");
/// The bundled light base's source.
pub const LIGHT_SOURCE: &str = include_str!("../../themes/light.toml");

/// A theme, every token resolved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeData {
    pub name: String,
    pub appearance: Appearance,
    pub tokens: Tokens,
    /// The file's app tables by name, as [`ThemeFile::extensions`].
    pub extensions: BTreeMap<String, Table>,
}

static DARK: LazyLock<ThemeData> = LazyLock::new(|| bundled("dark.toml", DARK_SOURCE));
static LIGHT: LazyLock<ThemeData> = LazyLock::new(|| bundled("light.toml", LIGHT_SOURCE));

/// A bundled base: its roots as written, every other token derived.
fn bundled(file_name: &str, source: &str) -> ThemeData {
    let (file, diagnostics) = ThemeFile::parse(Path::new(file_name), source, &[]);
    debug_assert_eq!(diagnostics, [], "the bundled theme has problems");
    let file = file.unwrap_or_default();
    ThemeData {
        name: file.name.clone().unwrap_or_default(),
        appearance: file.appearance.unwrap_or_default(),
        tokens: resolve(file.tokens.iter().map(|(key, color)| (*key, *color)), None),
        extensions: file.extensions,
    }
}

impl ThemeData {
    /// The bundled dark base.
    pub fn dark() -> &'static ThemeData {
        &DARK
    }

    /// The bundled light base.
    pub fn light() -> &'static ThemeData {
        &LIGHT
    }

    /// The bundled base for `appearance`.
    pub fn base(appearance: Appearance) -> &'static ThemeData {
        match appearance {
            Appearance::Dark => Self::dark(),
            Appearance::Light => Self::light(),
        }
    }

    /// `file` laid over the bundled base for its appearance (dark if it
    /// doesn't say): [`ThemeData::from_file_over`].
    pub fn from_file(file: &ThemeFile) -> ThemeData {
        Self::from_file_over(file, Self::base(file.appearance.unwrap_or_default()))
    }

    /// `file` laid over `base`, such as an installed theme the user chose:
    /// a token it leaves out keeps `base`'s colour ([`resolve`]), and each
    /// app table it has adds its keys to `base`'s, replacing those both
    /// set. A file without an appearance takes `base`'s, and one without
    /// a name its file's stem.
    pub fn from_file_over(file: &ThemeFile, base: &ThemeData) -> ThemeData {
        let name = file.name.clone().or_else(|| {
            file.path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .filter(|stem| !stem.is_empty())
        });
        let mut extensions = base.extensions.clone();
        for (table, keys) in &file.extensions {
            let merged = extensions.entry(table.clone()).or_default();
            merged.extend(keys.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
        ThemeData {
            name: name.unwrap_or_else(|| base.name.clone()),
            appearance: file.appearance.unwrap_or(base.appearance),
            tokens: resolve(
                file.tokens.iter().map(|(key, color)| (*key, *color)),
                Some(&base.tokens),
            ),
            extensions,
        }
    }

    /// The app table called `name`, if the theme has one.
    pub fn extension(&self, name: &str) -> Option<&Table> {
        self.extensions.get(name)
    }
}
