// SPDX-License-Identifier: Apache-2.0

//! `kiln::testing`'s isolation: settings a `.config(..)` hook names in the
//! user's own config directory are never read, and the app is Linux's from
//! the start, whatever the host: its window options, its keys, and the
//! chrome of a workspace and of a custom root.

use std::{ffi::OsString, fs};

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, Render, SharedString, Styled as _, Task, TestAppContext,
    Window, div,
};
use kiln::{
    AppBuilder, Root,
    command::{CommandId, Value},
    core::{AppId, AppPaths, Os, PathStyle},
    platform::{Capabilities, keys},
    prelude::*,
    serde::{Deserialize, Serialize},
    shell::{PaneItem, command_palette::COMMAND_PALETTE},
    testing::TestApp,
};
use tempfile::TempDir;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(crate = "kiln::serde", default)]
struct NotesSettings {
    font_size: f64,
}

struct Note(FocusHandle);

impl Render for Note {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.0).size_full()
    }
}

impl Focusable for Note {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.0.clone()
    }
}

impl PaneItem for Note {
    const KIND: &'static str = "test.note";

    fn tab_title(&self, _: &App) -> SharedString {
        "note".into()
    }

    fn restore(_: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        Task::ready(Some(cx.new(|cx| Note(cx.focus_handle()))))
    }
}

fn notes() -> AppBuilder {
    kiln::app("dev.kiln.Notes", "Kiln Notes")
        .quit_on_last_window(false)
        .item::<Note>()
}

/// The notes app's directories under `home`, as the user's own would be.
fn user_paths(home: &TempDir) -> AppPaths {
    let home: OsString = home.path().into();
    AppPaths::resolve(
        &AppId::new("dev.kiln.Notes", "kiln-notes", "Kiln Notes"),
        PathStyle::Native,
        Os::current(),
        |name| match name {
            "HOME" | "APPDATA" | "LOCALAPPDATA" => Some(home.clone()),
            _ => None,
        },
    )
}

#[gpui::test]
fn settings_a_config_hook_names_in_the_user_s_dir_are_not_read(cx: &mut TestAppContext) {
    let home = TempDir::new().unwrap();
    let user = user_paths(&home).settings_file().unwrap();
    fs::create_dir_all(user.parent().unwrap()).unwrap();
    fs::write(&user, "font_size = 99.0\n").unwrap();
    // The hook names the user's file itself, as from `app_paths()` taken
    // before the test's directories apply.
    let builder = notes().config(move |config| config.settings::<NotesSettings>(Some(user), None));
    let mut app = TestApp::open(builder, cx);
    let settings = app.cx().read(|cx| cx.settings::<NotesSettings>());
    assert_eq!(*settings, NotesSettings::default());
}

/// The keys `command_palette.toggle` is bound to now.
fn palette_keys(cx: &App) -> Vec<String> {
    let keymap = keys::current_keymap(cx);
    let command = CommandId::parse("command_palette.toggle").unwrap();
    (keymap.bindings_for(&command).iter())
        .map(|binding| binding.keys.to_string())
        .collect()
}

/// The options the app's windows opened with.
fn options(app: &mut TestApp) -> gpui::WindowOptions {
    let spec = app.spec().clone();
    app.cx().read(|cx| kiln::window_options(&spec, cx))
}

fn assert_linux_options(options: &gpui::WindowOptions) {
    assert!(!options.app_owns_titlebar_drag);
    let titlebar = options.titlebar.as_ref().unwrap();
    assert!(!titlebar.appears_transparent);
    assert_eq!(titlebar.traffic_light_position, None);
}

/// The chrome drawn: the window manager's title bar, not the unified one,
/// and the menus in the window's top row.
fn assert_linux_chrome(app: &mut TestApp) {
    assert_eq!(
        app.cx().debug_bounds("title-bar"),
        None,
        "no unified title bar"
    );
    assert!(
        app.cx().debug_bounds("window-drag").is_some(),
        "the top row the in-window menus are in"
    );
}

#[gpui::test]
fn a_workspace_is_linux_s_from_the_start(cx: &mut TestAppContext) {
    let mut app = TestApp::open(notes(), cx);
    assert_linux_options(&options(&mut app));
    assert_linux_chrome(&mut app);
    let shell = (app.workspace()).read_with(app.cx(), |workspace, _| workspace.shell().clone());
    let menus = shell.read_with(app.cx(), |shell, _| shell.menu_bar().is_some());
    assert!(menus, "menus in the window");

    assert_eq!(app.cx().read(palette_keys), ["ctrl-shift-p"]);
    app.press("cmd-shift-p");
    let shell_modal = |app: &mut TestApp| shell.read_with(app.cx(), |shell, cx| shell.modal(cx));
    assert_eq!(shell_modal(&mut app), None, "macOS's keys aren't bound");
    app.press("ctrl-shift-p");
    assert_eq!(shell_modal(&mut app), Some(COMMAND_PALETTE));
}

struct View;

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn view(_: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|_| View).into()
}

#[gpui::test]
fn a_custom_root_is_linux_s_from_the_start(cx: &mut TestAppContext) {
    let mut app = TestApp::open(notes().root(view), cx);
    assert_linux_options(&options(&mut app));
    assert_linux_chrome(&mut app);
    let root = app.window().root::<Root>(app.cx()).expect("a Root window");
    let menus = root.read_with(app.cx(), |root, _| root.menu_bar().is_some());
    assert!(menus, "menus in the window");
    assert_eq!(app.cx().read(palette_keys), ["ctrl-shift-p"]);
}

#[gpui::test]
fn an_app_started_without_test_app_is_this_platform_s(cx: &mut TestAppContext) {
    let home = TempDir::new().unwrap();
    let (_, closed) = kiln::platform::config::batch_channel();
    let builder = notes()
        .paths(user_paths(&home))
        .config(|config| config.watch_with(closed));
    cx.update(|cx| {
        let started = builder.start(cx);
        assert_eq!(Capabilities::of(cx), Capabilities::current());
        let unified = Capabilities::current().unified_title_bar;
        let options = kiln::window_options(&started.spec, cx);
        assert_eq!(options.app_owns_titlebar_drag, unified);
    });
}
