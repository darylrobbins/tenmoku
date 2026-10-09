//! Layered TOML configuration: built-in defaults, then the user's
//! `settings.toml`, then the project's, each checked against a JSON schema,
//! with diagnostics that point at the offending token and a watcher for hot
//! reload. Must not depend on GPUI.
//!
//! [`SettingsStore`] holds the effective [`Settings`]; [`ConfigWatcher`]
//! says which files changed, for [`SettingsStore::reload`]; [`write_setting`]
//! changes one setting in a file, keeping its comments. [`load_themes`]
//! reads the colour themes.

pub mod diagnostic;
pub mod layers;
pub mod paths;
pub mod settings;
pub mod theme;
pub mod trust;
pub mod watch;
pub mod write;

pub use diagnostic::{Diagnostic, Severity};
pub use layers::{Layer, load_layer, merge, parse_layer};
pub use paths::{
    config_dir, config_dir_from, project_settings_path, themes_dir, themes_dir_from,
    user_settings_path,
};
pub use settings::{EditorSettings, Settings, SettingsEvent, SettingsStore, settings_schema};
pub use theme::{Appearance, Color, EditorColors, Theme, ThemeSet, UiColors, load_themes};
pub use watch::ConfigWatcher;
pub use write::write_setting;

/// The `settings.toml` a new user starts from: every key commented out at its
/// default, so it changes nothing until edited.
pub const USER_SETTINGS_TEMPLATE: &str = include_str!("../settings-template.toml");

#[cfg(test)]
mod tests;
