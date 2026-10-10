// SPDX-License-Identifier: LicenseRef-Proprietary

//! Setting up an app: [`application`] for GPUI's platform with kiln's
//! assets, then [`init`] with a [`Config`] before the first window opens.
//!
//! ```ignore
//! kiln::application().run(|cx: &mut App| {
//!     kiln::init(
//!         cx,
//!         kiln::Config::new("com.example.Notes", "Notes")
//!             .commands(notes::register)
//!             .keymap("keymap.toml", include_str!("keymap.toml"))
//!             .menus("menus.toml", include_str!("menus.toml")),
//!     );
//!     cx.open_window(kiln::window_options(&WindowSpec::main("Notes"), cx), |window, cx| {
//!         window.follow_system_appearance(cx);
//!         cx.new(|cx| Notes::new(window, cx))
//!     })
//!     .unwrap();
//! });
//! ```
//!
//! The app's commands are registered through the config, not after
//! `init`, because `init` checks the keymap and menus against every
//! command registered when it runs: one registered later would be dropped
//! from them as unknown. Taking them in the config makes that order the
//! only one there is.

use std::{borrow::Cow, path::PathBuf};

use gpui::{App, Global, SharedString, WindowOptions};
use kiln_core::{CommandCatalog, Diagnostic, Keymap, KeymapLayer, MenuBar, Os, load_keymap_layer};
use kiln_platform::{Capabilities, WindowSpec, commands, keys, menus};
use kiln_ui::{Assets, TextField, Theme, ThemeMode, set_themes};

/// GPUI's application for this platform, with kiln-ui's icons and other
/// assets installed. GPUI's Linux backends (X11 and Wayland) come with
/// kiln, so an app needs no GPUI platform dependency of its own.
pub fn application() -> gpui::Application {
    gpui_platform::application().with_assets(Assets)
}

/// A TOML source and the path its diagnostics name.
struct Source {
    path: PathBuf,
    text: Cow<'static, str>,
}

type Register = Box<dyn FnOnce(&mut App)>;

/// What [`init`] sets up: the app's identity, its typed commands, its one
/// keymap and one menu table, and the theme mode it starts in.
pub struct Config {
    app_id: String,
    app_name: SharedString,
    commands: Vec<Register>,
    keymap: Option<Source>,
    user_keymap: Option<PathBuf>,
    menus: Option<Source>,
    theme_mode: ThemeMode,
}

impl Config {
    /// An app with the reverse-DNS ID `app_id` (`com.example.Notes`), which
    /// the window system matches to its desktop file, and the name its
    /// menus and app commands show (`Hide Notes`). It has no commands,
    /// keymap or menus until they're added, and follows the system's dark
    /// or light appearance.
    pub fn new(app_id: impl Into<String>, app_name: impl Into<SharedString>) -> Self {
        Self {
            app_id: app_id.into(),
            app_name: app_name.into(),
            commands: Vec::new(),
            keymap: None,
            user_keymap: None,
            menus: None,
            theme_mode: ThemeMode::System,
        }
    }

    /// Registers commands when [`init`] runs, before it checks the keymap
    /// and menus: pass the `register` that [`crate::commands!`] writes.
    /// Each call adds one, run in order.
    pub fn commands(mut self, register: impl FnOnce(&mut App) + 'static) -> Self {
        self.commands.push(Box::new(register));
        self
    }

    /// The app's keymap: `source` is its TOML, one file for every platform
    /// (see [`KeymapLayer::parse_for`]), and `path` the name its
    /// diagnostics give it.
    pub fn keymap(
        mut self,
        path: impl Into<PathBuf>,
        source: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.keymap = Some(Source {
            path: path.into(),
            text: source.into(),
        });
        self
    }

    /// The user's keymap file, read over the app's; a missing file is no
    /// bindings, and one with errors is left out.
    pub fn user_keymap(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_keymap = Some(path.into());
        self
    }

    /// The app's menus: `source` is its TOML table, one for every platform
    /// (see [`MenuBar::parse_for`]), and `path` the name its diagnostics
    /// give it.
    pub fn menus(mut self, path: impl Into<PathBuf>, source: impl Into<Cow<'static, str>>) -> Self {
        self.menus = Some(Source {
            path: path.into(),
            text: source.into(),
        });
        self
    }

    /// The theme mode to start in, over kiln's bundled dark and light
    /// themes: [`ThemeMode::System`] unless set.
    pub fn theme_mode(mut self, mode: ThemeMode) -> Self {
        self.theme_mode = mode;
        self
    }

    /// The keymap as on `os`, its bindings checked against `catalog` if
    /// given: the app's, then the user's file over it.
    pub fn keymap_for(
        &self,
        os: Os,
        catalog: Option<&dyn CommandCatalog>,
    ) -> (Keymap, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let mut layers = Vec::new();
        if let Some(source) = &self.keymap {
            let (layer, found) = KeymapLayer::parse_for(&source.path, &source.text, catalog, os);
            layers.extend(layer);
            diagnostics.extend(found);
        }
        if let Some(path) = &self.user_keymap {
            let (layer, found) = load_keymap_layer(path, catalog);
            layers.extend(layer);
            diagnostics.extend(found);
        }
        (Keymap::from_layers(layers), diagnostics)
    }

    /// The menu bar as on `os`, its items checked against `catalog` if
    /// given; empty without a table, or with errors in it.
    pub fn menu_bar_for(
        &self,
        os: Os,
        catalog: Option<&dyn CommandCatalog>,
    ) -> (MenuBar, Vec<Diagnostic>) {
        let Some(source) = &self.menus else {
            return (MenuBar::default(), Vec::new());
        };
        let (bar, diagnostics) = MenuBar::parse_for(&source.path, &source.text, catalog, os);
        (bar.unwrap_or_default(), diagnostics)
    }
}

/// The app's identity, as [`init`] recorded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    /// The reverse-DNS ID: `com.example.Notes`.
    pub id: String,
    pub name: SharedString,
}

impl Global for AppInfo {}

/// Sets kiln up for the app, on this platform, in this order:
///
/// 1. kiln's command dispatch, with the app commands (`app.quit`,
///    `window.close` and the rest) as typed actions too: [`crate::app`],
///    [`crate::window`];
/// 2. the app's own commands, from [`Config::commands`];
/// 3. the keymap and menus, parsed for this platform and checked against
///    every command registered so far, then installed;
/// 4. in-window prompts where the platform has no native ones, and
///    `TextField`'s editing keys for this platform;
/// 5. kiln's dark and light themes, in [`Config::theme_mode`].
///
/// Returns the keymap's and menus' diagnostics, which it has also logged:
/// errors and warnings at those levels.
pub fn init(cx: &mut App, mut config: Config) -> Vec<Diagnostic> {
    commands::init(cx, config.app_name.clone());
    crate::app_commands::init(cx);
    for register in std::mem::take(&mut config.commands) {
        register(cx);
    }
    cx.set_global(AppInfo {
        id: config.app_id.clone(),
        name: config.app_name.clone(),
    });

    let os = Os::current();
    let catalog = commands::catalog(cx);
    let (keymap, mut diagnostics) = config.keymap_for(os, Some(&*catalog));
    keys::init(cx, keymap);
    let (menu_bar, menu_diagnostics) = config.menu_bar_for(os, Some(&*catalog));
    diagnostics.extend(menu_diagnostics);
    menus::install(cx, menu_bar);
    for diagnostic in &diagnostics {
        if diagnostic.is_error() {
            log::error!("{diagnostic}");
        } else {
            log::warn!("{diagnostic}");
        }
    }

    kiln_shell::init(cx);
    cx.bind_keys(TextField::key_bindings(os == Os::MacOs));
    set_themes(cx, config.theme_mode, Theme::dark(), Theme::light());
    diagnostics
}

/// GPUI's options for a window as `spec` describes it, on this platform,
/// with the app ID [`init`] recorded if `spec` names none.
pub fn window_options(spec: &WindowSpec, cx: &App) -> WindowOptions {
    let mut spec = spec.clone();
    if spec.app_id.is_none() {
        spec.app_id = cx.try_global::<AppInfo>().map(|info| info.id.clone());
    }
    kiln_platform::window_options(&spec, &Capabilities::current(), cx)
}
