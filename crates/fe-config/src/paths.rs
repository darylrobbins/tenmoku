//! Where config files live.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// The settings file's name, in the user and project config directories.
pub const SETTINGS_FILE: &str = "settings.toml";

/// The workspace trust file's name, in the state directory.
pub const TRUST_FILE: &str = "trust.toml";

/// The user config directory, from the process environment: see
/// [`config_dir_from`]. `None` with none of its variables set.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(|name| std::env::var_os(name))
}

/// The user config directory, reading variables through `env`:
///
/// 1. `$FASTEDIT_CONFIG_DIR`
/// 2. `$XDG_CONFIG_HOME/fastedit`
/// 3. `$HOME/.config/fastedit`
///
/// Empty variables count as unset, and so does a relative
/// `$XDG_CONFIG_HOME`, as the XDG spec says.
pub fn config_dir_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |name: &str| {
        env(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = var("FASTEDIT_CONFIG_DIR") {
        return Some(dir);
    }
    if let Some(dir) = var("XDG_CONFIG_HOME").filter(|dir| dir.is_absolute()) {
        return Some(dir.join("fastedit"));
    }
    var("HOME").map(|home| home.join(".config").join("fastedit"))
}

/// `<config_dir>/settings.toml`.
pub fn user_settings_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(SETTINGS_FILE))
}

/// `<root>/.fastedit/settings.toml`.
pub fn project_settings_path(root: &Path) -> PathBuf {
    root.join(".fastedit").join(SETTINGS_FILE)
}

/// The user state directory, from the process environment: see
/// [`state_dir_from`]. `None` with none of its variables set.
pub fn state_dir() -> Option<PathBuf> {
    state_dir_from(|name| std::env::var_os(name))
}

/// The user state directory, for what fastedit records about this machine
/// (not settings to sync), reading variables through `env`:
///
/// 1. `$FASTEDIT_STATE_DIR`
/// 2. `$XDG_STATE_HOME/fastedit`
/// 3. `$HOME/.local/state/fastedit`
///
/// Empty variables count as unset, and so does a relative
/// `$XDG_STATE_HOME`, as the XDG spec says.
pub fn state_dir_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |name: &str| {
        env(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = var("FASTEDIT_STATE_DIR") {
        return Some(dir);
    }
    if let Some(dir) = var("XDG_STATE_HOME").filter(|dir| dir.is_absolute()) {
        return Some(dir.join("fastedit"));
    }
    var("HOME").map(|home| home.join(".local").join("state").join("fastedit"))
}

/// `<state_dir>/trust.toml`: see [`crate::trust`].
pub fn trust_path() -> Option<PathBuf> {
    state_dir().map(|dir| dir.join(TRUST_FILE))
}
