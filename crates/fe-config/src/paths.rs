//! Where fastedit's config files live: kiln-core's [`AppPaths`] for the
//! `fastedit` app, in the XDG layout on every platform, so they stay where
//! they've always been (`~/.config/fastedit`, `~/.local/state/fastedit`).

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use kiln_core::{AppId, AppPaths, Os, PathStyle};

pub use kiln_core::paths::{SETTINGS_FILE, TRUST_FILE};

/// The user's theme file's name, beside [`SETTINGS_FILE`].
pub const THEME_FILE: &str = "theme.toml";

/// The user's vimrc's name, beside [`SETTINGS_FILE`].
pub const VIMRC_FILE: &str = "vimrc";

/// The name people see: the app menu, window titles and prompts.
pub const APP_NAME: &str = "Tenmoku";

/// Tenmoku, as kiln knows it. Its reverse DNS and short name still key the
/// config and state directories as `fastedit`.
pub fn app_id() -> AppId {
    AppId::new("dev.fastedit.fastedit", "fastedit", APP_NAME)
}

/// Tenmoku's directories, reading variables through `env`.
pub fn app_paths_from(env: impl Fn(&str) -> Option<OsString>) -> AppPaths {
    AppPaths::resolve(&app_id(), PathStyle::Xdg, Os::current(), env)
}

/// Tenmoku's directories, from the process environment.
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

/// `<config_dir>/theme.toml`, beside `settings.toml`: the user's own
/// colours, which kiln's config service hot-reloads and lays over the dark
/// or the light theme as its `appearance` says.
pub fn user_theme_path() -> Option<PathBuf> {
    Some(config_dir()?.join(THEME_FILE))
}

/// `<config_dir>/vimrc`, beside `settings.toml`: the user's vim remaps,
/// in the subset of vimrc that `fe_vim::parse_vimrc` reads.
pub fn user_vimrc_path() -> Option<PathBuf> {
    Some(config_dir()?.join(VIMRC_FILE))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_vimrc_path_is_in_the_config_dir() {
        let expected = config_dir().map(|dir| dir.join("vimrc"));
        assert_eq!(user_vimrc_path(), expected);
        assert_eq!(
            user_vimrc_path().map(|path| path.with_file_name(SETTINGS_FILE)),
            user_settings_path(),
            "beside settings.toml"
        );
    }
}
