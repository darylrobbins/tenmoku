// SPDX-License-Identifier: LicenseRef-Proprietary

//! `kiln::app`: the conventions it applies over `kiln::init`, and the
//! override of each, headless.

use std::{
    cell::Cell,
    ffi::OsString,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::{AnyView, AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div};
use kiln::{
    AppBuilder, Root, Started,
    core::{AppId, AppPaths, Os, PathStyle},
    platform::{
        config::{self, Batches, batch_channel},
        keys,
    },
    prelude::*,
    serde::{Deserialize, Serialize},
    shell::close,
    ui::{ThemeMode, hsla},
};

const ID: &str = "dev.kiln.Chat";
const NAME: &str = "Kiln Chat";

mod chat {
    kiln::commands!(
        chat,
        [
            /// Send
            Send,
        ]
    );
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(crate = "kiln::serde", default)]
struct ChatSettings {
    font_size: f64,
}

struct View;

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn view(_: &mut Window, cx: &mut gpui::App) -> AnyView {
    cx.new(|_| View).into()
}

/// A directory under the system's temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-app-builder-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// The chat's directories under this one: `config` and `state`, as
    /// `KILN_CHAT_CONFIG_DIR` and `KILN_CHAT_STATE_DIR` would set them.
    fn paths(&self) -> AppPaths {
        let id = AppId::new(ID, "kiln-chat", NAME);
        let config: OsString = self.0.join("config").into();
        let state: OsString = self.0.join("state").into();
        AppPaths::resolve(&id, PathStyle::Native, Os::current(), |name| match name {
            "KILN_CHAT_CONFIG_DIR" => Some(config.clone()),
            "KILN_CHAT_STATE_DIR" => Some(state.clone()),
            _ => None,
        })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// The chat on `paths`, its files' changes never sent, and its window's
/// view an empty one.
fn chat(paths: AppPaths) -> AppBuilder {
    let (_, closed) = batch_channel();
    chat_watching(paths, closed)
}

/// The chat on `paths`, its files' changes taken from `batches`.
fn chat_watching(paths: AppPaths, batches: Batches) -> AppBuilder {
    kiln::app(ID, NAME)
        .paths(paths)
        .commands(chat::register)
        .root(view)
        .config(move |config| config.watch_with(batches))
}

fn start(app: AppBuilder, cx: &mut TestAppContext) -> Started {
    let started = cx.update(|cx| app.start(cx));
    cx.run_until_parked();
    started
}

#[test]
fn the_short_name_and_paths_follow_the_name_unless_given() {
    let app = kiln::app(ID, NAME);
    let id = AppId::new(ID, "kiln-chat", NAME);
    assert_eq!(*app.app_paths(), AppPaths::new(&id, PathStyle::Native));

    let short = kiln::app(ID, NAME).short_name("chat");
    let id = AppId::new(ID, "chat", NAME);
    assert_eq!(*short.app_paths(), AppPaths::new(&id, PathStyle::Native));

    // Given paths stay, whatever the short name.
    let dir = TempDir::new();
    let given = kiln::app(ID, NAME).paths(dir.paths()).short_name("chat");
    assert_eq!(*given.app_paths(), dir.paths());
    assert_eq!(given.app_paths().config_dir(), Some(&*dir.0.join("config")));

    for (name, short) in [
        ("  Kiln -- Chat! 2 ", "kiln-chat-2"),
        ("Café Notes", "café-notes"),
        ("Notes", "notes"),
    ] {
        let id = AppId::new(ID, short, name);
        assert_eq!(
            *kiln::app(ID, name).app_paths(),
            AppPaths::new(&id, PathStyle::Native),
            "{name}"
        );
    }
}

const RED: &str = "kiln_theme = 1\nappearance = \"dark\"\n[accent]\nbase = \"#ff0000\"\n";

const SEND: &str = "[[bindings]]\nkeys = \"secondary-enter\"\ncommand = \"chat.send\"\n";

/// The files the bindings for `chat.send` came from.
fn send_bindings(cx: &mut TestAppContext) -> Vec<PathBuf> {
    cx.update(|cx| {
        keys::current_keymap(cx)
            .bindings_for(&chat::Send::id())
            .into_iter()
            .map(|binding| binding.location.file.clone())
            .collect()
    })
}

#[gpui::test]
fn start_loads_the_users_settings_keymap_and_theme(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let config = dir.0.join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("settings.toml"), "font_size = 15\n").unwrap();
    fs::write(config.join("keymap.toml"), SEND).unwrap();
    fs::write(config.join("theme.toml"), RED).unwrap();

    let app = chat(dir.paths())
        .settings::<ChatSettings>()
        .theme_mode(ThemeMode::Dark);
    let started = start(app, cx);
    assert_eq!(started.diagnostics, []);
    assert_eq!(
        cx.update(|cx| cx.settings::<ChatSettings>().font_size),
        15.0
    );
    assert_eq!(send_bindings(cx), [config.join("keymap.toml")]);
    let red = hsla(kiln::ui::Color::rgba(0xff0000ff));
    assert_eq!(cx.update(|cx| cx.theme().colors.accent.base), red);
}

#[gpui::test]
fn config_runs_last_and_replaces_a_convention(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let config = dir.0.join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("keymap.toml"), SEND).unwrap();
    let other = dir.0.join("other.toml");
    fs::write(&other, SEND).unwrap();

    let seen = Rc::new(Cell::new(false));
    let app = chat(dir.paths()).config({
        let (other, seen) = (other.clone(), seen.clone());
        move |config| {
            // The conventions are in: what's set here replaces them.
            let keymap = config.keymap_for(Os::current(), None).0;
            seen.set(keymap.bindings_for(&chat::Send::id()).len() == 1);
            config.user_keymap(other)
        }
    });
    start(app, cx);
    assert!(seen.get(), "the conventional keymap was in the config");
    assert_eq!(send_bindings(cx), [other]);
}

#[gpui::test]
fn a_settings_override_replaces_the_conventional_store(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let config_dir = dir.0.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let conventional = config_dir.join("settings.toml");
    let other = dir.0.join("other.toml");
    fs::write(&conventional, "font_size = 15\n").unwrap();
    fs::write(&other, "font_size = 20\n").unwrap();

    let (sender, batches) = batch_channel();
    let app = chat_watching(dir.paths(), batches)
        .settings::<ChatSettings>()
        .config({
            let other = other.clone();
            move |config| config.settings::<ChatSettings>(Some(other), None)
        });
    start(app, cx);
    let font_size =
        |cx: &mut TestAppContext| cx.update(|cx| cx.settings::<ChatSettings>().font_size);
    assert_eq!(font_size(cx), 20.0);

    // The conventional file is no store's: its changes and errors are
    // nobody's.
    for text in ["font_size = 30\n", "font_size = \"big\"\n"] {
        fs::write(&conventional, text).unwrap();
        sender.send(vec![conventional.clone()]);
        cx.run_until_parked();
        assert_eq!(font_size(cx), 20.0);
        assert_eq!(cx.update(|cx| config::diagnostics(cx).to_vec()), []);
    }

    // The override's are.
    fs::write(&other, "font_size = 25\n").unwrap();
    sender.send(vec![other.clone()]);
    cx.run_until_parked();
    assert_eq!(font_size(cx), 25.0);
    fs::write(&other, "font_size = \"big\"\n").unwrap();
    sender.send(vec![other.clone()]);
    cx.run_until_parked();
    assert_eq!(font_size(cx), 25.0);
    let diagnostics = cx.update(|cx| config::diagnostics(cx).to_vec());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].file, other);
}

/// Closes the app's only window, counting the quits that follow.
fn close_the_last_window(app: AppBuilder, cx: &mut TestAppContext) -> usize {
    let quits = Rc::new(Cell::new(0));
    let started = start(app, cx);
    cx.update(|cx| {
        let quits = quits.clone();
        close::set_quit(cx, move |_| quits.set(quits.get() + 1));
    });
    started
        .window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.windows().len()), 0);
    quits.get()
}

#[gpui::test]
fn closing_the_last_window_quits_if_it_should(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let app = chat(dir.paths()).quit_on_last_window(true);
    assert_eq!(close_the_last_window(app, cx), 1);
}

#[gpui::test]
fn closing_the_last_window_stays_if_it_should(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let app = chat(dir.paths()).quit_on_last_window(false);
    assert_eq!(close_the_last_window(app, cx), 0);
}

#[gpui::test]
fn by_default_the_last_window_quits_except_on_macos(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let quits = close_the_last_window(chat(dir.paths()), cx);
    assert_eq!(quits, if cfg!(target_os = "macos") { 0 } else { 1 });
}

#[gpui::test]
fn root_opens_one_window_of_roots_around_the_view(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let made = Rc::new(Cell::new(None));
    let app = chat(dir.paths()).root({
        let made = made.clone();
        move |window, cx| {
            let view = view(window, cx);
            made.set(Some(view.entity_id()));
            view
        }
    });
    let started = start(app, cx);
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);
    let root: gpui::Entity<Root> = started.window.root(cx).unwrap();
    let content = cx.read(|cx| root.read(cx).content().entity_id());
    assert_eq!(Some(content), made.get());

    assert_eq!(started.spec.title, NAME);
    let options = cx.update(|cx| kiln::window_options(&started.spec, cx));
    assert_eq!(options.app_id.as_deref(), Some(ID));
    let info = cx.update(|cx| cx.global::<kiln::AppInfo>().clone());
    assert_eq!((info.id.as_str(), info.name.as_ref()), (ID, NAME));
}

#[gpui::test]
fn window_changes_the_spec(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let app = chat(dir.paths()).window(|mut spec| {
        spec.title = "Chat".into();
        spec
    });
    let started = start(app, cx);
    assert_eq!(started.spec.title, "Chat");
    assert_eq!(started.spec.app_id.as_deref(), Some(ID));
}

#[test]
fn the_function_and_the_commands_module_both_resolve() {
    let app: AppBuilder = kiln::app(ID, NAME);
    assert!(app.app_paths().config_dir().is_some());
    assert_eq!(kiln::app::Quit::id().as_str(), "app.quit");
}
