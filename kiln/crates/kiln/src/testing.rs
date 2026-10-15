// SPDX-License-Identifier: LicenseRef-Proprietary

//! Test support for an app on kiln, behind the `test-support` feature: a
//! [`TestApp`] opens a [`kiln::app`](crate::app()) headless, as on Linux,
//! in temporary directories so that no user file is read, and drives it by
//! keys, commands and menus; [`assert_clean_config`] checks an app's keymap
//! and menus as on every platform.

use std::ffi::OsString;

use gpui::{App, Entity, TestAppContext, VisualTestContext};
use kiln_command::Invocation;
use kiln_core::{AppId, AppPaths, Os, PathStyle};
use kiln_platform::{Capabilities, config::batch_channel, menus};
use kiln_shell::{ItemPane, PaneItem, Workspace};
use tempfile::TempDir;

use crate::{
    AppBuilder, AppWindow, Config, Started, WindowSpec, contributions::shipped_catalog, prelude::*,
};

/// An app opened headless: see [`TestApp::open`].
pub struct TestApp {
    started: Started,
    cx: VisualTestContext,
    /// The directories the app keeps its files in, removed on drop.
    _dirs: TempDir,
}

impl TestApp {
    /// Starts the app `builder` describes headless, as on Linux whatever
    /// the host ([`Config::capabilities`]): its keymap and menus, its
    /// windows' options, and the chrome of a workspace or a
    /// [`Root`](crate::Root), from the start. Its directories are temporary
    /// ones, empty, and its user files, settings too, are read from there
    /// whatever path a `.config(..)` hook gave them
    /// ([`Config::user_files_in`]), so that no user file is read or
    /// watched; a settings store the app loaded itself
    /// ([`Config::settings_store`]) is the app's to point elsewhere. The
    /// test platform answers prompts; `init` must report no diagnostics;
    /// and the window is drawn.
    ///
    /// # Panics
    ///
    /// If `init` returns a diagnostic, or as [`AppBuilder::start`] does.
    pub fn open(builder: AppBuilder, cx: &mut TestAppContext) -> TestApp {
        let dirs = TempDir::new().expect("a temporary directory");
        let home: OsString = dirs.path().into();
        // Every directory, on any platform, under the temporary one.
        let paths = AppPaths::resolve(
            &AppId::new("dev.kiln.Test", "kiln-test", "Kiln Test"),
            PathStyle::Native,
            Os::current(),
            |name| match name {
                "HOME" | "APPDATA" | "LOCALAPPDATA" => Some(home.clone()),
                _ => None,
            },
        );
        let (_, closed) = batch_channel();
        // Last, so that it holds whatever the app's `.config(..)` hooks
        // set: the user's files are looked up in the temporary directory,
        // where there are none, when `init` loads them.
        let config_dir = dirs.path().join("config");
        let builder = builder.paths(paths).config(move |config| {
            config
                .user_files_in(config_dir)
                .watch_with(closed)
                .capabilities(Capabilities::for_os(Os::Linux))
        });
        let started = cx.update(|cx| {
            let started = builder.start(cx);
            assert_eq!(started.diagnostics, [], "kiln::init's diagnostics");
            // The test platform answers prompts (`simulate_prompt_answer`),
            // not the in-window prompt `init` sets up where there are no
            // native ones.
            cx.reset_prompt_builder();
            started
        });
        let cx = VisualTestContext::from_window(started.window.0, cx);
        let mut app = TestApp {
            started,
            cx,
            _dirs: dirs,
        };
        app.draw();
        app
    }

    /// Runs until parked and draws a frame, if the window is still open: a
    /// command that closes it leaves nothing to draw.
    pub fn draw(&mut self) {
        self.cx.run_until_parked();
        let window = self.started.window;
        let drawn = window.update(&mut self.cx, |_, window, cx| window.draw(cx).clear(cx));
        if drawn.is_ok() {
            self.cx.run_until_parked();
        }
    }

    /// Presses `keys`, space-separated, as GPUI writes them, with
    /// `secondary` the app's platform's, Linux's `ctrl`:
    /// `press("secondary-shift-p")`. Then runs until parked and draws.
    pub fn press(&mut self, keys: &str) {
        let os = self.cx.read(Capabilities::of).os;
        let secondary = if os == Os::MacOs { "cmd" } else { "ctrl" };
        self.cx
            .simulate_keystrokes(&keys.replace("secondary", secondary));
        self.draw();
    }

    /// Runs the command `id` (`chat.new_conversation`) in the window, from
    /// its focused element, then draws.
    ///
    /// # Panics
    ///
    /// If `id` isn't a command ID, or the command can't be dispatched.
    pub fn run_command(&mut self, id: &str) {
        let invocation = Invocation::of(id).unwrap_or_else(|error| panic!("{error}"));
        let result = self
            .cx
            .update(|window, cx| window.run_command(invocation, cx));
        result.unwrap_or_else(|error| panic!("command {id}: {error}"));
        self.draw();
    }

    /// Chooses the item titled `item` (or with that ID) of `menu` as in a
    /// menu drawn in the window, then draws: `menu("Conversation", "New
    /// Conversation")`.
    ///
    /// # Panics
    ///
    /// If the menu bar has no such item.
    pub fn menu(&mut self, menu: &str, item: &str) {
        self.cx.update(|window, cx| {
            let id = menus::installed(cx)
                .and_then(|bar| {
                    let items = &bar.menu(menu)?.items;
                    let found = items.iter().find(|candidate| {
                        let title = candidate.command().map(|command| command.title.as_str());
                        title == Some(item) || candidate.id() == Some(item)
                    });
                    found?.id().map(str::to_owned)
                })
                .unwrap_or_else(|| panic!("no menu item {menu} > {item}"));
            menus::run(menu, &id, window, cx);
        });
        self.draw();
    }

    /// The window's workspace.
    ///
    /// # Panics
    ///
    /// If the window's root isn't a [`Workspace`] of [`ItemPane`]s: it is
    /// a [`Root`](crate::Root), from [`AppBuilder::root`].
    pub fn workspace(&mut self) -> Entity<Workspace<ItemPane>> {
        self.started
            .window
            .root::<Workspace<ItemPane>>(&mut self.cx)
            .expect("the app's window is a workspace")
    }

    /// The active item of the workspace's active pane, if it is a `T`.
    pub fn active_item<T: PaneItem>(&mut self) -> Option<Entity<T>> {
        let workspace = self.workspace();
        self.cx.read(|cx| {
            let item = workspace.read(cx).active_item(cx)?;
            item.downcast_ref::<T>().cloned()
        })
    }

    /// The app's first window.
    pub fn window(&self) -> AppWindow {
        self.started.window
    }

    /// What the app's windows were opened from: their options are
    /// [`window_options`](crate::window_options) of it.
    pub fn spec(&self) -> &WindowSpec {
        &self.started.spec
    }

    /// The window's test context, for what `TestApp` has no call for.
    pub fn cx(&mut self) -> &mut VisualTestContext {
        &mut self.cx
    }
}

/// Checks `config`'s keymap and menus as on macOS, Linux and Windows
/// against the commands registered in `cx` and the config's own catalogs
/// ([`Config::catalog`]), as `init` checks them, for an app that
/// keeps its own [`init`](crate::init) rather than [`TestApp`]. Call it
/// once the commands are registered.
///
/// # Panics
///
/// Listing every diagnostic with the OS it was found on and its file.
pub fn assert_clean_config(config: &Config, cx: &App) {
    let catalog = shipped_catalog(config, cx);
    let mut found = Vec::new();
    for os in Os::ALL {
        let (_, keymap) = config.keymap_for(os, Some(&*catalog));
        let (_, menus) = config.menu_bar_for(os, Some(&*catalog));
        for diagnostic in keymap.into_iter().chain(menus) {
            found.push(format!(
                "{os:?}: {} ({})",
                diagnostic,
                diagnostic.file.display()
            ));
        }
    }
    assert!(
        found.is_empty(),
        "the config has {} diagnostics:\n{}",
        found.len(),
        found.join("\n")
    );
}
