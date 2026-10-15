// SPDX-License-Identifier: LicenseRef-Proprietary

//! Test support for an app on kiln, behind the `test-support` feature: a
//! [`TestApp`] opens a [`kiln::app`](crate::app()) headless, in temporary
//! directories so that no user file is read, and drives it by keys,
//! commands and menus; [`assert_clean_config`] checks an app's keymap and
//! menus as on every platform.

use std::ffi::OsString;

use gpui::{App, AppContext as _, Entity, TestAppContext, VisualTestContext};
use kiln_command::Invocation;
use kiln_core::{AppId, AppPaths, Os, PathStyle};
use kiln_platform::{Capabilities, config::batch_channel, menus};
use kiln_shell::{ItemPane, MenuBarView, PaneItem, Workspace};
use tempfile::TempDir;

use crate::{AppBuilder, AppWindow, Config, Started, contributions::shipped_catalog, prelude::*};

/// An app opened headless: see [`TestApp::open`].
pub struct TestApp {
    started: Started,
    cx: VisualTestContext,
    /// The directories the app keeps its files in, removed on drop.
    _dirs: TempDir,
}

impl TestApp {
    /// Starts the app `builder` describes headless: its directories are
    /// temporary ones, empty, so that no user file is read or watched; the
    /// test platform answers prompts; `init` must report no diagnostics;
    /// and the window is drawn.
    ///
    /// Two limits, deferred to T718 (they need kiln-platform and
    /// kiln-shell): settings that a `.config(..)` hook loads from paths of
    /// its own are still read, as only the user keymap and theme are
    /// remapped; and the Linux chrome is applied to a workspace window
    /// after it opens, so its window options and a [`Root`](crate::Root)
    /// window stay the host's.
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
        // Last, so that no `.config(..)` of the app's names a user file: the
        // keymap and theme are looked up in the temporary directory, where
        // there are none. (A settings store the app loaded itself from a
        // path of its own is the app's to point elsewhere.)
        let config_dir = dirs.path().join("config");
        let builder = builder.paths(paths).config(move |config| {
            config
                .user_keymap(config_dir.join("keymap.toml"))
                .user_theme(config_dir.join("theme.toml"))
                .watch_with(closed)
        });
        let started = cx.update(|cx| {
            let started = builder.start(cx);
            assert_eq!(started.diagnostics, [], "kiln::init's diagnostics");
            // The test platform answers prompts (`simulate_prompt_answer`),
            // not the in-window prompt `init` sets up where there are no
            // native ones.
            cx.reset_prompt_builder();
            for window in &started.windows {
                if let Some(workspace) = window.root::<Workspace<ItemPane>>(cx) {
                    linux_chrome(&workspace, cx);
                }
            }
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
    /// `secondary` this platform's: `press("secondary-shift-p")`. Then
    /// runs until parked and draws.
    pub fn press(&mut self, keys: &str) {
        let secondary = if Os::current() == Os::MacOs {
            "cmd"
        } else {
            "ctrl"
        };
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

    /// The window's test context, for what `TestApp` has no call for.
    pub fn cx(&mut self) -> &mut VisualTestContext {
        &mut self.cx
    }
}

/// Draws the workspace's chrome as on Linux whatever the host: the menus
/// in the window, the window manager's title bar, and the frame.
fn linux_chrome(workspace: &Entity<Workspace<ItemPane>>, cx: &mut App) {
    let capabilities = Capabilities::for_os(Os::Linux);
    let shell = workspace.read(cx).shell().clone();
    shell.update(cx, |shell, cx| {
        shell.set_title_bar(capabilities.unified_title_bar, cx);
        let bar = (!capabilities.native_menus).then(|| cx.new(MenuBarView::new));
        shell.set_menu_bar(bar, cx);
        shell.set_window_frame(!capabilities.unified_title_bar, cx);
    });
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
