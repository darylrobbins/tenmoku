// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`app`]: kiln's conventions for an app's files, settings, quitting and
//! first window, composed over [`init`] and [`Config`], each with a
//! one-line override.

use std::{borrow::Cow, path::PathBuf, rc::Rc};

use gpui::{
    AnyView, AnyWindowHandle, App, AppContext, Context, Entity, Pixels, QuitMode, SharedString,
    Window, WindowHandle,
};
use kiln_command::Value;
use kiln_core::{AppId, AppPaths, Diagnostic, Os, PathStyle, SettingsType};
use kiln_platform::WindowSpec;
use kiln_shell::{
    DockItem, DockPosition, PaneItem, Root, StatusItem, Workspace, close,
    layout::{self, LayoutStore, WindowLayout},
};
use kiln_ui::{ThemeMode, follow_system_appearance};

use crate::{
    Contributions,
    setup::{Config, application, init, window_options},
};

/// Changes the [`Config`] the conventions built: see [`AppBuilder::config`].
type ConfigHook = Box<dyn FnOnce(Config) -> Config>;

/// Adds a settings store, given the user's `settings.toml`.
type AddSettings = Box<dyn FnOnce(Config, Option<PathBuf>) -> Config>;

type WindowHook = Box<dyn FnOnce(WindowSpec) -> WindowSpec>;

type RootView = Box<dyn FnOnce(&mut Window, &mut App) -> AnyView>;

/// Sets up each workspace window: adds a panel, say. A launch that
/// restores several windows calls it for each.
type WorkspaceHook = Rc<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// Adds to [`Contributions`]: a [`crate::Module`], named for the warning
/// if it adds nothing, or the inline form.
struct Contribute {
    add: Box<dyn FnOnce(&mut App)>,
    module: Option<&'static str>,
}

/// An app with the reverse-DNS ID `id` (`dev.kiln.Chat`) and the name
/// people see (`Kiln Chat`), set up by kiln's conventions. Each has a
/// one-line override:
///
/// | Convention | Override |
/// |---|---|
/// | The short name is the name lowercased, each run of other characters than letters and digits a `-`, none at either end: `Kiln Chat` is `kiln-chat` | [`AppBuilder::short_name`] |
/// | The app's directories are `AppPaths::new(&AppId::new(id, short, name), PathStyle::Native)`, so `<SHORT>_CONFIG_DIR` and `<SHORT>_STATE_DIR` move them | [`AppBuilder::paths`] |
/// | The user's `keymap.toml` and `theme.toml` in the config directory, watched | [`AppBuilder::config`] |
/// | [`AppBuilder::settings`] reads `settings.toml` in the config directory, and no project file | [`AppBuilder::config`] with [`Config::settings`], which replaces the store |
/// | Closing the last window quits, except on macOS | [`AppBuilder::quit_on_last_window`] |
/// | Its windows from [`WindowSpec::main`] with the app's name and ID | [`AppBuilder::window`] |
/// | Each window a [`Workspace`] of [`ItemPane`](kiln_shell::ItemPane)s under the key context `workspace`, titled with the app's name, with the items, panels and status sources the builder adds, then those contributed ([`Contributions`]) | [`AppBuilder::key_context`], [`AppBuilder::root`] for a view of the app's own in a [`Root`] |
/// | The workspaces' layout saved in the state directory's `layouts`, the state directory standing for the root folder, and on start every saved window no live one holds opened again, else one fresh window | [`AppBuilder::restore_layout`] |
/// | kiln's own keys under the app's keymap, and its menus if the app has no table | [`AppBuilder::config`] with [`Config::default_keys`] or [`Config::default_menus`] |
/// | In a debug build, a mistake in the keymap or menus the app ships panics at start | [`AppBuilder::config`] with [`Config::fail_fast`] |
///
/// [`AppBuilder::commands`], [`AppBuilder::keymap`], [`AppBuilder::menus`],
/// [`AppBuilder::theme_mode`] and [`AppBuilder::theme_extensions`] are
/// [`Config`]'s, and [`AppBuilder::config`] changes anything else on it.
/// The window is a workspace once the app adds an item kind
/// ([`AppBuilder::item`]), a panel ([`AppBuilder::panel`]) or a setup hook
/// ([`AppBuilder::workspace`]), or a [`Root`] around the app's view from
/// [`AppBuilder::root`]: one of them is required. A library crate's
/// panels, item kinds, status sources, commands and settings come in
/// through its [`crate::Module`] ([`AppBuilder::module`]), and the app's
/// own may too ([`AppBuilder::contributions`]).
///
/// ```ignore
/// kiln::app("com.example.Notes", "Notes")
///     .commands(notes::register)
///     .keymap("keymap.toml", include_str!("keymap.toml"))
///     .settings::<NotesSettings>()
///     .item::<Note>()
///     .panel(DockPosition::Left, |window, cx| cx.new(|cx| Notes::new(window, cx)))
///     .status(notes::word_count)
///     .run();
/// ```
///
/// is, by hand, on a platform other than macOS:
///
/// ```ignore
/// kiln::application().run(|cx: &mut App| {
///     let id = AppId::new("com.example.Notes", "notes", "Notes");
///     let paths = AppPaths::new(&id, PathStyle::Native);
///     let config_dir = paths.config_dir().unwrap();
///     kiln::init(
///         cx,
///         kiln::Config::new("com.example.Notes", "Notes")
///             .commands(notes::register)
///             .keymap("keymap.toml", include_str!("keymap.toml"))
///             .settings::<NotesSettings>(paths.settings_file(), None)
///             .user_keymap(config_dir.join("keymap.toml"))
///             .user_theme(config_dir.join("theme.toml"))
///             .default_keys(true)
///             .default_menus(true)
///             .fail_fast(cfg!(debug_assertions)),
///     );
///     layout::register_item::<Note>(cx);
///     cx.set_quit_mode(QuitMode::Explicit);
///     cx.on_window_closed(|cx, _| {
///         if cx.windows().is_empty() {
///             kiln::shell::close::quit(cx);
///         }
///     })
///     .detach();
///     let store = LayoutStore::new(paths.layouts_dir().unwrap(), paths.state_dir().unwrap());
///     let spec = WindowSpec::main("Notes").app_id("com.example.Notes");
///     for (layout, saved) in layout::launch(&store, true) {
///         cx.open_window(kiln::window_options(&spec, cx), |window, cx| {
///             cx.new(|cx| {
///                 let mut workspace = Workspace::item_panes("workspace", window, cx).title("Notes");
///                 let notes = cx.new(|cx| Notes::new(window, cx));
///                 workspace.add_panel(DockPosition::Left, notes, window, cx);
///                 workspace.add_status_source(notes::word_count, cx);
///                 workspace.set_layout(layout, window, cx);
///                 if let Some(saved) = &saved {
///                     workspace.restore(saved, window, cx);
///                 }
///                 workspace
///             })
///         })
///         .unwrap();
///     }
///     cx.activate(true);
/// });
/// ```
pub fn app(id: impl Into<String>, name: impl Into<SharedString>) -> AppBuilder {
    let id = id.into();
    let name = name.into();
    let short = short_name(&name, &id);
    let paths = native_paths(&id, &short, &name);
    AppBuilder {
        config: Config::new(id.clone(), name.clone())
            .default_keys(true)
            .default_menus(true)
            .fail_fast(cfg!(debug_assertions)),
        id,
        name,
        short,
        paths,
        paths_given: false,
        settings: Vec::new(),
        hooks: Vec::new(),
        quit_on_last_window: Os::current() != Os::MacOs,
        window: None,
        root: None,
        key_context: "workspace".into(),
        items: Vec::new(),
        panels: Vec::new(),
        dock_sizes: Vec::new(),
        status: Vec::new(),
        setup: Vec::new(),
        contribute: Vec::new(),
        restore_layout: true,
    }
}

/// An app being set up: see [`app`].
#[must_use = "an app does nothing until it's `run` or `start`ed"]
pub struct AppBuilder {
    id: String,
    name: SharedString,
    short: String,
    paths: AppPaths,
    /// Whether [`AppBuilder::paths`] set them, so the short name leaves
    /// them be.
    paths_given: bool,
    config: Config,
    settings: Vec<AddSettings>,
    hooks: Vec<ConfigHook>,
    quit_on_last_window: bool,
    window: Option<WindowHook>,
    root: Option<RootView>,
    key_context: SharedString,
    /// Registers an item kind for restore, once [`init`] has run.
    items: Vec<fn(&mut App)>,
    /// Adds a panel to each workspace.
    panels: Vec<WorkspaceHook>,
    dock_sizes: Vec<(DockPosition, Pixels)>,
    status: Vec<fn(&App) -> Option<StatusItem>>,
    /// The app's own setup of each workspace, once the rest is in.
    setup: Vec<WorkspaceHook>,
    contribute: Vec<Contribute>,
    restore_layout: bool,
}

/// What [`AppBuilder::start`] did.
pub struct Started {
    /// [`init`]'s diagnostics: the keymap's, menus', settings' and theme's.
    pub diagnostics: Vec<Diagnostic>,
    /// The first window it opened: a [`Workspace`], or kiln's [`Root`]
    /// around the app's view.
    pub window: AppWindow,
    /// Every window it opened, the first `window`: a launch that restores
    /// the saved layout opens one for each window saved.
    pub windows: Vec<AppWindow>,
    /// What the windows were opened from, the app's ID in it.
    pub spec: WindowSpec,
}

/// A window [`AppBuilder::start`] opened, its root view a [`Workspace`] or
/// a [`Root`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppWindow(pub AnyWindowHandle);

impl AppWindow {
    /// The window's root view, if it's still open and a `V`:
    /// `window.root::<Workspace>(cx)`.
    pub fn root<V: 'static>(&self, cx: &mut impl AppContext) -> Option<Entity<V>> {
        (cx.update_window(self.0, |view, _, _| view.downcast::<V>().ok()))
            .ok()
            .flatten()
    }

    /// The window's handle, typed, if its root view is a `V`.
    pub fn downcast<V: 'static>(&self) -> Option<WindowHandle<V>> {
        self.0.downcast()
    }

    /// Updates the window: [`AnyWindowHandle::update`].
    pub fn update<C: AppContext, R>(
        &self,
        cx: &mut C,
        update: impl FnOnce(AnyView, &mut Window, &mut App) -> R,
    ) -> gpui::Result<R> {
        self.0.update(cx, update)
    }
}

impl AppBuilder {
    /// The short lowercase name, for the app's directories and their
    /// environment variables (`kiln-chat`: `~/.config/kiln-chat`,
    /// `KILN_CHAT_CONFIG_DIR`). The paths follow it unless
    /// [`AppBuilder::paths`] gave them.
    pub fn short_name(mut self, short: impl Into<String>) -> Self {
        self.short = short.into();
        if !self.paths_given {
            self.paths = native_paths(&self.id, &self.short, &self.name);
        }
        self
    }

    /// The app's directories, in place of the native ones: say, under a
    /// temporary directory in a test.
    pub fn paths(mut self, paths: AppPaths) -> Self {
        self.paths = paths;
        self.paths_given = true;
        self
    }

    /// The app's directories, as the conventions and overrides so far
    /// have them.
    pub fn app_paths(&self) -> &AppPaths {
        &self.paths
    }

    /// The app's settings, of its own type `S`: `settings.toml` in the
    /// config directory over `S::default()`, reloaded when it changes, as
    /// [`Config::settings`] has them with no project file. Each type is its
    /// own store: call it once for each. `.config(|config|
    /// config.settings::<S>(..))` replaces this store with another.
    pub fn settings<S: SettingsType>(mut self) -> Self {
        self.settings
            .push(Box::new(|config, user| config.settings::<S>(user, None)));
        self
    }

    /// Whether closing the last window quits the app, through
    /// [`kiln_shell::close::quit`]: by default it does except on macOS,
    /// where an app stays open with no windows. GPUI's own quitting on the
    /// last window's close is off ([`QuitMode::Explicit`]) either way, so
    /// this decides alone.
    pub fn quit_on_last_window(mut self, quit: bool) -> Self {
        self.quit_on_last_window = quit;
        self
    }

    /// Changes the window's spec, [`WindowSpec::main`] with the app's name
    /// as its title and the app's ID: its size, say. The chrome's title is
    /// the spec's.
    pub fn window(mut self, spec: impl FnOnce(WindowSpec) -> WindowSpec + 'static) -> Self {
        self.window = Some(Box::new(spec));
        self
    }

    /// The window's view, made once the window opens, in place of the
    /// workspace: kiln's [`Root`] is around it. Its one window saves no
    /// layout, and the workspace's calls ([`AppBuilder::item`] and the
    /// rest) are left unused.
    pub fn root(mut self, view: impl FnOnce(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.root = Some(Box::new(view));
        self
    }

    /// The workspace's root key context, which its docks look their show
    /// keys up in: `workspace` unless set.
    pub fn key_context(mut self, context: impl Into<SharedString>) -> Self {
        self.key_context = context.into();
        self
    }

    /// Has saved items of `T`'s kind come back with the saved layout,
    /// through [`PaneItem::restore`] ([`layout::register_item`]).
    pub fn item<T: PaneItem>(mut self) -> Self {
        self.items.push(layout::register_item::<T>);
        self
    }

    /// Adds the panel `make` makes to the dock at `position` of each
    /// workspace window.
    pub fn panel<T: DockItem>(
        mut self,
        position: DockPosition,
        make: impl Fn(&mut Window, &mut App) -> Entity<T> + 'static,
    ) -> Self {
        self.panels.push(Rc::new(move |workspace, window, cx| {
            let panel = make(window, cx);
            workspace.add_panel(position, panel, window, cx);
        }));
        self
    }

    /// Shows the item `source` gives on each workspace's status bar, over
    /// the active item's: [`Workspace::add_status_source`], called each
    /// time the bar draws.
    pub fn status(mut self, source: fn(&App) -> Option<StatusItem>) -> Self {
        self.status.push(source);
        self
    }

    /// The width of the dock at `position`, or its height at the bottom,
    /// once its panels are added: a saved layout's size replaces it.
    pub fn dock_size(mut self, position: DockPosition, size: Pixels) -> Self {
        self.dock_sizes.push((position, size));
        self
    }

    /// Sets up each workspace window once its workspace exists, with its
    /// panels, dock sizes and status sources: to open the first items,
    /// say. A saved layout is restored after it, its panes in place
    /// of those it opened. Each call adds one, run in order.
    pub fn workspace(
        mut self,
        setup: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    ) -> Self {
        self.setup.push(Rc::new(setup));
        self
    }

    /// Adds what a library crate contributes, its [`crate::Module`]: `start`
    /// calls it first, before [`init`], so what it adds is set up with the
    /// app's own ([`Contributions`]). In a debug build, a module that adds
    /// nothing logs a warning naming it. Each call adds one, run in order.
    ///
    /// ```ignore
    /// kiln::app("com.example.Notes", "Notes")
    ///     .module(outline::module)
    ///     .run();
    /// ```
    pub fn module(mut self, module: impl FnOnce(&mut App) + 'static) -> Self {
        let name = std::any::type_name_of_val(&module);
        self.contribute.push(Contribute {
            add: Box::new(module),
            module: Some(name),
        });
        self
    }

    /// Adds to [`Contributions`] inline, as a [`AppBuilder::module`] does:
    /// `.contributions(|cx| Contributions::add_panel(cx, ..))`.
    pub fn contributions(mut self, add: impl FnOnce(&mut App) + 'static) -> Self {
        self.contribute.push(Contribute {
            add: Box::new(add),
            module: None,
        });
        self
    }

    /// Whether the workspaces' layout is saved and restored: by default it
    /// is. Off, the app opens one fresh window and saves nothing.
    pub fn restore_layout(mut self, restore: bool) -> Self {
        self.restore_layout = restore;
        self
    }

    /// [`Config::commands`].
    pub fn commands(mut self, register: impl FnOnce(&mut App) + 'static) -> Self {
        self.config = self.config.commands(register);
        self
    }

    /// [`Config::keymap`].
    pub fn keymap(
        mut self,
        path: impl Into<PathBuf>,
        source: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.config = self.config.keymap(path, source);
        self
    }

    /// [`Config::menus`].
    pub fn menus(mut self, path: impl Into<PathBuf>, source: impl Into<Cow<'static, str>>) -> Self {
        self.config = self.config.menus(path, source);
        self
    }

    /// [`Config::theme_mode`].
    pub fn theme_mode(mut self, mode: ThemeMode) -> Self {
        self.config = self.config.theme_mode(mode);
        self
    }

    /// [`Config::theme_extensions`].
    pub fn theme_extensions(mut self, names: &'static [&'static str]) -> Self {
        self.config = self.config.theme_extensions(names);
        self
    }

    /// Changes the [`Config`] the conventions built, once they have: any
    /// of its calls replaces what they set, such as
    /// `.config(|config| config.user_keymap(path))` for another keymap
    /// file. Each call adds one, run in order.
    pub fn config(mut self, change: impl FnOnce(Config) -> Config + 'static) -> Self {
        self.hooks.push(Box::new(change));
        self
    }

    /// Runs the app: [`application`], with [`AppBuilder::start`] and then
    /// the app brought to the front.
    ///
    /// # Panics
    ///
    /// As [`AppBuilder::start`] does.
    pub fn run(self) {
        application().run(move |cx: &mut App| {
            self.start(cx);
            cx.activate(true);
        });
    }

    /// Sets the app up in `cx`: the modules and inline contributions added
    /// ([`AppBuilder::module`]), [`init`] with the [`Config`] and the
    /// contributed settings, the item kinds registered, quitting on the
    /// last window's close if it should (and GPUI's [`QuitMode`]
    /// [`QuitMode::Explicit`]), then the windows:
    /// the [`Root`] window, or a workspace window for each saved layout no
    /// live window holds, else one fresh one.
    ///
    /// # Panics
    ///
    /// Without [`AppBuilder::root`] and with no item kind, panel or
    /// workspace hook ([`AppBuilder::item`], [`AppBuilder::panel`],
    /// [`AppBuilder::workspace`], or an item kind or panel contributed);
    /// with [`Config::fail_fast`] on, as [`init`] does; or if a window
    /// doesn't open.
    pub fn start(self, cx: &mut App) -> Started {
        for Contribute { add, module } in self.contribute {
            let before = Contributions::count(cx);
            add(cx);
            if let Some(name) = module
                && cfg!(debug_assertions)
                && Contributions::count(cx) == before
            {
                log::warn!("the kiln module {name} contributes nothing");
            }
        }
        let mut config = self.config;
        let user_settings = self.paths.settings_file();
        for add in self.settings {
            config = add(config, user_settings.clone());
        }
        config = Contributions::configure(cx, config);
        if let Some(dir) = self.paths.config_dir() {
            config = config
                .user_keymap(dir.join("keymap.toml"))
                .user_theme(dir.join("theme.toml"));
        }
        for change in self.hooks {
            config = change(config);
        }
        let diagnostics = init(cx, config);
        for register in &self.items {
            register(cx);
        }
        // Once `init` has run what was contributed before it, which may
        // contribute more: a model's setup a status source, a command's
        // registration a panel.
        let panels = Contributions::panels(cx);
        let statuses = Contributions::statuses(cx);
        let workspace = self.root.is_none();
        assert!(
            !workspace
                || !(self.items.is_empty()
                    && self.panels.is_empty()
                    && self.setup.is_empty()
                    && panels.is_empty()
                    && !Contributions::has_item_kinds(cx)),
            "kiln::app needs a window: `.root(..)` for a view of the app's own, or \
             `.item::<T>()`, `.panel(..)` or `.workspace(..)` for a workspace"
        );

        cx.set_quit_mode(QuitMode::Explicit);
        if self.quit_on_last_window {
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    close::quit(cx);
                }
            })
            .detach();
        }

        let spec = WindowSpec::main(self.name).app_id(self.id);
        let spec = match self.window {
            Some(change) => change(spec),
            None => spec,
        };
        let title = spec.title.clone();
        let windows = match self.root {
            Some(root) => {
                let window = cx
                    .open_window(window_options(&spec, cx), move |window, cx| {
                        let view = root(window, cx);
                        cx.new(|cx| Root::new(view, window, cx).title(title))
                    })
                    .expect("the app's window opens");
                vec![AppWindow(window.into())]
            }
            None => {
                let store = self.restore_layout.then(|| layouts(&self.paths)).flatten();
                let mut launched: Vec<_> = (store.iter())
                    .flat_map(|store| layout::launch(store, true))
                    .map(|(layout, saved)| (Some(layout), saved))
                    .collect();
                if launched.is_empty() {
                    launched.push((None, None));
                }
                let open = |(layout, saved): (Option<WindowLayout>, Option<Value>),
                            cx: &mut App| {
                    let window = cx
                        .open_window(window_options(&spec, cx), |window, cx| {
                            // As `Root` does: the shell has no such hook.
                            follow_system_appearance(window, cx);
                            let workspace = cx.new(|cx| {
                                let mut workspace =
                                    Workspace::item_panes(self.key_context.clone(), window, cx)
                                        .title(title.clone());
                                for panel in &self.panels {
                                    panel(&mut workspace, window, cx);
                                }
                                for panel in &panels {
                                    let made = (panel.make)(window, cx);
                                    made.add(panel.position, panel.id, &mut workspace, window, cx);
                                }
                                for (position, size) in &self.dock_sizes {
                                    workspace.set_dock_size(*position, *size, cx);
                                }
                                for source in &self.status {
                                    workspace.add_status_source(*source, cx);
                                }
                                for status in &statuses {
                                    workspace.add_status_source(status.item, cx);
                                }
                                for setup in &self.setup {
                                    setup(&mut workspace, window, cx);
                                }
                                if let Some(saved) = &saved {
                                    workspace.restore(saved, window, cx);
                                }
                                if let Some(layout) = layout {
                                    workspace.set_layout(layout, window, cx);
                                }
                                // Keys reach the shell only from focus
                                // within it: the empty pane's, unless setup
                                // or the restore chose another.
                                if window.focused(cx).is_none() {
                                    workspace.focus_active(window, cx);
                                }
                                workspace
                            });
                            crate::cx::record_workspace(window, &workspace, cx);
                            workspace
                        })
                        .expect("the app's window opens");
                    AppWindow(window.into())
                };
                launched
                    .into_iter()
                    .map(|launch| open(launch, cx))
                    .collect()
            }
        };
        Started {
            diagnostics,
            window: windows[0],
            windows,
            spec,
        }
    }
}

/// The app's saved window layouts, in the state directory's `layouts`: an
/// app has no root folder of its own, so the state directory stands for
/// one. `None` without a state directory.
fn layouts(paths: &AppPaths) -> Option<LayoutStore> {
    Some(LayoutStore::new(paths.layouts_dir()?, paths.state_dir()?))
}

/// `name` lowercased, each run of other characters than letters and digits
/// a `-`, none at either end; the last part of `id` the same way if that
/// leaves nothing, or `app`.
fn short_name(name: &str, id: &str) -> String {
    let short = |text: &str| {
        let mut short = String::new();
        for word in text
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|word| !word.is_empty())
        {
            if !short.is_empty() {
                short.push('-');
            }
            short.extend(word.chars().flat_map(char::to_lowercase));
        }
        short
    };
    [name, id.rsplit('.').next().unwrap_or(id)]
        .into_iter()
        .map(short)
        .find(|short| !short.is_empty())
        .unwrap_or_else(|| "app".to_owned())
}

fn native_paths(id: &str, short: &str, name: &str) -> AppPaths {
    AppPaths::new(&AppId::new(id, short, name.to_owned()), PathStyle::Native)
}
