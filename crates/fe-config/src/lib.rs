//! fastedit's configuration, on kiln-core's config machinery: built-in
//! defaults, then the user's
//! `settings.toml`, then the project's, each checked against a JSON schema,
//! with diagnostics that point at the offending token and a watcher for hot
//! reload. Must not depend on GPUI.
//!
//! [`SettingsStore`] holds the effective [`Settings`]; [`ConfigWatcher`]
//! says which files changed, for [`SettingsStore::reload`]; [`write_setting`]
//! changes one setting in a file, keeping its comments. [`EditorPalette`]
//! reads the editor's colours from a kiln theme; kiln's config service
//! selects and hot-reloads the theme itself.

pub mod editorconfig;
pub mod paths;
pub mod recent;
pub mod settings;
pub mod theme;
pub mod write;

// The generic config machinery is kiln's (docs/decisions/app-framework.md);
// fastedit keeps its paths to it.
pub use kiln_core::{diagnostic, layers, trust, watch};

pub use diagnostic::{Diagnostic, Severity};
pub use editorconfig::{EditorConfig, EndOfLine, IndentKind, IndentSize, editorconfig_for};
pub use layers::{Layer, load_layer, merge, parse_layer};
pub use paths::{
    config_dir, config_dir_from, project_settings_path, user_settings_path, user_theme_path,
};
pub use recent::{RecentList, recent_path};
pub use settings::{
    EditorSettings, PROJECT_TERMINAL_SHELL, Settings, SettingsEvent, SettingsStore,
    TerminalSettings, ThemeModeSetting, ThemeSettings, TreeSettings, VimRemap, VimRemapMode,
    VimRemapModes, VimSettings, settings_schema,
};
pub use theme::{Color, EditorColors, EditorPalette};
pub use watch::ConfigWatcher;
pub use write::write_setting;

/// The `settings.toml` a new user starts from: every key commented out at its
/// default, so it changes nothing until edited.
pub const USER_SETTINGS_TEMPLATE: &str = include_str!("../settings-template.toml");

#[cfg(test)]
mod tests;
