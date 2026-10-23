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
//! `init` checks the keymap and menus against every command registered
//! when it runs, so the app's commands are registered through the config
//! ([`Config::commands`]), before that check. A command added through
//! [`crate::Contributions`] instead may come before `init` or any time
//! after, with its default keys and menu item: each addition after `init`
//! resolves the keymap and menus again, checking the app's and the user's
//! against the grown set, so a binding or item naming a command added
//! later takes it up then.
//!
//! The user's files (settings, keymap and theme) are watched from `init`
//! on, and each change applies to every window: see
//! [`kiln_platform::config`].

use std::{
    any::TypeId,
    borrow::Cow,
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{App, Global, SharedString, Task, WindowOptions};
use kiln_command::{CommandId, Value};
use kiln_core::{
    ApiVersion, AppKind, CatalogError, CommandCatalog, Diagnostic, ExtensionSet, Keymap,
    KeymapLayer, KeymapStore, LicenseNotice, MenuBar, Os, SettingsStore, SettingsType,
    load_keymap_layer, theme::ThemeFile,
};
use kiln_platform::{
    Capabilities, WindowSpec, commands,
    config::{
        Batches, ConfigService, ConfigSource, DEFAULT_LOG_TARGET, batch_channel, log_diagnostics_as,
    },
    keys, menus,
};
use kiln_ui::{Assets, TextField, Theme, ThemeMode, set_themes};

use crate::{
    config::{Layers, SelectionSource, UserTheme},
    contributions::{self, Shipped},
    extensions::{self, ExtensionSource, Shared},
    settings::{SettingsSection, section_store},
};

mod defaults;

pub(crate) use defaults::toml_escape;

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

/// Adds a settings store to the service, returning its diagnostics: one
/// [`Config::settings`] loads then, its user file in the directory given,
/// if any ([`Config::user_files_in`]).
type AddSettings =
    Box<dyn FnOnce(ConfigService, Option<&Path>) -> (ConfigService, Vec<Diagnostic>)>;

/// A settings section ([`Config::section`]): its type, its table's name,
/// and what adds its store to the service, given the settings file and
/// every section's name.
struct Section {
    ty: TypeId,
    name: &'static str,
    add: AddSection,
}

type AddSection =
    fn(ConfigService, Option<PathBuf>, &[&'static str]) -> (ConfigService, Vec<Diagnostic>);

/// Adds an app's own source to the service ([`Config::source`]), which
/// puts its diagnostics in the list given once it's installed.
type AddSource = Box<dyn FnOnce(ConfigService, Installed) -> ConfigService>;

/// The app's sources' diagnostics, each as it was installed, in order.
type Installed = Rc<RefCell<Vec<Diagnostic>>>;

/// An app's source, which adds its diagnostics to `installed` once it's
/// installed: what it found reading its files may have changed since it
/// was made.
struct Installing<S> {
    source: S,
    installed: Installed,
}

impl<S: ConfigSource> ConfigSource for Installing<S> {
    fn paths(&self) -> Vec<PathBuf> {
        self.source.paths()
    }

    fn trees(&self) -> Vec<PathBuf> {
        self.source.trees()
    }

    fn user_files(&self) -> Vec<PathBuf> {
        self.source.user_files()
    }

    fn install(&mut self, cx: &mut App) {
        self.source.install(cx);
        (self.installed.borrow_mut()).extend(self.source.diagnostics());
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        self.source.reload(changed, cx);
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.source.diagnostics()
    }
}

/// Each catalog in turn, until one knows the command.
pub(crate) struct Catalogs(pub(crate) Vec<Rc<dyn CommandCatalog>>);

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
    pub(crate) catalogs: Vec<Rc<dyn CommandCatalog>>,
    keymap: Option<Source>,
    user_keymap: Option<PathBuf>,
    keymap_reloaded: Option<Hook>,
    menus: Option<Source>,
    pub(crate) menu_shortcut_context: Vec<String>,
    theme_mode: ThemeMode,
    /// One store for each settings type, in the order first given.
    settings: Vec<(TypeId, AddSettings)>,
    /// The file the sections share, and the sections, in the order first
    /// given.
    settings_file: Option<PathBuf>,
    sections: Vec<Section>,
    user_theme: Option<PathBuf>,
    theme_extensions: &'static [&'static str],
    /// The app's own sources, in the order given.
    sources: Vec<AddSource>,
    batches: Option<Batches>,
    log_target: Option<&'static str>,
    default_keys: bool,
    default_menus: bool,
    fail_fast: bool,
    capabilities: Option<Capabilities>,
    /// Where the user's files are read from instead, each by its name.
    user_dir: Option<PathBuf>,
    extensions: extensions::Setup,
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
            settings_file: None,
            sections: Vec::new(),
            user_theme: None,
            theme_extensions: &[],
            sources: Vec::new(),
            batches: None,
            log_target: None,
            default_keys: false,
            default_menus: false,
            fail_fast: false,
            capabilities: None,
            user_dir: None,
            extensions: extensions::Setup::default(),
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
    /// keeps its last good version. Each type is its own store, and a
    /// later call for the same type replaces the earlier one. The files
    /// are read when [`init`] runs, not before.
    pub fn settings<S: SettingsType>(
        self,
        user: Option<PathBuf>,
        project: Option<PathBuf>,
    ) -> Self {
        self.add_settings::<S>(Box::new(move |service, user_dir| {
            let user = user.map(|user| moved_into(user, user_dir));
            let store = SettingsStore::<S>::load(user, project);
            let diagnostics = store.diagnostics().to_vec();
            (service.settings(store), diagnostics)
        }))
    }

    /// The app's settings, of its own type `S`, as [`Config::settings`]
    /// has them, from a store the app has loaded: one with a schema or a
    /// project filter ([`kiln_core::SettingsStore::load_with`]), or whose
    /// settings it read before `init`. It replaces an earlier store of the
    /// same type. Its files are the app's: [`Config::user_files_in`]
    /// leaves them be.
    pub fn settings_store<S: SettingsType>(self, store: SettingsStore<S>) -> Self {
        self.add_settings::<S>(Box::new(move |service, _| {
            let diagnostics = store.diagnostics().to_vec();
            (service.settings(store), diagnostics)
        }))
    }

    /// The settings file the sections share ([`Config::section`]): the
    /// user's `settings.toml`, which [`crate::app()`] gives. Without one,
    /// each section is its defaults.
    pub fn settings_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.settings_file = Some(path.into());
        self
    }

    /// The app's settings section `S`, as `#[kiln::settings]` declares one
    /// ([`SettingsSection`]): its table of the settings file
    /// ([`Config::settings_file`]), over `S::default()`, checked against
    /// `S`'s schema and reloaded when the file changes. Read it with
    /// `cx.setting::<S>()` in a view ([`crate::Cx::setting`]), which
    /// redraws after a reload changes it, or `cx.settings::<S>()`; every
    /// window redraws after one does. A key of another section's table
    /// isn't unknown to it, but one in no section's, or in its own that
    /// `S` doesn't have, is a warning. It replaces an earlier store of
    /// the same type, as [`Config::settings`] does.
    ///
    /// # Panics
    ///
    /// In [`init`], if two types are sections of the same name.
    pub fn section<S: SettingsSection>(mut self) -> Self {
        let ty = TypeId::of::<S>();
        self.settings.retain(|(added, _)| *added != ty);
        let section = Section {
            ty,
            name: S::SECTION,
            add: section_store::<S>,
        };
        match self.sections.iter_mut().find(|added| added.ty == ty) {
            Some(earlier) => *earlier = section,
            None => self.sections.push(section),
        }
        self
    }

    /// Adds the store `add` makes for `S`, in place of an earlier one.
    fn add_settings<S: SettingsType>(mut self, add: AddSettings) -> Self {
        let ty = TypeId::of::<S>();
        self.sections.retain(|section| section.ty != ty);
        match self.settings.iter_mut().find(|(added, _)| *added == ty) {
            Some((_, earlier)) => *earlier = add,
            None => self.settings.push((ty, add)),
        }
        self
    }

    /// The user's theme file, reloaded when it changes. Laid over the
    /// dark or light theme, as its `appearance` says (kiln's base, or the
    /// installed theme chosen for it with
    /// [`crate::extensions::select_themes`]), it takes that one's place,
    /// so [`Config::theme_mode`] and `cx.set_theme_mode` still pick
    /// between dark and light. A missing file is no theme of the user's,
    /// and one with errors keeps its last good version.
    pub fn user_theme(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_theme = Some(path.into());
        self
    }

    /// The app's own tables the user's theme file may have, such as an
    /// editor's colours: kept as written, for the app to read with
    /// `cx.theme().data().extension(name)`. A change to one alone is a
    /// theme change. Extensions' themes may have them too, and the user's
    /// file's keys go over theirs. kiln's own themes have none, so with
    /// those alone `extension` is `None` and the app uses its defaults.
    /// Without this, such a table is an unknown key.
    ///
    /// # Panics
    ///
    /// If a name is one of kiln's own keys (`ui`, `surface` …), which a
    /// theme file's table of that name would set instead.
    pub fn theme_extensions(mut self, names: &'static [&'static str]) -> Self {
        if let Some(name) = names.iter().find(|name| ThemeFile::is_reserved(name)) {
            panic!("`{name}` is a theme key of kiln's own, not an app's table");
        }
        self.theme_extensions = names;
        self
    }

    /// The user's extensions, a directory each under `dir` (the app's
    /// [`kiln_core::AppPaths::extensions_dir`], in its state directory),
    /// loaded by [`init`] in ID order and reloaded when anything under
    /// `dir` changes: see [`crate::extensions`]. Their manifests must name
    /// `app` in `[engine]` (the app's [`kiln_core::AppId::short`]) at an
    /// API version `api_version` serves, and IDs may not start with the
    /// namespace of a command registered by `init`. A manifest with errors
    /// keeps its last good version, its errors in
    /// [`kiln_platform::config::diagnostics`]; [`Config::fail_fast`]
    /// never panics over them.
    pub fn extensions(
        mut self,
        dir: impl Into<PathBuf>,
        app: impl Into<String>,
        api_version: ApiVersion,
    ) -> Self {
        self.extensions.dir = Some((dir.into(), app.into(), api_version));
        self
    }

    /// A contribution kind of the app's own that extensions may have, such
    /// as a language server: their `contributes.<name>` table is checked
    /// against `schema` and kept as written in
    /// [`kiln_core::Manifest::app`]. Each call adds one.
    pub fn extension_kind(mut self, name: &'static str, schema: serde_json::Value) -> Self {
        self.extensions.kinds.push(AppKind { name, schema });
        self
    }

    /// Loads only extensions whose licence these SPDX IDs allow (the
    /// user's `extensions.allowed_licenses`); unset, any licence loads.
    pub fn allowed_extension_licenses(mut self, ids: Vec<String>) -> Self {
        self.extensions.allowed_licenses = Some(ids);
        self
    }

    /// Loads a workspace folder's own extensions beside the user's, from
    /// `<root>/.<app>/extensions` for the root the app sets
    /// ([`crate::extensions::set_workspace_root`]), while it's trusted:
    /// see [`crate::extensions`]. They may contribute only `kinds`, of the
    /// app's own ([`Config::extension_kind`]); any other kind, themes,
    /// keymaps and menus too, is dropped with a warning. Takes
    /// [`Config::extensions`] too.
    pub fn workspace_extensions<S: Into<String>>(
        mut self,
        kinds: impl IntoIterator<Item = S>,
    ) -> Self {
        self.extensions.workspace_kinds = Some(kinds.into_iter().map(Into::into).collect());
        self
    }

    /// Calls `f` with the extensions [`init`] loaded, once the service has
    /// started, and again after each reload that changes them or their
    /// diagnostics: to apply an app kind's tables, say. With
    /// [`Config::workspace_extensions`], the set has the workspace's joined
    /// after the user's while they're loaded, each with its source
    /// ([`ExtensionSet::sourced`]). Each call adds one, called in the
    /// order added.
    pub fn on_extensions(mut self, f: impl FnMut(&ExtensionSet, &mut App) + 'static) -> Self {
        self.extensions.on_extensions.push(Box::new(f));
        self
    }

    /// Lists the files of an extension `f` reads for the app, such as the
    /// ones its manifest's tables name, relative to its directory. When one
    /// changes on disk, or comes or goes, with its manifest the same,
    /// [`Config::on_extensions`] is called again. Without it, only a change
    /// to a manifest or its diagnostics is.
    pub fn extension_files(
        mut self,
        f: impl Fn(&kiln_core::Manifest) -> Vec<std::path::PathBuf> + 'static,
    ) -> Self {
        self.extensions.files_of = Some(Box::new(f));
        self
    }

    /// Calls `f` with each licence notice: an extension the user
    /// installed whose licence is copyleft or unknown, the first time it
    /// loads at its version (see [`ExtensionSet::license_notices`]), to
    /// show it, as a toast say. Its text is [`LicenseNotice::message`].
    /// Each call adds one; without any, it's only logged.
    pub fn on_license_notice(mut self, f: impl FnMut(&LicenseNotice, &mut App) + 'static) -> Self {
        self.extensions.on_notice.push(Box::new(f));
        self
    }

    /// A config source of the app's own, such as a file in a format kiln
    /// doesn't read: watched and reloaded with the user's files, installed
    /// when the service starts, its diagnostics (as installed) logged with
    /// theirs and in [`kiln_platform::config::diagnostics`]. Each call adds one, after
    /// kiln's sources and in the order added. [`Config::user_files_in`]
    /// leaves its files be.
    pub fn source(mut self, source: impl ConfigSource) -> Self {
        self.sources.push(Box::new(move |service, installed| {
            service.source(Installing { source, installed })
        }));
        self
    }

    /// Takes the changed files from `batches` instead of watching them:
    /// for tests, which send what the watcher would
    /// ([`kiln_platform::config::batch_channel`]).
    pub fn watch_with(mut self, batches: Batches) -> Self {
        self.batches = Some(batches);
        self
    }

    /// Reads the user's files from `dir` instead, each by its file name,
    /// however they were named: the settings from [`Config::settings`],
    /// the sections' file ([`Config::settings_file`]), the keymap and the
    /// theme. For tests, so that none of the user's
    /// own files is read: [`crate::testing::TestApp`] reads them from an
    /// empty temporary directory. A store loaded before `init`
    /// ([`Config::settings_store`]) keeps the files it read.
    pub fn user_files_in(mut self, dir: impl Into<PathBuf>) -> Self {
        self.user_dir = Some(dir.into());
        self
    }

    /// Sets the app up as on a platform with `capabilities`, rather than
    /// this one: the keymap and menus as on its OS, the editing keys, the
    /// prompts, and every window's options and chrome
    /// ([`Capabilities::install`]). For tests, and for showing one
    /// platform's app on another; unset, the app is this platform's
    /// ([`Capabilities::current`]).
    pub fn capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = Some(capabilities);
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
    /// given: kiln's bundled keys if [`Config::default_keys`] is on, the
    /// app's over them, then the user's file over both.
    pub fn keymap_for(
        &self,
        os: Os,
        catalog: Option<&dyn CommandCatalog>,
    ) -> (Keymap, Vec<Diagnostic>) {
        let (mut layers, mut diagnostics) = self.own_keymap(os, catalog, None);
        if let Some(path) = &self.user_keymap {
            let (layer, found) = load_keymap_layer(path, catalog);
            layers.extend(layer);
            diagnostics.extend(found);
        }
        (Keymap::from_layers(layers), diagnostics)
    }

    /// The keymap store [`init`] reloads: the `layers` the app ships, as
    /// [`Config::own_keymap`] parsed them with their `diagnostics`, then
    /// the user's file.
    fn keymap_store(
        &self,
        layers: Vec<KeymapLayer>,
        diagnostics: Vec<Diagnostic>,
        catalog: Rc<dyn CommandCatalog>,
    ) -> KeymapStore {
        KeymapStore::with_defaults(
            layers,
            diagnostics,
            self.user_keymap.clone(),
            Some(Box::new(catalog)),
        )
    }

    /// The menu bar as on `os`, its items checked against `catalog` if
    /// given, their shortcuts resolved in [`Config::menu_shortcut_context`]:
    /// the app's table, or kiln's bundled menus if [`Config::default_menus`]
    /// is on and the app has none. Empty without a table, or with errors
    /// in it.
    pub fn menu_bar_for(
        &self,
        os: Os,
        catalog: Option<&dyn CommandCatalog>,
    ) -> (MenuBar, Vec<Diagnostic>) {
        let Some((path, text)) = self.menu_source() else {
            return (MenuBar::default(), Vec::new());
        };
        let frames: Vec<&str> = self
            .menu_shortcut_context
            .iter()
            .map(String::as_str)
            .collect();
        let (bar, diagnostics) = MenuBar::parse_in(path, &text, catalog, os, &frames);
        let mut bar = bar.unwrap_or_default();
        if self.menus.is_none() {
            defaults::name_app_menu(&mut bar, &self.app_name);
        }
        (bar, diagnostics)
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

/// Sets kiln up for the app, on this platform or the one
/// [`Config::capabilities`] names, in this order:
///
/// 1. kiln's command dispatch, with the app commands (`app.quit`,
///    `window.close` and the rest) as typed actions too: [`crate::app`](mod@crate::app),
///    [`crate::window`]; and the shell's, such as `menu_bar.open`;
/// 2. the app's own commands, from [`Config::commands`], and those
///    contributed so far ([`crate::Contributions`]);
/// 3. the keymap and menus, kiln's bundled ones too if
///    [`Config::default_keys`] and [`Config::default_menus`] are on, and
///    the contributed commands' keys and items, parsed for this platform
///    and checked against every command registered so far ([`catalog`]:
///    the shell's picker commands too), and the app's own catalogs
///    ([`Config::catalog`]), then the user's extensions' keymap and menu
///    layers over them ([`crate::extensions`]), then installed; from here
///    on, each command contributed, and each change to the extensions'
///    files, resolves them again;
/// 4. in-window prompts where the platform has no native ones, and
///    `TextField`'s editing keys for this platform;
/// 5. kiln's dark and light themes, in [`Config::theme_mode`];
/// 6. the user's settings, keymap and theme files, watched from now on
///    ([`kiln_platform::config`]); a user theme applies at once;
/// 7. the user's extensions ([`Config::extensions`]), their directory
///    watched from now on, and [`Config::on_extensions`] called with them
///    once the service starts; their themes are installed, for
///    [`crate::extensions::select_themes`] to choose; a workspace's
///    ([`Config::workspace_extensions`]) load once the app sets its root;
/// 8. the app's own sources ([`Config::source`]), watched from now on.
///
/// Returns the keymap's, menus', settings', theme's, extensions' and the
/// app's sources' diagnostics, which it has also logged: errors and
/// warnings at those levels. Those the user's files have later are logged as they change, and
/// [`kiln_platform::config::diagnostics`] has them all.
///
/// # Panics
///
/// With [`Config::fail_fast`] on, if the keymap or menus the app ships
/// have any diagnostic, such as two contributed commands with the same
/// default keys in the same context; and when a command contributed later
/// brings one. Never for an extension's.
pub fn init(cx: &mut App, mut config: Config) -> Vec<Diagnostic> {
    if let Some(capabilities) = config.capabilities {
        capabilities.install(cx);
    }
    if let Some(dir) = config.user_dir.clone() {
        config.user_keymap = (config.user_keymap.take()).map(|path| moved_into(path, Some(&dir)));
        config.user_theme = (config.user_theme.take()).map(|path| moved_into(path, Some(&dir)));
    }
    commands::init(cx, config.app_name.clone());
    crate::app_commands::init(cx);
    kiln_shell::register(cx);
    for register in std::mem::take(&mut config.commands) {
        register(cx);
    }
    contributions::register_early(cx);
    cx.set_global(AppInfo {
        id: config.app_id.clone(),
        name: config.app_name.clone(),
    });

    // Discovered first, so their keymap and menu files are laid in.
    let layers = Layers::get(cx);
    let setup = std::mem::take(&mut config.extensions);
    let namespaces = command_namespaces(cx);
    let extension_source = ExtensionSource::load(
        setup,
        namespaces,
        config.theme_extensions,
        layers.clone(),
        extensions::Files::get(cx),
        // Batches given name a workspace's directory too.
        config.batches.is_none(),
    );

    let os = Capabilities::of(cx).os;
    let catalog = contributions::shipped_catalog(&config, cx);
    let shipped = contributions::shipped(&config, os, &*catalog, cx);
    config.check_fail_fast(&shipped.diagnostics());
    let Shipped {
        layers: keymap_layers,
        keymap_diagnostics,
        menu_bar,
        menu_diagnostics,
        extension_diagnostics,
    } = shipped;
    extensions::set_laid(cx, extension_diagnostics, false);
    let keymap = config.keymap_store(keymap_layers, keymap_diagnostics, catalog);
    let mut diagnostics = keymap.diagnostics().to_vec();
    keys::init(cx, keymap.keymap().clone());
    diagnostics.extend(menu_diagnostics.iter().cloned());
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
    for (_, add) in std::mem::take(&mut config.settings) {
        let found;
        (service, found) = add(service, config.user_dir.as_deref());
        diagnostics.extend(found);
    }
    let sections = std::mem::take(&mut config.sections);
    let names: Vec<&'static str> = sections.iter().map(|section| section.name).collect();
    for (ix, name) in names.iter().enumerate() {
        assert!(
            !names[..ix].contains(name),
            "two settings types are the section [{name}] of settings.toml: rename one's `section`"
        );
    }
    let settings_file =
        (config.settings_file.take()).map(|path| moved_into(path, config.user_dir.as_deref()));
    for section in sections {
        let found;
        (service, found) = (section.add)(service, settings_file.clone(), &names);
        diagnostics.extend(found);
    }
    if let Some(path) = config.user_theme.take() {
        let theme = UserTheme::load(path, config.theme_extensions, layers.clone());
        diagnostics.extend(theme.diagnostics());
        service = service.source(theme);
    }
    if let Some(source) = extension_source {
        let source = Rc::new(RefCell::new(source));
        cx.set_global(extensions::Running(source.clone()));
        let source = Shared(source);
        diagnostics.extend(source.diagnostics());
        service = service.source(source);
    }
    let installed = Installed::default();
    for add in std::mem::take(&mut config.sources) {
        service = add(service, installed.clone());
    }
    // Last, so a choice made between batches can put its diagnostics in
    // place of the last ones at the end ([`Layers::select`]).
    service = service.source(SelectionSource(layers));
    let target = config.log_target.unwrap_or(DEFAULT_LOG_TARGET);
    log_diagnostics_as(target, &diagnostics);
    let shared_keymap = service.keymap_store().expect("the service has the keymap");
    let task = match config.batches.take() {
        Some(batches) => service.start_with(batches, cx),
        None if service.paths().is_empty() && service.trees().is_empty() => {
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
    // The app's sources', as installed.
    let installed = installed.take();
    log_diagnostics_as(target, &installed);
    diagnostics.extend(installed);
    contributions::start(cx, config, os, shared_keymap, menu_diagnostics);
    diagnostics
}

/// Every command [`init`] checks an app's keymap and menus against, but
/// the app's own catalogs ([`Config::catalog`]): the app registry's, the
/// typed commands (kiln's, the shell's and the app's) and the shell's
/// picker commands (`picker.confirm`, `text_input.backspace` …), which
/// its palette's keys run. For an app's tests that check its keymap and
/// menus as on another platform ([`Config::keymap_for`]); call it once the
/// commands are registered.
pub fn catalog(cx: &App) -> Rc<dyn CommandCatalog> {
    Rc::new(Catalogs(vec![
        commands::catalog(cx),
        Rc::from(kiln_shell::catalog()),
    ]))
}

/// GPUI's options for a window as `spec` describes it, on the app's
/// platform ([`Capabilities::of`]: this one unless [`Config::capabilities`]
/// said otherwise), with the app ID [`init`] recorded if `spec` names none.
pub fn window_options(spec: &WindowSpec, cx: &App) -> WindowOptions {
    let mut spec = spec.clone();
    if spec.app_id.is_none() {
        spec.app_id = cx.try_global::<AppInfo>().map(|info| info.id.clone());
    }
    kiln_platform::window_options(&spec, &Capabilities::of(cx), cx)
}

/// The namespace of every command registered so far, each once: the app
/// registry's and the typed ones.
fn command_namespaces(cx: &App) -> Vec<String> {
    let app = commands::app_command_list(cx)
        .into_iter()
        .map(|meta| meta.id);
    let typed = kiln_platform::command_actions::typed_commands(cx);
    let mut namespaces: Vec<String> = app
        .chain(typed.into_iter().map(|typed| typed.id))
        .map(|id| id.namespace().to_owned())
        .collect();
    namespaces.sort();
    namespaces.dedup();
    namespaces
}

/// `path` in `dir` by its file name, if there's a `dir`.
fn moved_into(path: PathBuf, dir: Option<&Path>) -> PathBuf {
    match (dir, path.file_name()) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => path,
    }
}
