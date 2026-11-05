// SPDX-License-Identifier: Apache-2.0

//! `kiln::init` with the user's files: settings, keymap and theme, each
//! reloaded as the config service hears of a change, headless. The batches
//! are sent as the watcher would send them.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::TestAppContext;
use kiln::{
    Config, GlobalSettings,
    core::{Layer, ProjectFilter, SettingsOptions, SettingsStore},
    core::{Os, Severity},
    platform::{
        config::{self, BatchSender, batch_channel},
        keys,
        menus::MenuCommand,
    },
    prelude::*,
    serde::{Deserialize, Serialize},
    ui::{Appearance, GlobalTheme, Theme, ThemeMode, hsla},
};

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
        ]
    );
}

const KEYMAP: &str = r#"
[[bindings]]
keys = "secondary-n"
command = "notes.new_note"
"#;

const MENUS: &str = r#"
[[menus]]
name = "File"

[[menus.items]]
title = "New Note"
command = "notes.new_note"
"#;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(crate = "kiln::serde", default)]
struct NotesSettings {
    font_size: f64,
    wrap: bool,
}

impl Default for NotesSettings {
    fn default() -> Self {
        Self {
            font_size: 13.0,
            wrap: true,
        }
    }
}

/// A directory under the system's temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-init-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn config() -> Config {
    Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .keymap("keymap.toml", KEYMAP)
        .menus("menus.toml", MENUS)
}

/// Sets the app up with `config`, its files' changes sent through the
/// sender returned.
fn init(config: Config, cx: &mut TestAppContext) -> BatchSender {
    let (sender, batches) = batch_channel();
    let diagnostics = cx.update(|cx| kiln::init(cx, config.watch_with(batches)));
    assert_eq!(diagnostics, []);
    cx.run_until_parked();
    sender
}

/// Writes `text` to `path` and sends a batch naming it.
fn save(path: &Path, text: &str, sender: &BatchSender, cx: &mut TestAppContext) {
    fs::write(path, text).unwrap();
    sender.send(vec![path.to_owned()]);
    cx.run_until_parked();
}

fn font_size(cx: &mut TestAppContext) -> f64 {
    cx.update(|cx| cx.settings::<NotesSettings>().font_size)
}

#[gpui::test]
fn settings_load_and_reload_through_init(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let user = dir.path("settings.toml");
    let project = dir.path("project.toml");
    fs::write(&user, "font_size = 15\n").unwrap();
    fs::write(&project, "wrap = false\n").unwrap();
    let sender = init(
        config().settings::<NotesSettings>(Some(user.clone()), Some(project)),
        cx,
    );
    assert_eq!(
        *cx.update(|cx| cx.settings::<NotesSettings>()),
        NotesSettings {
            font_size: 15.0,
            wrap: false,
        }
    );

    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    let _observing = cx.update(|cx| {
        let notified = notified.clone();
        cx.observe_global::<GlobalSettings<NotesSettings>>(move |_| {
            notified.set(notified.get() + 1)
        })
    });
    save(&user, "font_size = 17\n", &sender, cx);
    assert_eq!(font_size(cx), 17.0);
    assert_eq!(notified.get(), 1);

    // An error keeps the last good settings, and says where it is.
    save(&user, "font_size = \"big\"\n", &sender, cx);
    assert_eq!(font_size(cx), 17.0);
    let diagnostics = cx.update(|cx| config::diagnostics(cx).to_vec());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(diagnostics[0].key.as_deref(), Some("font_size"));
}

#[gpui::test]
fn without_settings_they_are_the_defaults(cx: &mut TestAppContext) {
    init(config(), cx);
    assert_eq!(
        *cx.update(|cx| cx.settings::<NotesSettings>()),
        NotesSettings::default()
    );
}

/// The menus' GPUI key bindings for File > New Note, as GPUI spells them.
fn new_note_bindings(cx: &mut TestAppContext) -> Vec<String> {
    let action = MenuCommand {
        menu: "File".into(),
        item: "notes.new_note".into(),
    };
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keymap
            .bindings_for_action(&action)
            .map(|binding| binding.keystrokes()[0].inner().unparse())
            .collect()
    })
}

/// `keys` with this platform's `secondary`, as GPUI spells it.
fn gpui_keys(keys: &str) -> String {
    let secondary = if Os::current() == Os::MacOs {
        "cmd"
    } else {
        "ctrl"
    };
    gpui::Keystroke::parse(&keys.replace("secondary", secondary))
        .unwrap()
        .unparse()
}

#[gpui::test]
fn the_user_keymap_hot_reloads_with_the_menus(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let user = dir.path("keymap.toml");
    let sender = init(config().user_keymap(&user), cx);
    assert_eq!(new_note_bindings(cx), [gpui_keys("secondary-n")]);
    assert_eq!(
        cx.update(|cx| cx.binding_label(&notes::NewNote::id())),
        Some(
            if Os::current() == Os::MacOs {
                "⌘N"
            } else {
                "Ctrl+N"
            }
            .into()
        )
    );

    let rebind = r#"
        [[bindings]]
        keys = "secondary-shift-n"
        command = "notes.new_note"

        [[bindings]]
        keys = "secondary-n"
        command = ""
    "#;
    save(&user, rebind, &sender, cx);
    assert_eq!(new_note_bindings(cx), [gpui_keys("secondary-shift-n")]);
    let bound = cx.update(|cx| {
        keys::current_keymap(cx)
            .bindings_for(&notes::NewNote::id())
            .len()
    });
    assert_eq!(bound, 1);

    // A syntax error keeps the last good keymap.
    save(&user, "[[bindings]\n", &sender, cx);
    assert_eq!(new_note_bindings(cx), [gpui_keys("secondary-shift-n")]);
    let diagnostics = cx.update(|cx| config::diagnostics(cx).to_vec());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].file, user);
}

const RED: &str = "kiln_theme = 1\nappearance = \"dark\"\n[accent]\nbase = \"#ff0000\"\n";
const GREEN: &str = "kiln_theme = 1\nappearance = \"dark\"\n[accent]\nbase = \"#00ff00\"\n";

fn accent(cx: &mut TestAppContext) -> gpui::Hsla {
    cx.update(|cx| cx.theme().colors.accent.base)
}

fn color(hex: u32) -> gpui::Hsla {
    hsla(kiln::ui::Color::rgba(hex << 8 | 0xff))
}

#[gpui::test]
fn the_user_theme_takes_its_appearances_place_and_hot_reloads(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let path = dir.path("theme.toml");
    fs::write(&path, RED).unwrap();
    let sender = init(config().theme_mode(ThemeMode::Dark).user_theme(&path), cx);
    assert_eq!(accent(cx), color(0xff0000));
    assert_eq!(cx.update(|cx| cx.theme().name.clone()), "theme");

    save(&path, GREEN, &sender, cx);
    assert_eq!(accent(cx), color(0x00ff00));

    // The mode still picks between dark and light, the user's theme dark.
    cx.update(|cx| cx.set_theme_mode(ThemeMode::Light));
    assert_eq!(cx.update(|cx| cx.theme().clone()), Theme::light());
    cx.update(|cx| cx.set_theme_mode(ThemeMode::Dark));
    assert_eq!(accent(cx), color(0x00ff00));

    // A file that isn't TOML keeps the last good theme.
    save(&path, "accent = [\n", &sender, cx);
    assert_eq!(accent(cx), color(0x00ff00));
    let diagnostics = cx.update(|cx| config::diagnostics(cx).to_vec());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");

    // A light theme goes in the light slot, and kiln's dark comes back.
    let light = "kiln_theme = 1\nappearance = \"light\"\n[accent]\nbase = \"#0000ff\"\n";
    save(&path, light, &sender, cx);
    assert_eq!(cx.update(|cx| cx.theme().clone()), Theme::dark());
    cx.update(|cx| cx.set_theme_mode(ThemeMode::Light));
    assert_eq!(accent(cx), color(0x0000ff));
    assert_eq!(cx.update(|cx| cx.theme().appearance), Appearance::Light);
    assert_eq!(cx.update(|cx| config::diagnostics(cx).to_vec()), []);

    // Without the file, kiln's own pair is back.
    fs::remove_file(&path).unwrap();
    sender.send(vec![path.clone()]);
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.theme().clone()), Theme::light());
    assert_eq!(
        cx.update(|cx| cx.global::<GlobalTheme>().mode()),
        Some(ThemeMode::Light)
    );
}

#[gpui::test]
fn a_batch_of_other_files_changes_nothing(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let settings = dir.path("settings.toml");
    let theme = dir.path("theme.toml");
    let keymap = dir.path("keymap.toml");
    fs::write(&settings, "font_size = 15\n").unwrap();
    fs::write(&theme, RED).unwrap();
    let sender = init(
        config()
            .settings::<NotesSettings>(Some(settings.clone()), None)
            .user_keymap(&keymap)
            .user_theme(&theme)
            .theme_mode(ThemeMode::Dark),
        cx,
    );
    // Every file changes, but the batch names another.
    fs::write(&settings, "font_size = 20\n").unwrap();
    fs::write(&theme, GREEN).unwrap();
    fs::write(
        &keymap,
        "[[bindings]]\nkeys = \"secondary-n\"\ncommand = \"\"\n",
    )
    .unwrap();
    sender.send(vec![dir.path("other.toml")]);
    cx.run_until_parked();
    assert_eq!(font_size(cx), 15.0);
    assert_eq!(accent(cx), color(0xff0000));
    assert_eq!(new_note_bindings(cx), [gpui_keys("secondary-n")]);
}

#[gpui::test]
fn init_creates_the_user_config_directory(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let config_dir = dir.path("config");
    let settings = config_dir.join("settings.toml");
    let (_, closed) = batch_channel();
    cx.update(|cx| {
        kiln::init(
            cx,
            config()
                .settings::<NotesSettings>(Some(settings), None)
                .watch_with(closed),
        )
    });
    assert!(config_dir.is_dir());
}

#[gpui::test]
fn a_settings_store_with_options_reloads_through_init(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let user = dir.path("settings.toml");
    let project = dir.path("project.toml");
    fs::write(&user, "font_size = 15\n").unwrap();
    fs::write(&project, "font_size = 30\nwrap = false\n").unwrap();
    // A project may set wrap, but not the font size.
    let only_wrap = ProjectFilter::new("font_size", |layer: &mut Layer| {
        layer.table.remove("font_size");
    });
    let options = SettingsOptions::new().project_filter(only_wrap);
    let store = SettingsStore::<NotesSettings>::load_with(
        Some(user.clone()),
        Some(project.clone()),
        options,
    );
    let reports = std::rc::Rc::new(std::cell::Cell::new(0));
    let config = config()
        .settings_store(store)
        .log_target("notes")
        .on_keymap_reload({
            let reports = reports.clone();
            move |_| reports.set(reports.get() + 1)
        });
    let sender = init(config, cx);
    assert_eq!(
        *cx.update(|cx| cx.settings::<NotesSettings>()),
        NotesSettings {
            font_size: 15.0,
            wrap: false,
        }
    );
    assert_eq!(cx.update(|cx| config::log_target(cx)), "notes");

    save(&project, "font_size = 40\nwrap = true\n", &sender, cx);
    assert_eq!(font_size(cx), 15.0, "still narrowed");
    assert!(cx.update(|cx| cx.settings::<NotesSettings>().wrap));
    assert_eq!(reports.get(), 0, "no keymap given, none reloaded");
}

#[gpui::test]
fn on_keymap_reload_follows_each_keymap_change(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let user = dir.path("keymap.toml");
    let reports = std::rc::Rc::new(std::cell::Cell::new(0));
    let config = config().user_keymap(&user).on_keymap_reload({
        let reports = reports.clone();
        move |cx| {
            // Once the keymap is in use.
            let bound = keys::current_keymap(cx)
                .bindings_for(&notes::NewNote::id())
                .len();
            reports.set(reports.get() + bound);
        }
    });
    let sender = init(config, cx);
    assert_eq!(reports.get(), 0);
    let binding = "[[bindings]]\nkeys = \"secondary-shift-n\"\ncommand = \"notes.new_note\"\n";
    save(&user, binding, &sender, cx);
    assert_eq!(reports.get(), 2, "called once, with both bindings in use");
    // The same file again swaps nothing.
    save(&user, binding, &sender, cx);
    assert_eq!(reports.get(), 2);
}
