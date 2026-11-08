//! The themes Tenmoku bundles, which `[theme] name` selects by title or
//! ID: "Dark Modern" or "Ember", say, with no theme file to copy. The
//! titles are this table's, whatever each file's own `name` says.

use std::path::Path;

use kiln_core::theme::{Appearance, ThemeFile};

use crate::theme::EXTENSIONS;

/// A theme in Tenmoku's binary: [`bundled_themes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BundledTheme {
    /// What `[theme] name` calls it: "Dark Modern".
    pub title: &'static str,
    /// Its file's stem, which `[theme] name` takes too:
    /// "vscode-dark-modern".
    pub id: &'static str,
    pub appearance: Appearance,
    /// The theme file, with Tenmoku's tables ([`EXTENSIONS`]).
    pub source: &'static str,
}

impl BundledTheme {
    /// The theme file, parsed with Tenmoku's tables.
    pub fn file(&self) -> ThemeFile {
        let path = format!("{}.toml", self.id);
        let (file, diagnostics) = ThemeFile::parse(Path::new(&path), self.source, EXTENSIONS);
        debug_assert_eq!(
            diagnostics,
            [],
            "the bundled theme {} has problems",
            self.id
        );
        file.unwrap_or_default()
    }
}

macro_rules! bundled {
    ($title:literal, $id:literal, $appearance:ident) => {
        BundledTheme {
            title: $title,
            id: $id,
            appearance: Appearance::$appearance,
            source: include_str!(concat!("../themes/", $id, ".toml")),
        }
    };
}

const BUNDLED: &[BundledTheme] = &[
    bundled!("Dark Modern", "vscode-dark-modern", Dark),
    bundled!("Dark 2026", "vscode-2026-dark", Dark),
    bundled!("Light 2026", "vscode-2026-light", Light),
    bundled!("Catppuccin Latte", "catppuccin-latte", Light),
    bundled!("Catppuccin Frappe", "catppuccin-frappe", Dark),
    bundled!("Catppuccin Macchiato", "catppuccin-macchiato", Dark),
    bundled!("Catppuccin Mocha", "catppuccin-mocha", Dark),
    bundled!("Monokai", "vscode-monokai", Dark),
    bundled!("Tenmoku Dark", "tenmoku-dark", Dark),
    bundled!("Tenmoku Light", "tenmoku-light", Light),
    bundled!("Tenmoku Ember", "tenmoku-ember", Dark),
    bundled!("Yohen", "yohen", Dark),
    bundled!("Hakuji", "hakuji", Light),
];

/// Every bundled theme, in the order a list of them shows.
pub fn bundled_themes() -> &'static [BundledTheme] {
    BUNDLED
}

/// The bundled theme `name` is the title or ID of, ignoring case and the
/// space around it. A title's "Tenmoku " may be left out: "Ember".
pub fn find(name: &str) -> Option<&'static BundledTheme> {
    let name = name.trim();
    BUNDLED.iter().find(|theme| {
        let short = theme.title.strip_prefix("Tenmoku ");
        [Some(theme.title), short, Some(theme.id)]
            .into_iter()
            .flatten()
            .any(|known| known.eq_ignore_ascii_case(name))
    })
}
