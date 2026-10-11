// SPDX-License-Identifier: LicenseRef-Proprietary

//! Setting up an app: [`application`] for GPUI's platform with kiln's
//! assets, then [`init`] with a [`Config`] before the first window opens.
//!
//! ```ignore
//! kiln::application().run(|cx: &mut App| {
//!     let id = AppId::new("com.example.Notes", "notes", "Notes");
//!     let paths = AppPaths::new(&id, PathStyle::Native);
//!     kiln::init(
//!         cx,
//!         kiln::Config::new("com.example.Notes", "Notes")
//!             .commands(notes::register)
//!             .keymap("keymap.toml", include_str!("keymap.toml"))
//!             .menus("menus.toml", include_str!("menus.toml"))
//!             .settings::<NotesSettings>(paths.settings_file(), None)
//!             .user_keymap(paths.keymap_file().unwrap())
//!             .user_theme(paths.config_dir().unwrap().join("theme.toml")),
//!     );
//!     cx.open_window(kiln::window_options(&WindowSpec::main("Notes"), cx), |window, cx| {
//!         let notes = cx.new(|cx| Notes::new(window, cx));
//!         // The platform's chrome, and each window's setup.
//!         cx.new(|cx| kiln::Root::new(notes, window, cx).title("Notes"))
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
//!
//! The user's files (settings, keymap and theme) are watched from `init`
//! on, and each change applies to every window: see
//! [`kiln_platform::config`].

use std::{borrow::Cow, path::PathBuf, rc::Rc};

use gpui::{App, Global, SharedString, Task, WindowOptions};
use kiln_command::{CommandId, Value};
use kiln_core::{
    CatalogError, CommandCatalog, Diagnostic, Keymap, KeymapLayer, KeymapStore, MenuBar, Os,
    SettingsStore, SettingsType, load_keymap_layer,
};
use kiln_platform::{
    Capabilities, WindowSpec, commands,
    config::{
        Batches, ConfigService, ConfigSource as _, DEFAULT_LOG_TARGET, batch_channel,
        log_diagnostics_as,
    },
    keys, menus,
};
use kiln_ui::{Assets, TextField, Theme, ThemeMode, set_themes};

use crate::config::UserTheme;

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

/// Called with the app after something changed.
type Hook = Box<dyn FnMut(&mut App)>;

/// Adds a settings store to the service, returning its diagnostics.
type AddSettings = Box<dyn FnOnce(ConfigService) -> (ConfigService, Vec<Diagnostic>)>;

/// Each catalog in turn, until one knows the command.
struct Catalogs(Vec<Rc<dyn CommandCatalog>>);

impl CommandCatalog for Catalogs {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        for catalog in &self.0 {
            match catalog.check(command, args) {
                Err(CatalogError::UnknownCommand) => {}
                result => return result,
            }
        }
        Err(CatalogError::UnknownCommand)
    }
}

/// What [`init`] sets up: the app's identity, its typed commands, its one
/// keymap and one menu table, the theme mode it starts in, and the user's
/// files it watches: settings, keymap and theme.
pub struct Config {
    app_id: String,
    app_name: SharedString,
    commands: Vec<Register>,
    catalogs: Vec<Rc<dyn CommandCatalog>>,
    keymap: Option<Source>,
    user_keymap: Option<PathBuf>,
    keymap_reloaded: Option<Hook>,
    menus: Option<Source>,
    menu_shortcut_context: Vec<String>,
    theme_mode: ThemeMode,
    settings: Vec<AddSettings>,
    user_theme: Option<PathBuf>,
    batches: Option<Batches>,
    log_target: Option<&'static str>,
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
            catalogs: Vec::new(),
            keymap: None,
            user_keymap: None,
            keymap_reloaded: None,
            menus: None,
            menu_shortcut_context: Vec::new(),
            theme_mode: ThemeMode::System,
            settings: Vec::new(),
            user_theme: None,
            batches: None,
            log_target: None,
        }
    }

    /// Registers commands when [`init`] runs, before it checks the keymap
    /// and menus: pass the `register` that [`crate::commands!`] writes.
    /// Each call adds one, run in order.
    pub fn commands(mut self, register: impl FnOnce(&mut App) + 'static) -> Self {
        self.commands.push(Box::new(register));
        self
    }

    /// Commands kiln's registries don't hold, that the keymap and menus may
    /// name too: those an app's views run from their own registries when a
    /// [`kiln_platform::commands::RunCommand`] reaches them. [`init`]
    /// checks bindings and menu items against kiln's commands and the
    /// app's typed ones first, then each of these in turn.
    pub fn catalog(mut self, catalog: impl CommandCatalog + 'static) -> Self {
        self.catalogs.push(Rc::new(catalog));
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

    /// The user's keymap file, read over the app's and reloaded when it
    /// changes; a missing file is no bindings, and one with errors keeps
    /// its last good version (none, at first).
    pub fn user_keymap(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_keymap = Some(path.into());
        self
    }

    /// Calls `reloaded` after each reload of the user's keymap that
    /// changes the keymap in use, once it's installed and the menus show
    /// its shortcuts: to report on it, say.
    pub fn on_keymap_reload(mut self, reloaded: impl FnMut(&mut App) + 'static) -> Self {
        self.keymap_reloaded = Some(Box::new(reloaded));
        self
    }

    /// The app's settings, of its own type `S`: the user's file, then the
    /// project's, over `S::default()`, either file absent or missing, and
    /// reloaded when they change. Read them with `cx.settings::<S>()`
    /// ([`crate::prelude::AppExt::settings`]) and follow changes with
    /// `cx.observe_global::<GlobalSettings<S>>(..)`. A file with errors
    /// keeps its last good version. Each type is its own store: call it
    /// once for each.
    pub fn settings<S: SettingsType>(
        self,
        user: Option<PathBuf>,
        project: Option<PathBuf>,
    ) -> Self {
        self.settings_store(SettingsStore::<S>::load(user, project))
    }

    /// The app's settings, of its own type `S`, as [`Config::settings`]
    /// has them, from a store the app has loaded: one with a schema or a
    /// project filter ([`kiln_core::SettingsStore::load_with`]), or whose
    /// settings it read before `init`.
    pub fn settings_store<S: SettingsType>(mut self, store: SettingsStore<S>) -> Self {
        self.settings.push(Box::new(move |service| {
            let diagnostics = store.diagnostics().to_vec();
            (service.settings(store), diagnostics)
        }));
        self
    }

    /// The user's theme file, reloaded when it changes. Laid over kiln's
    /// dark or light base, as its `appearance` says, it takes that one's
    /// place, so [`Config::theme_mode`] and `cx.set_theme_mode` still pick
    /// between dark and light. A missing file is no theme of the user's,
    /// and one with errors keeps its last good version.
    pub fn user_theme(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_theme = Some(path.into());
        self
    }

    /// Takes the changed files from `batches` instead of watching them:
    /// for tests, which send what the watcher would
    /// ([`kiln_platform::config::batch_channel`]).
    pub fn watch_with(mut self, batches: Batches) -> Self {
        self.batches = Some(batches);
        self
    }

    /// Logs the config's diagnostics and reloads under `target`, such as
    /// the app's crate name, so the app's logger treats them as its own:
    /// see [`kiln_platform::config::log_target`]. Unset, they're logged
    /// under `kiln_platform::config`.
    pub fn log_target(mut self, target: &'static str) -> Self {
        self.log_target = Some(target);
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

    /// The frames a menu item's shortcut is resolved in when it names no
    /// `shortcut_context` of its own: those the app's main view usually
    /// has, such as `["main", "editor"]`, so a binding limited to them
    /// shows in the menus (see [`MenuBar::parse_in`]). None unless set.
    pub fn menu_shortcut_context<T: Into<String>>(
        mut self,
        frames: impl IntoIterator<Item = T>,
    ) -> Self {
        self.menu_shortcut_context = frames.into_iter().map(Into::into).collect();
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

    /// The keymap store [`init`] reloads: the app's keymap parsed for `os`,
    /// then the user's file.
    fn keymap_store(&self, os: Os, catalog: Rc<dyn CommandCatalog>) -> KeymapStore {
        let (layer, diagnostics) = match &self.keymap {
            Some(source) => KeymapLayer::parse_for(&source.path, &source.text, Some(&*catalog), os),
            None => (None, Vec::new()),
        };
        KeymapStore::with_default(
            layer,
            diagnostics,
            self.user_keymap.clone(),
            Some(Box::new(catalog)),
        )
    }

    /// The menu bar as on `os`, its items checked against `catalog` if
    /// given, their shortcuts resolved in [`Config::menu_shortcut_context`];
    /// empty without a table, or with errors in it.
    pub fn menu_bar_for(
        &self,
        os: Os,
        catalog: Option<&dyn CommandCatalog>,
    ) -> (MenuBar, Vec<Diagnostic>) {
        let Some(source) = &self.menus else {
            return (MenuBar::default(), Vec::new());
        };
        let frames: Vec<&str> = self
            .menu_shortcut_context
            .iter()
            .map(String::as_str)
            .collect();
        let (bar, diagnostics) =
            MenuBar::parse_in(&source.path, &source.text, catalog, os, &frames);
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

/// The config service's task, held for as long as the app runs.
struct Watching(#[allow(dead_code, reason = "held, not read")] Task<()>);

impl Global for Watching {}

/// Sets kiln up for the app, on this platform, in this order:
///
/// 1. kiln's command dispatch, with the app commands (`app.quit`,
///    `window.close` and the rest) as typed actions too: [`crate::app`],
///    [`crate::window`]; and the shell's, such as `menu_bar.open`;
/// 2. the app's own commands, from [`Config::commands`];
/// 3. the keymap and menus, parsed for this platform and checked against
///    every command registered so far, and the app's own catalogs
///    ([`Config::catalog`]), then installed;
/// 4. in-window prompts where the platform has no native ones, and
///    `TextField`'s editing keys for this platform;
/// 5. kiln's dark and light themes, in [`Config::theme_mode`];
/// 6. the user's settings, keymap and theme files, watched from now on
///    ([`kiln_platform::config`]); a user theme applies at once.
///
/// Returns the keymap's, menus', settings' and theme's diagnostics, which
/// it has also logged: errors and warnings at those levels. Those the
/// user's files have later are logged as they change, and
/// [`kiln_platform::config::diagnostics`] has them all.
pub fn init(cx: &mut App, mut config: Config) -> Vec<Diagnostic> {
    commands::init(cx, config.app_name.clone());
    crate::app_commands::init(cx);
    kiln_shell::register(cx);
    for register in std::mem::take(&mut config.commands) {
        register(cx);
    }
    cx.set_global(AppInfo {
        id: config.app_id.clone(),
        name: config.app_name.clone(),
    });

    let os = Os::current();
    let mut catalog = commands::catalog(cx);
    let more = std::mem::take(&mut config.catalogs);
    if !more.is_empty() {
        catalog = Rc::new(Catalogs(std::iter::once(catalog).chain(more).collect()));
    }
    let keymap = config.keymap_store(os, catalog.clone());
    let mut diagnostics = keymap.diagnostics().to_vec();
    keys::init(cx, keymap.keymap().clone());
    let (menu_bar, menu_diagnostics) = config.menu_bar_for(os, Some(&*catalog));
    diagnostics.extend(menu_diagnostics);
    menus::install(cx, menu_bar);

    kiln_shell::init(cx);
    cx.bind_keys(TextField::key_bindings(os == Os::MacOs));
    set_themes(cx, config.theme_mode, Theme::dark(), Theme::light());

    let mut service = match config.keymap_reloaded.take() {
        Some(reloaded) => ConfigService::new().keymap_with(keymap, reloaded),
        None => ConfigService::new().keymap(keymap),
    };
    if let Some(target) = config.log_target {
        service = service.log_target(target);
    }
    for add in std::mem::take(&mut config.settings) {
        let found;
        (service, found) = add(service);
        diagnostics.extend(found);
    }
    if let Some(path) = config.user_theme.take() {
        let theme = UserTheme::load(path);
        diagnostics.extend(theme.diagnostics());
        service = service.source(theme);
    }
    let target = config.log_target.unwrap_or(DEFAULT_LOG_TARGET);
    log_diagnostics_as(target, &diagnostics);
    let task = match config.batches.take() {
        Some(batches) => service.start_with(batches, cx),
        None if service.paths().is_empty() => {
            // Nothing to watch: the service only applies what's loaded.
            let (_, closed) = batch_channel();
            service.start_with(closed, cx)
        }
        None => match service.start(cx) {
            Ok(task) => task,
            Err(error) => {
                log::warn!("config files won't reload when changed: {error}");
                Task::ready(())
            }
        },
    };
    cx.set_global(Watching(task));
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
