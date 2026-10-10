//! Where fastedit's config files live: kiln-core's [`AppPaths`] for the
//! `fastedit` app, in the XDG layout on every platform, so they stay where
//! they've always been (`~/.config/fastedit`, `~/.local/state/fastedit`).

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use kiln_core::{AppId, AppPaths, Os, PathStyle};

pub use kiln_core::paths::{SETTINGS_FILE, THEMES_DIR, TRUST_FILE};

/// fastedit, as kiln knows it.
pub fn app_id() -> AppId {
    AppId::new("dev.fastedit.fastedit", "fastedit", "fastedit")
}

/// fastedit's directories, reading variables through `env`.
pub fn app_paths_from(env: impl Fn(&str) -> Option<OsString>) -> AppPaths {
    AppPaths::resolve(&app_id(), PathStyle::Xdg, Os::current(), env)
}

/// fastedit's directories, from the process environment.
pub fn app_paths() -> AppPaths {
    app_paths_from(|name| std::env::var_os(name))
}

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
    app_paths_from(env).config_dir().map(Path::to_owned)
}

/// `<config_dir>/settings.toml`.
pub fn user_settings_path() -> Option<PathBuf> {
    app_paths().settings_file()
}

/// `<config_dir>/themes`, from the process environment: see
/// [`crate::theme::load_themes`].
pub fn themes_dir() -> Option<PathBuf> {
    themes_dir_from(|name| std::env::var_os(name))
}

/// `<config_dir>/themes`, reading variables through `env` as
/// [`config_dir_from`] does.
pub fn themes_dir_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    app_paths_from(env).themes_dir()
}

/// `<root>/.fastedit/settings.toml`.
pub fn project_settings_path(root: &Path) -> PathBuf {
    app_paths().project_settings_file(root)
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
    app_paths_from(env).state_dir().map(Path::to_owned)
}

/// `<state_dir>/trust.toml`: see [`crate::trust`].
pub fn trust_path() -> Option<PathBuf> {
    app_paths().trust_file()
}
