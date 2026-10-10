// SPDX-License-Identifier: LicenseRef-Proprietary

//! Everything the gallery sets up before its window opens: kiln's command
//! dispatch, its typed commands, the keymap and menus (one TOML file each,
//! for every platform), prompts, the text field's keys and the themes.

use std::path::Path;

use gpui::App;
use kiln::{
    core::{Diagnostic, Keymap, KeymapLayer, MenuBar, Os},
    platform::{Capabilities, commands as kiln_commands, keys, menus},
    ui::{TextField, Theme, ThemeMode, set_themes},
};

use crate::commands;

/// The app's name, for the menus and the window.
pub(crate) const APP_NAME: &str = "Kiln Gallery";

/// The keymap, for every platform.
pub(crate) const KEYMAP: &str = include_str!("keymap.toml");

/// The menus, for every platform.
pub(crate) const MENUS: &str = include_str!("menus.toml");

/// Sets up kiln for the gallery, as on this platform.
pub(crate) fn init(cx: &mut App) {
    kiln_commands::init(cx, APP_NAME);
    commands::register(cx);
    cx.on_action(|_: &commands::Quit, cx| cx.quit());

    let catalog = kiln_commands::catalog(cx);
    let (keymap, diagnostics) = keymap(Os::current(), Some(&*catalog));
    report(&diagnostics);
    keys::init(cx, keymap);
    let (menu_bar, diagnostics) = menu_bar(Os::current(), Some(&*catalog));
    report(&diagnostics);
    menus::install(cx, menu_bar);

    // In-window prompts where the platform has no native alerts.
    kiln::shell::init(cx);
    // TextField's editing keys are GPUI bindings, which kiln's keymap
    // leaves alone.
    let macos = Capabilities::current().os == Os::MacOs;
    cx.bind_keys(TextField::key_bindings(macos));

    set_themes(cx, initial_mode(), Theme::dark(), Theme::light());
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

/// The gallery's keymap as on `os`, checked against `catalog` if given.
pub(crate) fn keymap(
    os: Os,
    catalog: Option<&dyn kiln::core::CommandCatalog>,
) -> (Keymap, Vec<Diagnostic>) {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new("keymap.toml"), KEYMAP, catalog, os);
    (Keymap::from_layers(layer), diagnostics)
}

/// The gallery's menus as on `os`, checked against `catalog` if given.
pub(crate) fn menu_bar(
    os: Os,
    catalog: Option<&dyn kiln::core::CommandCatalog>,
) -> (MenuBar, Vec<Diagnostic>) {
    let (bar, diagnostics) = MenuBar::parse_for(Path::new("menus.toml"), MENUS, catalog, os);
    (bar.unwrap_or_default(), diagnostics)
}

fn report(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        eprintln!("{diagnostic}");
    }
}
