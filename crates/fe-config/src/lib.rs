//! fastedit's configuration, on kiln-core's config machinery: built-in
//! defaults, then the user's
//! `settings.toml`, then the project's, each checked against a JSON schema,
//! with diagnostics that point at the offending token and a watcher for hot
//! reload. Must not depend on GPUI.
//!
//! [`SettingsStore`] holds the effective [`Settings`]; [`ConfigWatcher`]
//! says which files changed, for [`SettingsStore::reload`]; [`write_setting`]
//! changes one setting in a file, keeping its comments. [`load_themes`]
//! reads the colour themes, and [`ThemeStore`] holds the selected one
//! across reloads.

pub mod paths;
pub mod settings;
pub mod theme;
pub mod theme_store;
pub mod write;

// The generic config machinery is kiln's (docs/decisions/app-framework.md);
// fastedit keeps its paths to it.
pub use kiln_core::{diagnostic, layers, trust, watch};

pub use diagnostic::{Diagnostic, Severity};
pub use layers::{Layer, load_layer, merge, parse_layer};
pub use paths::{
    config_dir, config_dir_from, project_settings_path, themes_dir, themes_dir_from,
    user_settings_path,
};
pub use settings::{EditorSettings, Settings, SettingsEvent, SettingsStore, settings_schema};
pub use theme::{Appearance, Color, EditorColors, Theme, ThemeSet, UiColors, load_themes};
pub use theme_store::{ThemeEvent, ThemeStore};
pub use watch::ConfigWatcher;
pub use write::write_setting;

/// The `settings.toml` a new user starts from: every key commented out at its
/// default, so it changes nothing until edited.
pub const USER_SETTINGS_TEMPLATE: &str = include_str!("../settings-template.toml");

#[cfg(test)]
mod tests;
