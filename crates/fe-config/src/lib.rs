//! fastedit's configuration, on kiln-core's config machinery: built-in
//! defaults, then the user's
//! `settings.toml`, then the project's, each checked against a JSON schema,
//! with diagnostics that point at the offending token and a watcher for hot
//! reload. Must not depend on GPUI.
//!
//! [`SettingsStore`] holds the effective [`Settings`]; [`ConfigWatcher`]
//! says which files changed, for [`SettingsStore::reload`]; [`write_setting`]
//! (and [`write_setting_str`], [`write_settings`])
//! changes one setting in a file, keeping its comments. [`EditorPalette`]
//! reads the editor's colours from a kiln theme; kiln's config service
//! selects and hot-reloads the theme itself. [`themes`] lists the themes
//! fastedit bundles, which `[theme] name` selects, and [`chat_models`] the
//! Claude models it knows. [`SettingsModel`] lists
//! every setting with its values, for the Settings editor, and
//! [`set_setting`] and [`reset_setting`] write one into the user's file or
//! the project's.

pub mod chat_models;
pub mod editorconfig;
pub mod paths;
pub mod recent;
pub mod settings;
pub mod settings_edit;
pub mod settings_model;
pub mod theme;
pub mod themes;
pub mod write;

// The generic config machinery is kiln's (docs/decisions/app-framework.md);
// fastedit keeps its paths to it.
pub use kiln_core::{diagnostic, layers, trust, watch};

pub use diagnostic::{Diagnostic, Severity};
pub use editorconfig::{EditorConfig, EndOfLine, IndentKind, IndentSize, editorconfig_for};
pub use layers::{Layer, load_layer, merge, parse_layer};
pub use paths::{
    APP_NAME, config_dir, config_dir_from, project_settings_path, user_settings_path,
    user_theme_path,
};
pub use recent::{RecentList, recent_path};
pub use settings::{
    ChatEffort, ChatSettings, EditorSettings, ExtensionsSettings, FormatterPrefer,
    FormatterSettings, PROJECT_CHAT, PROJECT_EXTENSIONS, PROJECT_FORMATTERS,
    PROJECT_TERMINAL_SHELL, PROJECT_VIM_USER_ONLY, Settings, SettingsEvent, SettingsStore,
    StickyScrollModel, StickyScrollSettings, TerminalSettings, ThemeModeSetting, ThemeSettings,
    TreeSettings, VimJumpStyle, VimPreset, VimRemap, VimRemapMode, VimRemapModes, VimSettings,
    settings_schema,
};
pub use settings_edit::{SettingEditError, SettingScope, reset_setting, set_setting};
pub use settings_model::{
    ChoiceSource, EffectiveValue, EnumVariant, SettingEntry, SettingGroup, SettingKind,
    SettingsModel, ValueSource,
};
pub use theme::{Color, EditorColors, EditorPalette};
pub use watch::ConfigWatcher;
pub use write::{SettingValue, write_setting, write_setting_str, write_settings};

/// The `settings.toml` a new user starts from: every key commented out at its
/// default, so it changes nothing until edited.
pub const USER_SETTINGS_TEMPLATE: &str = include_str!("../settings-template.toml");

#[cfg(test)]
mod tests;
