// SPDX-License-Identifier: LicenseRef-Proprietary

//! The gallery's kiln config: its typed commands, its keymap and menus
//! (one TOML file each, for every platform) and the theme to start in.
//! `kiln::init` does the rest.

use kiln::{Config, ui::ThemeMode};

use crate::commands;

/// The app's name, for the menus and the window.
pub(crate) const APP_NAME: &str = "Kiln Gallery";

pub(crate) fn config() -> Config {
    Config::new("dev.kiln.Gallery", APP_NAME)
        .commands(commands::register)
        .keymap("keymap.toml", include_str!("keymap.toml"))
        .menus("menus.toml", include_str!("menus.toml"))
        .theme_mode(initial_mode())
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
