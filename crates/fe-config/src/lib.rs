//! Layered TOML configuration: built-in defaults, then the user's
//! `settings.toml`, then the project's, each checked against a JSON schema,
//! with diagnostics that point at the offending token and a watcher for hot
//! reload. Must not depend on GPUI.
//!
//! [`SettingsStore`] holds the effective [`Settings`]; [`ConfigWatcher`]
//! says which files changed, for [`SettingsStore::reload`].

pub mod diagnostic;
pub mod layers;
pub mod paths;
pub mod settings;
pub mod watch;

pub use diagnostic::{Diagnostic, Severity};
pub use layers::{Layer, load_layer, merge};
pub use paths::{config_dir, config_dir_from, project_settings_path, user_settings_path};
pub use settings::{EditorSettings, Settings, SettingsEvent, SettingsStore, settings_schema};
pub use watch::ConfigWatcher;

/// The `settings.toml` a new user starts from: every key commented out at its
/// default, so it changes nothing until edited.
pub const USER_SETTINGS_TEMPLATE: &str = include_str!("../settings-template.toml");

#[cfg(test)]
mod tests;
