// SPDX-License-Identifier: LicenseRef-Proprietary

//! The chat's kiln config: its typed commands, its keymap and menus (one
//! TOML file each, for every platform), the theme to start in, its table
//! in the user's theme file, the user's files and where its layout is
//! saved. `kiln::init` does the rest.

use kiln::{
    Config,
    core::{AppId, AppPaths, PathStyle},
    shell::layout::LayoutStore,
    ui::ThemeMode,
};

use crate::{app, commands, conversation};

/// The app's name, for the menus and the window.
pub(crate) const APP_NAME: &str = "Kiln Chat";

const APP_ID: &str = "dev.kiln.Chat";

pub(crate) fn config() -> Config {
    Config::new(APP_ID, APP_NAME)
        .commands(commands::register)
        .keymap("keymap.toml", include_str!("keymap.toml"))
        .menus("menus.toml", include_str!("menus.toml"))
        // The menus show the shortcuts bound in the window's contexts.
        .menu_shortcut_context([app::KEY_CONTEXT, conversation::KEY_CONTEXT])
        .theme_mode(initial_mode())
        // `[chat]` in the user's theme file: see `conversation::bubble_color`.
        .theme_extensions(&[conversation::THEME_TABLE])
}

/// Where the chat keeps its files: `~/.config/kiln-chat` and
/// `~/.local/state/kiln-chat` on Linux, or `$KILN_CHAT_CONFIG_DIR` and
/// `$KILN_CHAT_STATE_DIR`.
fn paths() -> AppPaths {
    let id = AppId::new(APP_ID, "kiln-chat", APP_NAME);
    AppPaths::new(&id, PathStyle::Native)
}

/// `config` with the user's files, which kiln watches: `keymap.toml` and
/// `theme.toml` in the chat's config directory.
#[allow(
    dead_code,
    reason = "main's: tests/host.rs leaves the user's files out"
)]
pub(crate) fn with_user_files(config: Config) -> Config {
    let paths = paths();
    let Some(dir) = paths.config_dir() else {
        return config;
    };
    config
        .user_keymap(dir.join("keymap.toml"))
        .user_theme(dir.join("theme.toml"))
}

/// The saved window layouts, in the state directory. A chat has no root
/// folder of its own, so the state directory stands for one.
#[allow(dead_code, reason = "main's")]
pub(crate) fn layouts() -> Option<LayoutStore> {
    let paths = paths();
    let (layouts, state) = (paths.layouts_dir()?, paths.state_dir()?);
    Some(LayoutStore::new(layouts, state))
}

/// The theme mode to start in: `KILN_CHAT_THEME` (`dark`, `light` or
/// `system`), or the system's.
fn initial_mode() -> ThemeMode {
    match std::env::var("KILN_CHAT_THEME").as_deref() {
        Ok("dark") => ThemeMode::Dark,
        Ok("light") => ThemeMode::Light,
        _ => ThemeMode::System,
    }
}
