// SPDX-License-Identifier: Apache-2.0

//! The gallery's kiln config: its typed commands, its keymap and menus
//! (one TOML file each, for every platform), the theme to start in and the
//! user's files. `kiln::init` does the rest.

use kiln::{
    Config,
    core::{AppId, AppPaths, PathStyle},
    ui::ThemeMode,
};

use crate::{commands, settings::GallerySettings};

/// The app's name, for the menus and the window.
pub(crate) const APP_NAME: &str = "Kiln Gallery";

pub(crate) fn config() -> Config {
    Config::new("dev.kiln.Gallery", APP_NAME)
        .commands(commands::register)
        .keymap("keymap.toml", include_str!("keymap.toml"))
        .menus("menus.toml", include_str!("menus.toml"))
        .theme_mode(initial_mode())
}

/// `config` with the user's files, which kiln watches: `settings.toml`
/// (the [`GallerySettings`]), `keymap.toml` and `theme.toml`, in the
/// gallery's config directory (`~/.config/kiln-gallery` on Linux, or
/// `$KILN_GALLERY_CONFIG_DIR`).
#[allow(
    dead_code,
    reason = "main's: tests/gallery.rs leaves the user's files out"
)]
pub(crate) fn with_user_files(config: Config) -> Config {
    let id = AppId::new("dev.kiln.Gallery", "kiln-gallery", APP_NAME);
    let paths = AppPaths::new(&id, PathStyle::Native);
    let Some(dir) = paths.config_dir() else {
        return config;
    };
    config
        .settings::<GallerySettings>(paths.settings_file(), None)
        .user_keymap(dir.join("keymap.toml"))
        .user_theme(dir.join("theme.toml"))
}

/// The theme mode to start in: `KILN_GALLERY_THEME` (`dark`, `light` or
/// `system`), or the system's.
fn initial_mode() -> ThemeMode {
    match std::env::var("KILN_GALLERY_THEME").as_deref() {
        Ok("dark") => ThemeMode::Dark,
        Ok("light") => ThemeMode::Light,
        _ => ThemeMode::System,
    }
}
