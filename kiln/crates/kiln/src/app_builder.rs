// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`app`]: kiln's conventions for an app's files, settings, quitting and
//! first window, composed over [`init`] and [`Config`], each with a
//! one-line override.

use std::{borrow::Cow, path::PathBuf};

use gpui::{AnyView, App, AppContext as _, QuitMode, SharedString, Window, WindowHandle};
use kiln_core::{AppId, AppPaths, Diagnostic, PathStyle, SettingsType};
use kiln_platform::WindowSpec;
use kiln_shell::{Root, close};
use kiln_ui::ThemeMode;

use crate::setup::{Config, application, init, window_options};

/// Changes the [`Config`] the conventions built: see [`AppBuilder::config`].
type ConfigHook = Box<dyn FnOnce(Config) -> Config>;

/// Adds a settings store, given the user's `settings.toml`.
type AddSettings = Box<dyn FnOnce(Config, Option<PathBuf>) -> Config>;

type WindowHook = Box<dyn FnOnce(WindowSpec) -> WindowSpec>;

type RootView = Box<dyn FnOnce(&mut Window, &mut App) -> AnyView>;

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
/// | One window, from [`WindowSpec::main`] with the app's name and ID | [`AppBuilder::window`] |
///
/// [`AppBuilder::commands`], [`AppBuilder::keymap`], [`AppBuilder::menus`],
/// [`AppBuilder::theme_mode`] and [`AppBuilder::theme_extensions`] are
/// [`Config`]'s, and [`AppBuilder::config`] changes anything else on it.
/// The window's view comes from [`AppBuilder::root`], which is required.
///
/// ```ignore
/// kiln::app("com.example.Notes", "Notes")
///     .commands(notes::register)
///     .keymap("keymap.toml", include_str!("keymap.toml"))
///     .menus("menus.toml", include_str!("menus.toml"))
///     .settings::<NotesSettings>()
///     .root(|window, cx| cx.new(|cx| Notes::new(window, cx)).into())
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
///             .menus("menus.toml", include_str!("menus.toml"))
///             .settings::<NotesSettings>(paths.settings_file(), None)
///             .user_keymap(config_dir.join("keymap.toml"))
///             .user_theme(config_dir.join("theme.toml")),
///     );
///     cx.set_quit_mode(QuitMode::Explicit);
///     cx.on_window_closed(|cx, _| {
///         if cx.windows().is_empty() {
///             kiln::shell::close::quit(cx);
///         }
///     })
///     .detach();
///     let spec = WindowSpec::main("Notes").app_id("com.example.Notes");
///     cx.open_window(kiln::window_options(&spec, cx), |window, cx| {
///         let notes = cx.new(|cx| Notes::new(window, cx));
///         cx.new(|cx| kiln::Root::new(notes, window, cx).title("Notes"))
///     })
///     .unwrap();
///     cx.activate(true);
/// });
/// ```
pub fn app(id: impl Into<String>, name: impl Into<SharedString>) -> AppBuilder {
    let id = id.into();
    let name = name.into();
    let short = short_name(&name, &id);
    let paths = native_paths(&id, &short, &name);
    AppBuilder {
        config: Config::new(id.clone(), name.clone()),
        id,
        name,
        short,
        paths,
        paths_given: false,
        settings: Vec::new(),
        hooks: Vec::new(),
        quit_on_last_window: !cfg!(target_os = "macos"),
        window: None,
        root: None,
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
}

/// What [`AppBuilder::start`] did.
pub struct Started {
    /// [`init`]'s diagnostics: the keymap's, menus', settings' and theme's.
    pub diagnostics: Vec<Diagnostic>,
    /// The window it opened, kiln's [`Root`] around the app's view.
    pub window: WindowHandle<Root>,
    /// What that window was opened from, the app's ID in it.
    pub spec: WindowSpec,
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

    /// The window's view, made once the window opens: kiln's [`Root`] is
    /// around it.
    pub fn root(mut self, view: impl FnOnce(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.root = Some(Box::new(view));
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

    /// Sets the app up in `cx`: [`init`] with the [`Config`], quitting on
    /// the last window's close if it should (and GPUI's [`QuitMode`]
    /// [`QuitMode::Explicit`]), then the window.
    ///
    /// # Panics
    ///
    /// Without [`AppBuilder::root`], or if the window doesn't open.
    pub fn start(self, cx: &mut App) -> Started {
        let root = self
            .root
            .expect("kiln::app needs `.root(..)`: the window's view");
        let mut config = self.config;
        let user_settings = self.paths.settings_file();
        for add in self.settings {
            config = add(config, user_settings.clone());
        }
        if let Some(dir) = self.paths.config_dir() {
            config = config
                .user_keymap(dir.join("keymap.toml"))
                .user_theme(dir.join("theme.toml"));
        }
        for change in self.hooks {
            config = change(config);
        }
        let diagnostics = init(cx, config);

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
        let window = cx
            .open_window(window_options(&spec, cx), move |window, cx| {
                let view = root(window, cx);
                cx.new(|cx| Root::new(view, window, cx).title(title))
            })
            .expect("the app's window opens");
        Started {
            diagnostics,
            window,
            spec,
        }
    }
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
